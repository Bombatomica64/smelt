//! Reading an operand: its owned or borrowed Rust expression, its MIR type, and
//! the rendered type text helpers those reads share.

use super::*;
use crate::rust::{RustExpr, RustType};

impl FunctionEmitter<'_> {
    /// Converts a type ID to Rust text for storage positions.
    ///
    /// Struct fields, generic arguments, and other named storage positions
    /// cannot use root `impl Trait`, so function values are rendered as boxed
    /// trait objects here.
    pub(crate) fn type_text_for(mir: &Mir, ty: TypeId) -> Result<String, EmitError> {
        let context = EmitContext::new(mir)?;
        Self::type_text_for_with_context(mir, &context, ty)
    }

    /// Converts a type ID to Rust text using an existing emission context.
    pub(crate) fn type_text_for_with_context(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        FunctionEmitter::type_renderer(mir, context, ty)?
        // TODO(plan-197): this synthetic emitter's `function` is
        // `mir.functions.first()`, so the lexical scope it renders under is an
        // arbitrary *foreign* function's — neither empty nor the caller's, and
        // dependent on MIR function ordering. Four of the five callers reach it
        // from inside a live emitter that discards its own correct scope.
        // Preserved verbatim here because replacing it with an explicitly empty
        // substitution changes emitted output whenever `functions[0]` is
        // generic; fixing it needs its own byte-verified change.
        .rendered_type(ty).map(RustType::into_string)
    }

    /// Converts a type ID to Rust text under an explicitly supplied
    /// type-parameter environment.
    ///
    /// This is used for class and interface storage emission, where MIR has
    /// already retained the declaring type parameter list. A type parameter
    /// outside the supplied scope renders as `SmeltUnknown`. Function bodies
    /// render under their own lexical scope (`MirFunction::type_params`).
    pub(crate) fn type_text_for_with_scoped_type_params(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<String, EmitError> {
        Self::rust_type_with_scoped_type_params(mir, context, ty, substitution)
            .map(RustType::into_string)
    }

    /// The rendered type of `ty` under `substitution`, outside any function.
    pub(crate) fn rust_type_with_scoped_type_params(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        FunctionEmitter::type_renderer(mir, context, ty)?.rust_type(ty, false, substitution)
    }

    /// Converts a type ID to a default expression with scoped type parameters.
    pub(crate) fn default_value_for_with_scoped_type_params(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustExpr, EmitError> {
        FunctionEmitter::type_renderer(mir, context, ty)?
        .default_value_with_scoped_type_params(ty, substitution)
    }

    /// Returns the Rust suffix needed when calling a throwing function.
    pub(super) fn throwing_call_suffix(&self, callee: &MirFunction) -> &'static str {
        if self.call_is_throwing(callee) { "?" } else { "" }
    }

    /// Whether a call of `callee` yields a `Result` this body propagates.
    pub(super) fn call_is_throwing(&self, callee: &MirFunction) -> bool {
        callee.can_throw && !callee.is_async && !callee.is_generator
    }

    /// Converts an operand to its Rust text representation.
    pub(super) fn operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        // A throw-payload temporary is rendered as the expression it was
        // assigned, at the point that consumes it; the staging statement and
        // declaration are suppressed elsewhere. See `emitter::throw`.
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand
            && let Some(text) = self.folded_throw_payload_expr(*local)?
        {
            return Ok(text);
        }
        match operand {
            Operand::Copy(place) => {
                if matches!(
                    self.mir.types.get(self.place_ty(place)?),
                    Some(Type::Function(_))
                ) || self.type_contains_noncloneable(self.place_ty(place)?)
                    // A reference-class field read already clones the value out of
                    // the borrowed cell, so it is owned; a second `.clone()` would
                    // be redundant.
                    || self.place_is_reference_class_field(place)
                    // An index read constructs its result rather than projecting
                    // to a place, so it is likewise already owned.
                    || self.index_place_read_is_owned(place)
                    // A scalar lowers to a `Copy` Rust type, so reading it out of
                    // a place already yields an owned value. `i.clone()` on an
                    // `f64` is what `clippy::clone_on_copy` exists to reject, and
                    // no hand-written port would spell a loop counter that way.
                    || self.place_type_is_copy_scalar(place)?
                {
                    self.place_expr(place)
                } else {
                    Ok({ let receiver = self.place_expr(place)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone())
                }
            }
            Operand::Move(place) => {
                // A value that lives in a shared capture cell is read as
                // `(*smelt_capture_x.borrow())`, a place behind a `Ref` guard.
                // Rust cannot MOVE out of that, so a move operand over such a
                // place has to clone — the same answer the `Copy` arm above
                // gives, for the same reason (the cell keeps owning the value).
                // A `Copy` scalar and a non-cloneable type are excluded exactly
                // as they are there.
                if self.place_reads_through_shared_capture(place)
                    && !self.place_type_is_copy_scalar(place)?
                    && !self.type_contains_noncloneable(self.place_ty(place)?)
                    && !matches!(
                        self.mir.types.get(self.place_ty(place)?),
                        Some(Type::Function(_))
                    )
                {
                    return Ok(self.place_expr(place)?.cloned_value());
                }
                self.place_expr(place)
            }
            Operand::Const(constant) => Ok(constant_expr(constant)),
        }
    }

    /// Whether reading `place` projects out of a shared closure-capture cell.
    ///
    /// A local captured by reference from a sibling closure is stored in an
    /// `Rc<RefCell<T>>` and every read of it renders as
    /// `(*smelt_capture_x.borrow())`, including a field or index projection off
    /// it, whose base is that same local. Such a read borrows; it does not own.
    pub(super) fn place_reads_through_shared_capture(&self, place: &Place) -> bool {
        let root = match place {
            Place::Local(local) => *local,
            Place::Field { base, .. } | Place::Index { base, .. } => *base,
            Place::Global { .. } => return false,
        };
        self.local_uses_shared_capture_storage(root) && self.is_local_declared(root)
    }

    /// Whether a place reads a scalar that lowers to a `Copy` Rust type.
    ///
    /// `bool`, the integers and `f64` are `Copy`, so a place read of one is already an
    /// owned value and needs no `.clone()`. Only these scalars are accepted: every
    /// other Smelt type either owns a heap allocation (`String`, `SmeltList`) or
    /// lowers to a type whose `Copy`-ness depends on its parameters, and a wrong
    /// answer here would emit a move out of a borrowed place.
    fn place_type_is_copy_scalar(&self, place: &Place) -> Result<bool, EmitError> {
        Ok(matches!(
            self.mir.types.get(self.place_ty(place)?),
            Some(Type::Bool | Type::Int | Type::Float | Type::FixedInt(_))
        ))
    }

    /// Render one argument for a parameter the callee takes by shared reference.
    ///
    /// When the argument already has the parameter's type the place is borrowed
    /// directly — that is the whole point of the by-reference parameter, and it
    /// is what removes the per-element deep copy. A place that is ALREADY a
    /// shared reference in the emitted Rust (a callback parameter spelled `name:
    /// &T`, see [`Self::operand_renders_as_shared_reference`]) is passed
    /// straight through: borrowing it again would hand the callee a `&&T` that
    /// only compiles because Rust deref-coerces it away, and a hand-writing Rust
    /// team would not spell that. When a coercion is needed, the coerced
    /// temporary is referenced instead; Rust extends its lifetime to the end of
    /// the statement, so `&(expr)` is valid even though the value is unnamed.
    pub(super) fn shared_reference_argument_expr(
        &self,
        arg: &Operand,
        target_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.operand_ty(arg)? != target_ty {
            return Ok(RustExpr::loose(format!("&({})", self.value_at_type(arg, target_ty)?)));
        }
        let borrowed = self.operand_borrow_expr(arg)?;
        if self.operand_renders_as_shared_reference(arg) {
            Ok(borrowed)
        } else {
            Ok(RustExpr::loose(format!("&{borrowed}")))
        }
    }

    /// Whether an operand's emitted Rust text is already a `&T`.
    ///
    /// Only a parameter binding recorded in
    /// [`FunctionEmitter::by_reference_param_locals`] qualifies: a closure whose
    /// contextual `dyn Fn` passes a parameter by shared reference spells that
    /// parameter `name: &T` while MIR keeps typing the local as the bare `T`. A
    /// projection off such a place reads through the reference and yields an
    /// owned place again, so only the bare local is answered `true`.
    pub(super) fn operand_renders_as_shared_reference(&self, operand: &Operand) -> bool {
        matches!(
            operand,
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                if self.by_reference_param_locals.contains(local)
        )
    }

    /// Converts an operand to Rust text for a read whose every use is a `&self`
    /// receiver — `.len()`, `.iter()`, `.chars()`, indexing.
    ///
    /// `operand_expr` conservatively clones a `Copy` read, because in general the
    /// caller wants an owned value it can move on. When the caller only ever
    /// *borrows* the result, that clone is a whole-collection copy for nothing, and
    /// inside a loop it changes the algorithm's complexity class rather than adding a
    /// constant: `chunk`'s `data.slice(start, start + size)` lowered to
    /// `data.clone().iter().skip(.. data.clone().len() ..)`, deep-copying every
    /// element on each of `ceil(n / size)` iterations. A hand-written Rust
    /// implementation would borrow, so the emitted code should too.
    ///
    /// Only call this when the emitted expression cannot move or mutate through the
    /// text: `&self` methods qualify, `into_*`/`push`/assignment do not. `Move`
    /// operands already elide the clone in `operand_expr`, so this only changes
    /// `Copy` reads.
    pub(super) fn operand_borrow_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        // A folded throw payload is rendered as the expression it was assigned
        // rather than as a place, so there is no clone to elide; defer to the
        // shared path so both reads agree on that text.
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand
            && self.folded_throw_payload_expr(*local)?.is_some()
        {
            return self.operand_expr(operand);
        }
        // A local held in shared closure storage reads as
        // `(*smelt_capture_x.borrow())`, and that `Ref` guard lives to the end of the
        // FULL enclosing statement. The clone is what ends the borrow early: eliding
        // it would let the guard span a nested closure call that re-borrows the same
        // cell, panicking "already borrowed" — a `RefCell` crash single-threaded JS
        // never produces. See the invariant documented on `local_value`. Keep
        // the shared-storage read on the ordinary path, whatever the caller asked
        // for; a borrow-only receiver is not worth a runtime panic.
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand
            && self.local_uses_shared_capture_storage(*local)
        {
            return self.operand_expr(operand);
        }
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_expr(place),
            Operand::Const(constant) => Ok(constant_expr(constant)),
        }
    }

    /// A `RegExp` rendered the way JavaScript stringifies one: `/source/flags`.
    ///
    /// `String(/foo/u)` and `/foo/u.toString()` are `"/foo/u"`, not the bare
    /// pattern. Every RegExp-to-string coercion routes through here so the typed
    /// and erased paths cannot drift: remeda's `isDeepEqual` compares two regexes
    /// with `data.toString() === other.toString()`, and when one side was narrowed
    /// from `unknown` while the other was cast, the two renderings disagreed.
    pub(super) fn regexp_literal_expr(receiver_text: &RustExpr) -> RustExpr {
        RustExpr::atom(format!("format!(\"/{{}}/{{}}\", {receiver_text}.source, {receiver_text}.flags)"))
    }

    /// Render conversion from a JavaScript property-key value to an owned Rust string.
    pub(super) fn property_key_to_string_expr(
        &self,
        value_text: &RustExpr,
        source_key: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // The caller's `value_text` is usually already an owned temporary (an
        // `operand_expr` render clones the local it reads), so ask for an owned
        // copy through `cloned_value` rather than appending `.clone()`
        // unconditionally: a property key is read once per element in every
        // `groupBy`/`countBy`/`keyBy` loop, and the second copy bought nothing.
        let owned_value_text = value_text.cloned_value();
        match self.mir.types.get(source_key) {
            Some(Type::String | Type::PyStr) => Ok(owned_value_text),
            Some(Type::Bool) => Ok(RustExpr::postfix_on(&value_text, format!("{value_text}.to_string()"))),
            // A property KEY is a stringified number, and JavaScript's rule is
            // the one that applies: `({ [1e21]: 1 })` has the key `"1e+21"`,
            // not twenty-two digits. `smelt_property_key` already takes it for
            // an erased key, so the two spellings agree.
            Some(Type::Int) => Ok(RustExpr::chain(format!(
                "{fn_name}({value_text} as f64)",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
            ))),
            Some(Type::Float) => Ok(RustExpr::chain(format!(
                "{fn_name}({value_text})",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
            ))),
            Some(Type::Optional(inner)) => {
                let inner_text = self.property_key_to_string_expr(&RustExpr::path("value"), *inner)?;
                Ok(RustExpr::postfix_on(&owned_value_text, format!(
                    "{owned_value_text}.map_or(String::new(), |value| {inner_text})"
                )))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(Self::regexp_literal_expr(value_text))
            }
            Some(Type::List(item_ty)) => {
                let item_text = self.property_key_to_string_expr(&RustExpr::path("value"), *item_ty)?;
                Ok(RustExpr::atom_on(&owned_value_text, format!(
                    "{owned_value_text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>().join(\",\")"
                )))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Class { .. }) => {
                // `smelt_property_key` inspects `SmeltUnknown` discriminants, so
                // its argument must be erased. A concrete union (`SmeltUnion…`) or
                // class instance is not a `SmeltUnknown`; erase it first so the
                // helper receives the runtime shape it matches over (E0308). A
                // source already spelled as `Unknown` erases to itself.
                let erased = self.erase_rendered(value_text, source_key)?;
                Ok(RustExpr::chain(format!(
                    "smelt_property_key({erased})"
                )))
            }
            _ => Ok(RustExpr::chain("\"[object Object]\".to_owned()")),
        }
    }

    /// Gets the type of an operand.
    pub(super) fn operand_ty(&self, operand: &Operand) -> Result<TypeId, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Const(Constant::None | Constant::Undefined) => Ok(self.none_ty),
            Operand::Const(Constant::Bool(_)) => self.type_id(Type::Bool),
            Operand::Const(Constant::Int(_)) => self.type_id(Type::Int),
            Operand::Const(Constant::Float(_)) => self.type_id(Type::Float),
            Operand::Const(Constant::BigInt(_)) => self.type_id(Type::BigInt),
            Operand::Const(Constant::String(_)) => self.type_id(self.text_type()),
            Operand::Const(Constant::Symbol(_)) => self.type_id(Type::Unknown),
        }
    }
}
