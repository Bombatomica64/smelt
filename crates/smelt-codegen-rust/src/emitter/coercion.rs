//! Coercion seam: the one module where a value crosses between its static Rust
//! type and the erased `SmeltUnknown` form. The public surface is a small set of
//! intent-named verbs; the per-type mechanics stay private below them:
//!
//! - `value_at_type(op, target)` / `rendered_at_type(text, src, target)` —
//!   coerce a value to a concrete target type (both endpoints are known and
//!   interned). The general entry; it dispatches to erase/extract internally.
//! - `erase(op)` / `erase_rendered(text, src)` — box a typed value into
//!   `SmeltUnknown`. This direction is *target-free*: boxing needs no MIR type.
//! - `extract(op, target)` / `extract_rendered(text, target)` — pull a typed
//!   value back out of `SmeltUnknown`.
//! - `tag_check(op, kind)` — runtime narrowing (`is this tag a String?`), which
//!   is a guard, not value coercion.
//!
//! See CONTEXT.md (## Coercion).

mod at_type;
mod erase;
mod extract;
mod function_adapter;
mod prop_key;
pub(crate) mod record;
mod record_adapter;
mod shapes;
mod tag;

use super::*;
use crate::rust::RustExpr;

use crate::erased_list_alias;
use crate::rust::RustShape;
pub(in crate::emitter) use extract::erased_to_bounded_list_expr;
use extract::{SHARED_ITEM, SharedItemConversion, erased_to_list_expr};
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Whether two MIR types render to the SAME Rust type.
    ///
    /// Type identity in MIR is finer-grained than Rust representation: several
    /// distinct `TypeId`s legitimately share one Rust spelling (an erased
    /// `Type::TypeParam` and `Type::Unknown` are both `SmeltUnknown`, a union
    /// with no concrete Rust form likewise). A coercion keyed on `TypeId`
    /// inequality therefore fires for pairs where there is nothing to convert,
    /// and "nothing to convert" is not free: a container coercion rebuilds the
    /// backing buffer, which severs the JavaScript reference identity the source
    /// value carries. Asking about the RENDERING is the question these seams
    /// actually mean.
    pub(super) fn same_rust_repr(&self, left: TypeId, right: TypeId) -> Result<bool, EmitError> {
        if left == right {
            return Ok(true);
        }
        Ok(self.signature_type(left)? == self.signature_type(right)?)
    }

    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    ///
    /// Renders at the emitter's ambient scope — the type parameters the Rust
    /// item being emitted declares. A position whose target is a *callee-owned*
    /// type must not use that scope; it calls [`Self::value_at_type_in`] with a
    /// scope narrowed to what the callee's emitted signature also spells.
    pub(super) fn value_at_type(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.value_at_type_in(operand, target, &self.render_scope())
    }

    /// Converts an operand to Rust text at an explicitly given render scope.
    ///
    /// The scope-taking form of [`Self::value_at_type`]. Every arm below — and
    /// every value-rendering verb they delegate to — asks this one scope rather
    /// than re-deriving the erasure rule at its own depth, which is what makes a
    /// render position's answer about a `Type::TypeParam` single-valued (H42).
    ///
    /// The arms here are the ones that need the operand itself, not only its
    /// rendering: its place, its constant, its closure, or a spelling an operand
    /// has that an inlined fragment cannot. Everything else falls through to
    /// [`Self::rendered_at_type`], the one ladder both entries share.
    pub(super) fn value_at_type_in(
        &self,
        operand: &Operand,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        if let Some(upcast) = self.base_initializer_receiver_upcast_expr(operand, target, scope)? {
            return Ok(upcast);
        }
        let source_ty = self.operand_ty(operand)?;
        // A function at an object type: its `name` and `length` read off the
        // operand itself, where the static name is known.
        if let Some(record) = self.function_record_expr(Some(operand), source_ty, target)? {
            return Ok(record);
        }
        let operand_expr = self.operand_expr(operand)?;
        if let Some(key) = self.prop_key_conversion(&operand_expr, source_ty, target)? {
            return Ok(key);
        }
        if let Some(converted) = self.py_int_conversion(&operand_expr, source_ty, target) {
            return Ok(converted);
        }
        if let Some(key) =
            self.key_union_at_string(&operand_expr.cloned_value(), source_ty, target)?
        {
            return Ok(key);
        }
        // A function operand renders as a borrow of its binding, and a union arm
        // owns its value: the binding may be read again after the injection.
        let owned_text = if matches!(self.mir.types.get(source_ty), Some(Type::Function(_)))
            && matches!(operand, Operand::Copy(Place::Local(_)))
        {
            operand_expr.cloned_value()
        } else {
            operand_expr.clone()
        };
        if let Some(injected) = self.inject_union_value(&owned_text, source_ty, target, scope)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value(&operand_expr, source_ty, target)? {
            return Ok(projected);
        }
        // See `rendered_at_type`: a concrete union of callables at the
        // variadic erased signature crosses through its own adapter.
        if self.concrete_union_members(source_ty).is_some()
            && let Some(Type::Function(function)) = self.mir.types.get(target)
            && self.is_erased_unknown_rest_function(function)
        {
            let erased = self.erase_rendered(&operand_expr, source_ty)?;
            return self.extract_rendered(&erased, target, scope);
        }
        // A tuple at a DIFFERENT tuple type (`[n, accept] as [number, Accept]`
        // returned as `[number, Accept | undefined]`) is coerced element by
        // element; the operand ladder below has no tuple rule and handed the
        // value back unchanged (E0308).
        if source_ty != target
            && let (Some(Type::Tuple(source_items)), Some(Type::Tuple(target_items))) =
                (self.mir.types.get(source_ty), self.mir.types.get(target))
            && source_items.len() == target_items.len()
        {
            return self.rendered_at_type(&operand_expr, source_ty, target, scope);
        }
        // A tuple at a LIST type (`[] as []` or `[a, b] as [A, B]` passed to a
        // `T[]` parameter, which TypeScript accepts) collects its elements into
        // a list. The text entry point owns that rule; the operand ladder below
        // has none and passed the tuple through unchanged (`()` where a
        // `SmeltList` was expected, E0308).
        if matches!(
            (self.mir.types.get(source_ty), self.mir.types.get(target)),
            (Some(Type::Tuple(_)), Some(Type::List(_)))
        ) {
            return self.rendered_at_type(&operand_expr, source_ty, target, scope);
        }
        let target_is_erased = matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(target).is_none()
            || self.is_erased_class_type(target);
        if target_is_erased
            && let Operand::Copy(Place::Field { base, field })
            | Operand::Move(Place::Field { base, field }) = operand
            && matches!(
                self.mir.types.get(self.local_decl(*base)?.ty),
                Some(Type::String | Type::PyStr)
            )
        {
            let (field_text, field_source_ty) =
                self.string_field_read(&self.local_value(*base)?, *field)?;
            return self.erase_rendered(&RustExpr::opaque(field_text), field_source_ty);
        }
        if source_ty == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.operand_expr(operand);
        }
        // A concrete host value used at a RECORD or COLLECTION type goes
        // through its erasure adapter first. Same rule as in
        // `rendered_at_type`, which this delegates to so the two spellings
        // of the coercion cannot disagree; the operand entry point is the one
        // an ordinary assignment reaches.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(source_ty)
            && matches!(
                self.mir.types.get(target),
                Some(Type::Dict(_, _) | Type::List(_) | Type::Set(_))
            )
            && self
                .stdlib_class_of_symbol(*name)?
                .is_some_and(smelt_stdlib::StdlibClass::erases_through_adapter)
        {
            return self.rendered_at_type(&operand_expr, source_ty, target, scope);
        }
        // `Future<A>` -> `Future<B>` is a coercion of the AWAITED value, so it
        // needs the awaiting adapter the text-based entry point already builds
        // (`smelt_source_future.await?`, then the item coercion). Falling through
        // to the unchanged operand instead emitted a future of the wrong item
        // type at its declared type -- an E0308 that only appears once the
        // program actually keeps the future, which is why a `return` of a
        // promise-returning call whose declared item is erased did not type-check
        // (radash's `_.try(async () => _.parallel(..))`).
        if matches!(self.mir.types.get(source_ty), Some(Type::Future(_)))
            && matches!(self.mir.types.get(target), Some(Type::Future(_)))
        {
            return self.rendered_at_type(&operand_expr, source_ty, target, scope);
        }
        if let Some(Type::TypeParam { name }) = self.mir.types.get(target)
            && scope.spells(*name)
        {
            return self.extract(operand, target, scope);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(target).is_none()
        {
            // A JS element read flowing into an erased slot keeps its own
            // fallibility, exactly as the `Option<..>` target above does: an
            // out-of-range read is `undefined`, so it must erase to
            // `SmeltUnknown::Undefined` rather than be made total first and
            // erase as the element type's missing value (`''`, `0`, `[]`).
            if let Some(read) = self.erased_element_read_expr(operand)? {
                return Ok(read);
            }
            return self.erase(operand);
        }
        if self.is_erased_class_type(target) {
            if let Some(read) = self.erased_element_read_expr(operand)? {
                return Ok(read);
            }
            return self.erase(operand);
        }
        // JavaScript keeps `null` and `undefined` distinct under `===`
        // (`undefined === null` is false). An `Option<SmeltUnknown>` slot has
        // room for both: `None` is the absent/`undefined` case and a present
        // `SmeltUnknown::Null` payload is an explicit `null` (see
        // `optional_inner_preserves_erased_singletons`). Storing JS `null` as
        // `Some(SmeltUnknown::Null)` is what lets the strict comparison in
        // `optional_erased_singleton_equality_expr` answer `x === null`
        // correctly; folding it into `None` made `x !== null` true for a slot
        // that holds `null`. Slots whose payload cannot carry the tag (a
        // concrete inner or a tagged union with no nullish arm) keep the
        // collapsed `None` encoding.
        // `null` in a slot that admits both absences (`Option<Option<T>>`,
        // `undefined` outside) is the present-but-null `Some(None)`.
        if matches!(operand, Operand::Const(Constant::None))
            && let (base, true, true) =
                smelt_hir::type_normalize::peel_optional(&self.mir.types, target)
            && base != self.none_ty
            && let Some(&Type::Optional(null_layer, _)) = self.mir.types.get(target)
        {
            return Ok(RustExpr::chain(format!(
                "Some({})",
                self.default_value(null_layer)?.delimited()
            )));
        }
        // `null` in a `T | null` slot is its `None`, whatever `T` is: a generic
        // body compiled once for every `T` tests `is_none()`. An erased
        // payload may still carry a `null` tag of its own (a value converted
        // in from `unknown`), so comparisons on such a slot accept both
        // (`absence_equality_expr`).
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(_, smelt_hir::Absence::Null)) = self.mir.types.get(target)
        {
            return self.default_value(target);
        }
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(inner, _)) = self.mir.types.get(target)
            && self.optional_inner_preserves_erased_singletons(*inner)
        {
            return Ok(RustExpr::chain(format!(
                "Some({})",
                self.rendered_at_type(
                    &self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Null")),
                    self.type_id(Type::Unknown)?,
                    *inner,
                    scope
                )?
                .delimited()
            )));
        }
        // The same distinction in a `null | undefined` slot (`Option<()>`, see
        // the frontend's `null_or_undefined_join`): `null` is the present unit.
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(inner, _)) = self.mir.types.get(target)
            && *inner == self.none_ty
        {
            return Ok(RustExpr::chain("Some(())"));
        }
        // `undefined` in a JavaScript `number` slot (a tuple or list literal's
        // element, an argument) is the number hole, the same value a missing
        // element of a `number[]` reads as (`element_missing_value`), so the
        // two compare equal and print as `undefined`.
        if matches!(operand, Operand::Const(Constant::Undefined))
            && self.indexes_utf16()
            && matches!(self.mir.types.get(target), Some(Type::Float))
        {
            return Ok(RustExpr::chain("smelt_number_hole()"));
        }
        if matches!(
            operand,
            Operand::Const(Constant::None | Constant::Undefined)
        ) {
            return self.default_value(target);
        }
        if let Some(slot) = self.callable_object_call_slot_expr(
            &self.operand_expr(operand)?,
            source_ty,
            target,
            scope,
        )? {
            return Ok(slot);
        }
        self.value_at_optional(operand, target, scope, source_ty, &operand_expr)
    }

    /// Marks a `source` → `target` structural coercion as being expanded.
    ///
    /// Returns `None` when the pair is already on
    /// [`FunctionEmitter::type_expansion_stack`], i.e. when the caller is about
    /// to re-enter an expansion it is already inside. Callers treat that as "no
    /// structural adapter available" and fall back to their remaining rules,
    /// which is what keeps mutually recursive record/callable shapes from
    /// exhausting the stack. The returned guard pops the pair on drop, so an
    /// early `?` return cannot leave the stack unbalanced.
    pub(super) fn enter_type_expansion(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Option<TypeExpansionGuard<'_>> {
        if self
            .conversions
            .type_expansion_stack
            .borrow()
            .contains(&(source, target))
        {
            return None;
        }
        Some(helper_reservation::push_entry(
            &self.conversions.type_expansion_stack,
            (source, target),
        ))
    }

    /// Coerces already-rendered Rust value text from a known source type to a destination type.
    ///
    /// `scope` is the render position's type-parameter environment
    /// ([`RenderScope`]). It decides, once for the whole render, whether a
    /// `Type::TypeParam` reached anywhere below is spellable as a Rust
    /// identifier or erases to `SmeltUnknown`, and every side of the position —
    /// the coerced entries, the recovered entries
    /// ([`Self::extract_rendered`]), and the container annotation
    /// ([`Self::collected_container_type`]) — asks that same scope. An
    /// ordinary emit site passes `&self.render_scope()`, the lexical scope of
    /// the Rust item being emitted; a site inside the recursion forwards the
    /// scope it was given rather than re-deriving one at its own depth. See
    /// `render_scope` for why that was the H42 defect.
    pub(super) fn rendered_at_type(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        if let Some(key) = self.prop_key_conversion(value_text, source, target)? {
            return Ok(key);
        }
        if let Some(converted) = self.py_int_conversion(value_text, source, target) {
            return Ok(converted);
        }
        if let Some(key) = self.key_union_at_string(value_text, source, target)? {
            return Ok(key);
        }
        if let Some(injected) = self.inject_union_value(value_text, source, target, scope)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value(value_text, source, target)? {
            return Ok(projected);
        }
        // A concrete union of callables called through the variadic erased
        // signature (its arms take different arguments): the value crosses
        // into the erased carrier through the union's own adapter, then is
        // read back as the erased callable.
        if self.concrete_union_members(source).is_some()
            && let Some(Type::Function(function)) = self.mir.types.get(target)
            && self.is_erased_unknown_rest_function(function)
        {
            let erased = self.erase_rendered(value_text, source)?;
            return self.extract_rendered(&erased, target, scope);
        }
        if let Some(wrapped) =
            self.callable_interface_from_function_expr(value_text, source, target, scope)?
        {
            return Ok(wrapped);
        }
        // `value_text` is usually already an owned temporary (an operand render clones
        // the local it reads), so take an owned copy rather than deep-copying it a
        // second time.
        let smelt_owned_value = value_text.cloned_value();
        if source == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(value_text.to_owned());
        }
        // A value of a type parameter that renders as a real Rust generic (a
        // lifted `T`) into optional layers over that same `T`: present in
        // every layer. It is a `T`, not the erased carrier, so it has no tag
        // to inspect.
        if matches!(self.mir.types.get(source), Some(Type::TypeParam { .. }))
            && matches!(self.mir.types.get(target), Some(Type::Optional(..)))
            && !self.renders_erased(source).unwrap_or(true)
            && let (base, undefined, null) =
                smelt_hir::type_normalize::peel_optional(&self.mir.types, target)
            && base == source
        {
            let wrapped = if undefined && null {
                format!("Some(Some({}))", smelt_owned_value.delimited())
            } else {
                format!("Some({})", smelt_owned_value.delimited())
            };
            return Ok(RustExpr::chain(wrapped));
        }
        // Text crossing between the two languages of a mixed crate: a
        // JavaScript string is UTF-16 (`JsString`), a Python `str` is UTF-8
        // (`String`). A frontend marks each crossing with a `TypeAssert` to the
        // receiving language's view of the type; containers reach here per
        // element through their own arms.
        match (self.mir.types.get(source), self.mir.types.get(target)) {
            (Some(Type::String), Some(Type::PyStr)) => {
                return Ok(smelt_owned_value.postfix(".to_string_lossy()"));
            }
            (Some(Type::PyStr), Some(Type::String)) => {
                return Ok(RustExpr::atom(format!(
                    "JsString::from({smelt_owned_value})",
                    smelt_owned_value = smelt_owned_value.delimited()
                )));
            }
            _ => {}
        }
        // A Python exception (or its `args`) at a `str` slot is its `str`: the
        // pytest `raises` lowering keeps the caught message as a string.
        if (self.is_py_exception_type(source) || self.is_py_exception_args_type(source))
            && matches!(self.mir.types.get(target), Some(Type::PyStr))
        {
            return Ok(value_text.postfix(".to_string()"));
        }
        // A value asked for at `bool` is a TRUTHINESS test, not a cast: that is
        // what JavaScript does wherever a boolean is expected, and it is the
        // only reading of a class instance, a list, a string or a number in a
        // boolean position. Without this the coercion fell through to the
        // structural arms, which had no rule and handed the value back
        // unchanged — `Option<Node<T>>` at `bool` emitted
        // `.map_or(false, |value| value)`, and the generated crate did not
        // compile (E0308, 48 sites in Hono's trie router alone). `Type::Unknown`
        // keeps its own erased narrowing path below, which already answers
        // truthiness for a tagged value.
        if matches!(self.mir.types.get(target), Some(Type::Bool))
            && !matches!(
                self.mir.types.get(source),
                Some(Type::Bool | Type::Unknown | Type::TypeParam { .. })
            )
        {
            return self.value_truthy_expr(value_text, source);
        }
        if let Some(view) = self.match_array_view_expr(value_text, source, target, scope)? {
            return Ok(view);
        }
        // A concrete host value cast to a RECORD or a COLLECTION goes through
        // its erasure adapter first.
        //
        // `erased as { type: string }` reads fields off the host record, and the
        // adapters below know how to walk a `SmeltUnknown` into a typed record —
        // but they are reached only when the source is already erased. A
        // concrete `SmeltBlob` therefore fell straight through and was assigned
        // to a `SmeltRecord` (E0308).
        //
        // `instanceof` narrows an erased local: inside
        // `x instanceof Blob ? (x as { type: string }) : ..` the operand is the
        // concrete class rather than `unknown`, so this cast has a concrete source.
        //
        // Stated over `erases_through_adapter`, so it holds for every host class
        // with an adapter rather than for the one that exposed it.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(source)
            && matches!(
                self.mir.types.get(target),
                Some(Type::Dict(_, _) | Type::List(_) | Type::Set(_))
            )
            && self
                .stdlib_class_of_symbol(*name)?
                .is_some_and(smelt_stdlib::StdlibClass::erases_through_adapter)
        {
            let erased = RustExpr::atom(self.carrier_format(
                self.carrier()?,
                format_args!(
                    "({smelt_owned_value}).into_smelt_unknown()",
                    smelt_owned_value = smelt_owned_value.delimited()
                ),
            ));
            return self.rendered_at_type(&erased, self.type_id(Type::Unknown)?, target, scope);
        }
        // A generated record (a struct of declared fields) at a DICT target is
        // the dict of its own enumerable properties: TypeScript assigns an
        // object type to `Record<K, V>` structurally, and JavaScript reads it
        // through the same keys. It goes through the record's erasure (the
        // object of its fields) and the dict extraction from an erased object,
        // the two conversions an erased value of this shape already takes.
        // Without it a struct passed to a `Record<K, V>` parameter
        // (`invert({ a: "x", length: 2 })` under its own record type) did not
        // compile (E0308).
        if matches!(self.mir.types.get(target), Some(Type::Dict(_, _)))
            && self
                .structural_record_fields(source)
                .is_some_and(|fields| !fields.is_empty())
        {
            let erased = self.erase_rendered(value_text, source)?;
            return self.rendered_at_type(&erased, self.type_id(Type::Unknown)?, target, scope);
        }
        if source == target && matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            if value_text.is_borrowed_callback() {
                return self.borrowed_function_handle_expr(value_text, target);
            }
            return Ok(smelt_owned_value);
        }
        self.rendered_at_same_container(value_text, source, target, scope, smelt_owned_value)
    }
}

/// Scope guard that pops a type pair off
/// [`FunctionEmitter::type_expansion_stack`] when the expansion finishes.
///
/// Held by value at the expansion site (see
/// [`FunctionEmitter::enter_type_expansion`]) so the stack stays balanced even
/// when the expansion returns an `EmitError` through `?`.
type TypeExpansionGuard<'emit> = helper_reservation::StackEntryGuard<'emit, (TypeId, TypeId)>;

/// The two directional verbs of the seam. Adapters below re-enter here for
/// every nested crossing (a callback's arguments, a record's fields), so the
/// recursion over a type's structure runs through the seam, not sideways
/// between adapters.
impl FunctionEmitter<'_> {
    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    ///
    /// The operand-shaped twin of [`Self::erase_value`]; the two must agree
    /// about every `Type`, because the same MIR value reaches one or the other
    /// depending only on whether the caller already had it rendered.
    pub(in crate::emitter) fn erase(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        if matches!(operand, Operand::Const(Constant::Undefined)) {
            return Ok(self.carrier_expr(self.carrier()?, &RustExpr::undefined()));
        }
        let text = self.operand_expr(operand)?;
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = text.cloned_value();
        let ty = self.operand_ty(operand)?;
        // A lifted `T` VALUE erases through the `IntoSmeltUnknown` bound its
        // signature declares (`classes::GENERATED_TYPE_PARAM_BOUNDS`): the
        // bound's own method, which every instantiation supplies, so the generic
        // trial does not count it against the body. Only a whole operand of type
        // `T` qualifies: a container of `T`s erases by rebuilding its storage,
        // which loses JavaScript aliasing the erased signature keeps, and still
        // counts.
        let _bound_conversion = matches!(
            self.mir.types.get(ty),
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name)
        )
        .then(|| self.context.carrier_boundary());
        match self.mir.types.get(ty) {
            Some(Type::TypeParam { name }) if !self.current_function_has_type_param(*name) => {
                Ok(text)
            }
            // A `void` value erases to the absent tag, but the expression that
            // produced it still has to RUN: `text` is routinely a call (`let r:
            // unknown = setter(k, v)`), and answering the bare constant deleted
            // the call and its side effects from the program.
            Some(Type::None) if text.is_unit() => {
                Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Null")))
            }
            Some(Type::None) => Ok(RustExpr::atom(self.carrier_format(
                self.carrier()?,
                format_args!(
                    "{{ let _ = {text}; SmeltUnknown::Null }}",
                    text = text.delimited()
                ),
            ))),
            Some(Type::List(item))
                if !self.list_items_render_as_unknown(ty, &self.render_scope())
                    && self.list_erases_live(*item) =>
            {
                // Genuine dynamic boundary: the live view IS the erased array.
                let method = self.list_live_erasure(*item)?;
                Ok(RustExpr::atom(self.carrier_format(
                    self.carrier()?,
                    format_args!("SmeltList::clone(&{text}){method}", text = text.delimited()),
                )))
            }
            Some(Type::List(item))
                if !self.list_items_render_as_unknown(ty, &self.render_scope()) =>
            {
                // A `List<None>` erases its elements to `Null`, but an
                // `[undefined, …]` literal must erase to `Undefined` (the type
                // lost the distinction; only the defining constants carry it).
                // Recover it from the def-site so genuine `null` arrays keep the
                // `Null` path. This mirrors the same recovery in `value_at_type`
                // for the `List<None> -> List<Unknown>` coercion shape.
                let value_wrap = if matches!(self.mir.types.get(*item), Some(Type::None))
                    && self.container_local_all_undefined_constants(operand)?
                {
                    self.carrier_expr(self.carrier()?, &RustExpr::undefined())
                } else {
                    self.erase_rendered(&RustExpr::path("smelt_value"), *item)?
                };
                // The typed list carries its own JS reference id (`SmeltList`),
                // so erasing to `SmeltUnknown::Array` reuses it directly — this is
                // what preserves `===`/`.toBe` identity across an erase/extract
                // round-trip (e.g. `tap`/`forEach` returning their input).
                //
                // The elements are taken through `Into<Vec<_>>` rather than
                // `into_iter()` because `{text}` is an owned list at most sites but a
                // `&SmeltList<_>` wherever the erased value is a by-reference callback
                // parameter (see `callback_param_is_shared_reference`), and
                // `(&SmeltList<T>).into_iter()` yields `&T` — which `{value_wrap}`
                // cannot consume (a primitive `value as f64` is not fixable by any
                // trait impl). `From<SmeltList<T>> for Vec<T>` MOVES the backing
                // storage and `From<&SmeltList<T>> for Vec<T>` clones it, so Rust's own
                // impl selection does the narrowing: the owned path stays copy-free and
                // only the borrowed path pays a clone — which erasing to an owned
                // `SmeltArray` requires anyway.
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_l = {text}; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| {value_wrap}).collect::<Vec<_>>())) }}", text = text.delimited(), value_wrap = value_wrap.delimited()
                ))))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key).is_some_and(Type::is_text)
                    && self.mir.types.get(*item) != Some(&Type::Unknown) =>
            {
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_record = {smelt_owned_text}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(smelt_key, smelt_value)| (smelt_key, {value_wrap})).collect())) }}", smelt_owned_text = smelt_owned_text.delimited(), value_wrap = value_wrap.delimited()
                ))))
            }
            // A property-key record is an object: an `unknown`-valued one
            // erases to a second handle on its own store (one JavaScript
            // object, as the string-keyed record does above); any other is
            // copied with its id, each value erased.
            Some(Type::Dict(key, item)) if self.is_prop_key_record_key(*key) => {
                if self.mir.types.get(*item) == Some(&Type::Unknown) {
                    return Ok(text.postfix(".share_unknown()"));
                }
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(text.postfix(format!(
                    ".to_unknown_with(|smelt_value| {value_wrap})",
                    value_wrap = value_wrap.delimited()
                )))
            }
            Some(Type::Dict(key, item))
                if self
                    .mir
                    .types
                    .get(*key)
                    .is_none_or(|candidate| !candidate.is_text()) =>
            {
                let key_wrap =
                    self.property_key_to_string_expr(&RustExpr::path("smelt_key"), *key)?;
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "SmeltUnknown::Object(SmeltObject::new({text}.into_iter().map(|(smelt_key, smelt_value)| ({key_wrap}, {value_wrap})).collect()))", text = text.tight(), key_wrap = key_wrap.delimited(), value_wrap = value_wrap.delimited()
                ))))
            }
            Some(Type::Set(item)) => {
                // Every Set erases through its `IntoSmeltUnknown` adapter, which
                // stamps the `__smelt_set` marker and keeps the set's object id, so
                // `instanceof Set`, `toString`, inspection and snapshots recognize
                // the erased value and `SmeltFromUnknown` round-trips it. The
                // adapter is the live one when the members can be rebuilt
                // (`Self::container_erasure_method`).
                let method = self
                    .container_erasure_with(&self.container_erasure_method(&[*item])?, &[*item])?;
                Ok(smelt_owned_text
                    .postfix(self.carrier_format(self.carrier()?, format_args!("{method}"))))
            }
            Some(Type::Optional(inner, _)) => {
                let value_wrap = self.erase_rendered(&RustExpr::path("smelt_value"), *inner)?;
                let absent = self.optional_absent_value(ty, *inner);
                Ok(smelt_owned_text.postfix(self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        ".map_or({absent}, |smelt_value| {value_wrap})",
                        value_wrap = value_wrap.delimited()
                    ),
                )))
            }
            Some(Type::Function(_)) => {
                let erased = self.erase_function_operand(operand)?;
                // A typed inline `function` expression is marked as a
                // constructor where it is erased (see `closure_value_expr`).
                if self.operand_is_inline_function_expression(operand)? {
                    return Ok(RustExpr::atom(format!(
                        "{{ let smelt_value = {erased}; if let SmeltUnknown::Function(smelt_function) = &smelt_value {{ smelt_mark_constructor(smelt_function); }} smelt_value }}",
                        erased = erased.delimited()
                    )));
                }
                Ok(erased)
            }
            // A generated union erases through `into_smelt_unknown()`, which
            // takes `self` BY VALUE. Every other consuming arm of this match
            // hands it an owned copy (`smelt_owned_text`); this one passed the
            // raw operand text, so erasing a place read moved out of it and the
            // next read of the same local was a use-after-move (E0382). The
            // rendered-value twin `erase_concrete_union_expr` has always owned
            // its source for exactly this reason — erasing a value is an
            // INSPECTION and must not consume it.
            Some(Type::Union(_))
                if self
                    .concrete_union_members(self.operand_ty(operand)?)
                    .is_some() =>
            {
                Ok(smelt_owned_text.postfix(
                    self.carrier_format(self.carrier()?, format_args!(".into_smelt_unknown()")),
                ))
            }
            // A `SmeltFuture` is a shared handle, so erasing it takes another
            // handle on the same promise rather than moving the local: the
            // erased view shares its settled result and keeps one identity
            // per source promise (`SmeltFuture::smelt_promise_with`).
            Some(Type::Future(item)) => self.promise_future_unknown_expr(&smelt_owned_text, *item),
            // Genuine dynamic boundary: a generator crossing into source
            // `unknown` is a live resumable state machine that no concrete
            // type, generated union, or scoped generic can represent on the
            // erased side. The runtime prelude's `IntoSmeltUnknown` adapter
            // reproduces the JavaScript iterator protocol (`next` →
            // `{ value, done }` steps) over the same shared state machine.
            Some(Type::Generator { .. }) => Ok(smelt_owned_text.postfix(
                self.carrier_format(self.carrier()?, format_args!(".into_smelt_unknown()")),
            )),
            Some(Type::Never) | None => {
                Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Null")))
            }
            // The rest has nothing to read from the operand itself.
            Some(_) => self.erase_value(&RenderedValue::new(text, ty)),
        }
    }

    /// Wrap a rendered value expression with a known static type into `SmeltUnknown`.
    ///
    /// Transitional wrapper: callers that still thread bare `(text, type)` reach
    /// the [`RenderedValue`]-based [`FunctionEmitter::erase_value`] through here
    /// by treating the text as a primary/postfix [`Precedence::Atom`]. New
    /// callers should build a [`RenderedValue`] with the right precedence and
    /// call [`FunctionEmitter::erase_value`] directly so erase can parenthesize
    /// loose operands itself.
    pub(in crate::emitter) fn erase_rendered(
        &self,
        value_text: &RustExpr,
        ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.erase_value(&RenderedValue::new(value_text.clone(), ty))
    }

    /// Emits extraction from `SmeltUnknown` into a concrete Rust type.
    ///
    /// JavaScript and Python code often narrows dynamic values through guards
    /// the frontend cannot fully preserve after generic or regex surfaces erase
    /// the shape. Keep primitive extraction total where the source language has
    /// a defined coercion/default instead of turning those paths into generated
    /// Rust panics.
    pub(in crate::emitter) fn extract(
        &self,
        value: &Operand,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // The primitive-target arms of `extract_rendered` (`Bool`/`Int`/
        // `Float`/`String`) match directly on a `SmeltUnknown` scrutinee. A
        // caller that reaches here with an already-concrete `Option` source whose
        // inner type is itself concrete — e.g. an `Option<String>` array element
        // coerced to a `String` method receiver — must instead go through the
        // general concrete coercion, which unwraps the `Option`. (List/Dict and
        // other targets normalize their source through `into_smelt_unknown`
        // first, so they still accept a non-erased source.) `value_at_type`
        // handles a non-erased `Option` source directly without delegating back
        // to `extract`, so this cannot recurse.
        // A whole `T` recovered from the carrier converts through the
        // `SmeltFromUnknown` bound every lifted parameter declares, so the
        // generic trial does not count it; see the `IntoSmeltUnknown` twin in
        // `erase`. A container of `T`s is rebuilt and still counts.
        let _bound_conversion = matches!(
            self.mir.types.get(target),
            Some(Type::TypeParam { name }) if scope.spells(*name)
        )
        .then(|| self.context.carrier_boundary());
        let source_ty = self.operand_ty(value)?;
        let target_is_primitive = matches!(
            self.mir.types.get(target),
            Some(Type::Bool | Type::Int | Type::PyInt | Type::Float | Type::String | Type::PyStr)
        );
        let source_inner_is_erased = matches!(
            self.mir.types.get(source_ty),
            Some(Type::Optional(inner, _)) if self.has_dynamic_shape(*inner)
        );
        // The same holds for a FUNCTION target that is the optional's own inner
        // type: a narrowed `Option<Rc<dyn Fn ..>>` unwraps to its function. The
        // erased path below matched it as a `SmeltUnknown`, which an
        // `Option<Rc<dyn Fn>>` is not (E0308), so it never compiled. Other
        // inner types keep the erased extraction and its handling of an absent
        // value (e.g. `Array.from(null)` reaching an iterable extraction).
        let target_is_source_inner =
            matches!(
                self.mir.types.get(source_ty),
                Some(Type::Optional(inner, _)) if *inner == target
            ) && matches!(self.mir.types.get(target), Some(Type::Function(_)));
        if (target_is_primitive || target_is_source_inner)
            && matches!(self.mir.types.get(source_ty), Some(Type::Optional(..)))
            && !source_inner_is_erased
        {
            return self.value_at_type(value, target);
        }
        // A bare function value (`Rc<dyn Fn ...>`) crossing into a record/list
        // extraction cannot be fed straight to the `into_smelt_unknown()` arms:
        // the erased-function Rust type does not implement `IntoSmeltUnknown`.
        // Wrap it into a `SmeltUnknown::Function` at the boundary first, then
        // extract from that erased value.
        if matches!(self.mir.types.get(source_ty), Some(Type::Function(_))) {
            let erased = self.erase(value)?;
            return self.extract_rendered(&erased, target, scope);
        }
        // A RECORD source needs the same normalization for the same reason. The
        // fetch-runtime arm of `extract_rendered` hands its text straight to
        // `SmeltFromUnknown::smelt_from_unknown`, which takes a `SmeltUnknown`,
        // so an object literal flowing into a `Response`/`Headers` position
        // (`{}` at a `Response`-returning slot) reached it as a `SmeltRecord`
        // and produced an E0308 naming neither the source line nor the reason.
        // Erasing here keeps `extract_rendered`'s contract — its text IS an
        // erased value — instead of teaching one of its arms to re-derive the
        // source type it was never given.
        if matches!(self.mir.types.get(source_ty), Some(Type::Dict(_, _)))
            && matches!(self.mir.types.get(target), Some(Type::Class { .. }))
            && self.is_fetch_runtime_class_type(target)?
        {
            let erased = self.erase(value)?;
            return self.extract_rendered(&erased, target, scope);
        }
        let text = self.operand_expr(value)?;
        self.extract_rendered(&text, target, scope)
    }

    /// Emits checked extraction from an already-rendered `SmeltUnknown` value.
    ///
    /// `scope` is the render position's type-parameter environment, threaded in
    /// by [`Self::rendered_at_type`] (or built by an ordinary emit site with
    /// `self.render_scope()`). The `Type::TypeParam` arm below recovers the
    /// value as the Rust generic exactly when `scope` spells that name, which is
    /// the same query the coercion path and the container annotation ask — so
    /// the recovered entries cannot carry a `T` the annotation erased, or vice
    /// versa.
    pub(in crate::emitter) fn extract_rendered(
        &self,
        text: &RustExpr,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = text.cloned_value();
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text.to_owned()),
            // An erased `Date` keeps the marker record; the concrete `SmeltDate`
            // recovers through its own adapter (the runtime-class arm below).
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Date) && self.is_erased_class_type(target) =>
            {
                Ok(text.to_owned())
            }
            // Extracting an erased value into the unit type means the source
            // language discards it (a `=> void` callback ignores whatever it
            // returns; JS does not assert the value is null), so it is dropped, not
            // checked: `tap(identity)` and a `vi.fn<(x) => void>` transformer return
            // values here.
            Some(Type::None) => Ok(RustExpr::atom(format!("{{ let _ = {smelt_owned_text}; () }}", smelt_owned_text = smelt_owned_text.delimited()))),
            // A bigint is extracted the way its scalar siblings below are: an
            // exhaustive match over the tags, keeping the `BigInt` tag's exact
            // value and coercing the primitives a dynamic value can hold
            // exactly as `SmeltFromUnknown for BigInt` does.
            // A symbol and a property key extract through their runtime
            // `SmeltFromUnknown` adapters: the symbol tag's spelling moves into
            // the typed value, and a key is `ToPropertyKey` of whatever arrived.
            Some(Type::Symbol) => Ok(RustExpr::chain(self.carrier_format(self.carrier()?, format_args!(
                "<SmeltSymbol as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})", smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::PropKey) => Ok(RustExpr::chain(self.carrier_format(self.carrier()?, format_args!(
                "SmeltToPropKey::smelt_to_prop_key(&({smelt_owned_text}))", smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::BigInt) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::BigInt(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_bigint_from_number(smelt_value).unwrap_or_default(), SmeltUnknown::String(smelt_value) => smelt_bigint_from_string(&smelt_value).unwrap_or_default(), SmeltUnknown::Bool(smelt_value) => {big}::from(i64::from(smelt_value)), SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => {big}::default() }}",
                big = crate::bigint_prelude::BIGINT_TYPE, smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::Bool) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_value != 0.0 && !smelt_value.is_nan(), SmeltUnknown::String(smelt_value) => !smelt_value.is_empty(), {smelt_bigint_arm}SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value),"), smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::Float) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }}, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),"), smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::Int) => self.extract_int(&smelt_owned_text),
            // A Python `int` read back out of the carrier (a `json.loads`
            // result, a value through an erased generic): the runtime's adapter.
            Some(Type::PyInt) => Ok(RustExpr::chain(self.carrier_format(self.carrier()?, format_args!(
                "<SmeltPyInt as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})", smelt_owned_text = smelt_owned_text.delimited()
            )))),
            // Reading an erased value out into a `String` slot is not the JS
            // `String(x)` CONVERSION -- nothing in the source asked for one; the
            // slot's declared type simply says `string`. So absence must not
            // become the conversion's answer `"undefined"`: the slot cannot
            // represent absence at all, and inventing a five-letter string for
            // it makes the erased side disagree with the very same absence
            // written as a literal, which the typed side renders as the type's
            // default (`String::new()`). `Null` has always answered
            // `String::new()` here rather than `"null"` for exactly that
            // reason; `Undefined` was the arm that had drifted, and a
            // `[3, undefined]` expectation therefore compared unequal to the
            // `[3, undefined]` a callee produced. The real conversion lives in
            // `strings.rs` (`String(x)`, template literals, a `replace`
            // replacer's return) and keeps `"undefined"`.
            //
            // The honest representation of an absent `string` is
            // `Optional(String)`; where the declared type refuses it (here a
            // library overload asserting `[number, string][]` for a `zip` that
            // pads with `undefined`) the type's default is the closest the slot
            // can come -- and it is at least the SAME answer on both sides of a
            // comparison.
            // A JavaScript string target takes the erased payload itself and
            // transcodes only the text a non-string value coerces to.
            Some(Type::String) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other {{ {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}) }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"), smelt_owned_text = smelt_owned_text.delimited()
            )))),
            Some(Type::PyStr) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {smelt_owned_text} {{ {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"), smelt_owned_text = smelt_owned_text.delimited()
            )))),
            // Iterable-to-list extraction inspects the source through the
            // `SmeltUnknown` variant space (array/string/`Symbol.iterator`). The
            // source expression is usually erased already, but a statically typed
            // `SmeltList<_>` or `Option<SmeltList<_>>` (e.g. `Array.from(arr)`
            // where `arr: ArrayLike<T> | null | undefined`) reaches here too, so
            // normalize through the `IntoSmeltUnknown` boundary adapter first
            // rather than matching `SmeltUnknown::` arms against a non-erased
            // value. The adapter is identity on an existing `SmeltUnknown` and
            // preserves the backing array id, so already-erased callers are
            // unaffected.
            Some(Type::List(_)) if self.list_items_render_as_unknown(target, scope) => {
                Ok(erased_to_list_expr(text, None, &self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::String(smelt_ch)"))))
            }
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::String) => {
                Ok(erased_to_list_expr(
                    text,
                    Some("<JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)"),
                    &RustExpr::path("smelt_ch"),
                ))
            }
            Some(Type::List(item)) if self.mir.types.get(*item).is_some_and(Type::is_text) => {
                Ok(erased_to_list_expr(
                    text,
                    Some(
                        "if let SmeltUnknown::String(smelt_text) = smelt_value { smelt_text.to_string() } else { smelt_value.to_string() }",
                    ),
                    &RustExpr::chain("smelt_ch.to_string()"),
                ))
            }
            Some(Type::List(item)) => {
                let rendered_item = self.extract_rendered(&RustExpr::path("smelt_value"), *item, scope)?;
                let holds_record = record_coercion::type_holds_generated_record(self.mir, *item);
                let item_text = if holds_record {
                    RustExpr::atom(format!("{{ smelt_conv_next_item(); {rendered_item} }}", rendered_item = rendered_item.delimited()))
                } else {
                    rendered_item
                };
                let shared = SharedItemConversion::new(item_text);
                let char_text = match shared.binding() {
                    Some(_) => RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                        "{SHARED_ITEM}(SmeltUnknown::String(smelt_ch))"
                    ))),
                    None => self.extract_rendered(
                        &self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::String(smelt_ch)")),
                        *item,
                        scope,
                    )?,
                };
                let list = shared.wrap(erased_to_list_expr(text, Some(shared.use_text()), &char_text));
                Ok(if holds_record {
                    RustExpr::atom(format!("{{ let _smelt_conv_list = smelt_conv_enter_list(); {list} }}", list = list.delimited()))
                } else {
                    list
                })
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key).is_some_and(Type::is_text)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                // An erased object read as a dictionary is its own store,
                // shared, except a class value: its enumerable static fields
                // (`Object.keys(Box)`) live in their storage
                // (`smelt_object_dict_view`, in a program with class values).
                let object_view = if self.context.demand().has(Need::ClassValue) {
                    "smelt_object_dict_view(&smelt_value)"
                } else {
                    "smelt_value.smelt_shared_record()"
                };
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(smelt_value) => {object_view}, SmeltUnknown::Array(smelt_values) => smelt_values.own_entries().into_iter().map(|(smelt_key, smelt_value)| (JsString::from(smelt_key), smelt_value)).collect(), SmeltUnknown::String(smelt_value) => smelt_value.units().iter().enumerate().map(|(smelt_index, smelt_unit)| (JsString::from(smelt_index.to_string()), SmeltUnknown::String(JsString::from_units(&[*smelt_unit])))).collect(), SmeltUnknown::Function(smelt_value) => SmeltRecord::from([(JsString::from(\"{SMELT_CALL}\"), SmeltUnknown::Function(smelt_value))]), _ => SmeltRecord::new() }}", text = text.delimited()
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let shared = SharedItemConversion::new(
                    self.extract_rendered(&RustExpr::path("smelt_value"), *item, scope)?,
                );
                let item_text = shared.use_text();
                // An erased object that is a view of a typed record
                // (`emitter::live_view`) is read as a dictionary VIEW of that
                // record: the record stays one object, and the dictionary's
                // reads and writes are the record's. Any other object is its
                // own store and is copied entry by entry, as before.
                let object_view = if crate::runtime_prelude::flag(self.context, "live_views") == Some(true)
                    && let Ok(erased_item) = self.erase_rendered(&RustExpr::path("smelt_typed").cloned_value(), *item)
                {
                    format!(
                        "match smelt_values.smelt_typed_view(move |smelt_value: SmeltUnknown| {item_text}, |smelt_typed| {erased_item}) {{ Some(smelt_view) => smelt_view, None => SmeltRecord::with_id_from_entries(smelt_values.id, smelt_values.into_iter().map(|(smelt_key, smelt_value)| (smelt_key, {item_text}))) }}",
                        erased_item = erased_item.delimited()
                    )
                } else {
                    format!("SmeltRecord::with_id_from_entries(smelt_values.id, smelt_values.into_iter().map(|(smelt_key, smelt_value)| (smelt_key, {item_text})))")
                };
                Ok(shared.wrap(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(smelt_values) => {object_view}, SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().enumerate().map(|(smelt_index, smelt_value)| (JsString::from(smelt_index.to_string()), {item_text})).collect(), SmeltUnknown::String(smelt_value) => smelt_value.units().iter().enumerate().map(|(smelt_index, smelt_unit)| {{ let smelt_value = SmeltUnknown::String(JsString::from_units(&[*smelt_unit])); (JsString::from(smelt_index.to_string()), {item_text}) }}).collect(), _ => SmeltRecord::new() }}", text = text.delimited()
                )))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key).is_some_and(Type::is_text) => {
                let shared = SharedItemConversion::new(
                    self.extract_rendered(&RustExpr::path("smelt_value"), *item, scope)?,
                );
                let item_text = shared.use_text();
                Ok(shared.wrap(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(smelt_values) => SmeltRecord::with_id_from_entries(smelt_values.id, smelt_values.into_iter().map(|(smelt_key, smelt_value)| (smelt_key.to_string_lossy(), {item_text}))), SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().enumerate().map(|(smelt_index, smelt_value)| (smelt_index.to_string(), {item_text})).collect(), SmeltUnknown::String(smelt_value) => smelt_value.code_point_strings().into_iter().enumerate().map(|(smelt_index, smelt_ch)| {{ let smelt_value = SmeltUnknown::String(smelt_ch); (smelt_index.to_string(), {item_text}) }}).collect(), _ => SmeltRecord::new() }}", text = text.delimited()
                )))))
            }
            // A property-key record reads its keys back through the inverse of
            // the storage encoding: a reserved `__smelt_symbol` key is the
            // symbol it was written for, every other key the string it is.
            // An `unknown`-valued one recovers a second handle on the erased
            // object's own store, so the record and the value it was read
            // from stay one object (`data as Record<PropertyKey, unknown>`).
            Some(Type::Dict(key, item)) if self.is_prop_key_record_key(*key) => {
                if self.mir.types.get(*item) == Some(&Type::Unknown) {
                    return Ok(RustExpr::chain(self.carrier_format(self.carrier()?, format_args!(
                        "SmeltPropKeyRecord::from_shared_unknown({smelt_owned_text})", smelt_owned_text = smelt_owned_text.delimited()
                    ))));
                }
                let item_text = self.extract_rendered(&RustExpr::path("smelt_value"), *item, scope)?;
                Ok(RustExpr::chain(self.carrier_format(self.carrier()?, format_args!(
                    "SmeltPropKeyRecord::from_unknown_with({smelt_owned_text}, |smelt_value| {item_text})",
                    smelt_owned_text = smelt_owned_text.delimited(), item_text = item_text.delimited()
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key).is_none_or(|candidate| !candidate.is_text()) => {
                let key_text = self.rendered_at_type(&RustExpr::path("smelt_key"), self.type_id(Type::String)?, *key, scope)?;
                let item_text = self.extract_rendered(&RustExpr::path("smelt_value"), *item, scope)?;
                if self.dict_uses_js_key_map(*key) {
                    return Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                        "if let SmeltUnknown::Object(smelt_values) = {smelt_owned_text} {{ SmeltJsMap::from_iter(smelt_values.into_iter().map(|(smelt_key, smelt_value)| ({key_text}, {item_text}))) }} else {{ SmeltJsMap::new() }}", smelt_owned_text = smelt_owned_text.delimited(), key_text = key_text.delimited(), item_text = item_text.delimited()
                    ))));
                }
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "if let SmeltUnknown::Object(smelt_values) = {smelt_owned_text} {{ smelt_values.into_iter().map(|(smelt_key, smelt_value)| ({key_text}, {item_text})).collect::<::indexmap::IndexMap<_, _>>() }} else {{ ::indexmap::IndexMap::new() }}", smelt_owned_text = smelt_owned_text.delimited(), key_text = key_text.delimited(), item_text = item_text.delimited()
                ))))
            }
            // A source `Map` recovers through `SmeltJsMap`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_map` marker: an erased Map
            // rebuilds its entries (and stable id) from the marker payload, and a
            // plain object falls back to string-keyed entries. This is the
            // inverse of the `SmeltJsMap` erasure adapter, so `unknown -> Map`
            // extraction stays lossless.
            Some(Type::JsMap(_, _)) => {
                let target_text = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())", text = text.delimited()
                ))))
            }
            Some(Type::TypeParam { name }) if scope.spells(*name) => {
                let param_name = crate::names::type_param(self.symbol_name(*name)?).into_string();
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "<{param_name} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())", text = text.delimited()
                ))))
            }
            Some(Type::TypeParam { .. }) => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!("({text}).into_smelt_unknown()", text = text.delimited())))),
            // A concrete union stores a tagged `SmeltUnion…` enum, so an erased
            // `SmeltUnknown` value extracted into that destination must be
            // reconstructed into the matching variant rather than passed through
            // as `SmeltUnknown`. This mirrors the `from_smelt_unknown` boundary
            // `inject_union_value` applies for `Unknown`/`TypeParam` sources;
            // here the source is a wider (non-concrete) union still stored erased.
            Some(Type::Union(_)) if self.concrete_union_members(target).is_some() => Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{}::from_smelt_unknown(({text}).into_smelt_unknown())",
                union::union_name(target), text = text.delimited()
            )))),
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(Type::Optional(inner, absence)) => {
                self.extract_optional(text, *inner, *absence, scope)
            }
            Some(Type::Tuple(items)) => self.extract_tuple(&smelt_owned_text, items, scope),
            // The prelude `SmeltFromUnknown` impl is the single inverse of the
            // `SmeltRegExp` erasure adapter, so `source`, `flags` AND the
            // writable `lastIndex` all survive a round trip through erased
            // dataflow (`clone(re)`, `structuredClone`, an `unknown` bag).
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})", smelt_owned_text = smelt_owned_text.delimited()
            )))),
            // Both synthetic match-result classes are backed by the concrete
            // `SmeltMatch` Rust type, whose prelude `SmeltFromUnknown` impl is
            // the inverse of its erasure adapter. Without this arm the generic
            // class fallback below answers `Default::default()`, silently
            // turning a round-tripped match into an EMPTY match.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                "<SmeltMatch as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())", text = text.delimited()
            )))),
            // The inverse of the fetch-type erasure. Without this arm the generic
            // class fallback answers `Default::default()`, so a header list or a
            // parameter list that round-tripped through erased dataflow came back
            // EMPTY with no diagnostic.
            Some(Type::Class { .. }) if self.is_fetch_runtime_class_type(target)? => {
                let rust_type = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "<{rust_type} as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})", smelt_owned_text = smelt_owned_text.delimited()
                ))))
            }
            Some(Type::Class { .. })
                if self.renders_erased(target)? =>
            {
                Ok(text.to_owned())
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(target) => Ok(text.to_owned()),
            Some(Type::Class { .. }) if self.can_extract_unknown_object_record(target) => self.extract_object_record(text, target, scope),
            // A source `Set` recovers through `SmeltJsSet`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_set` marker (rebuilding members
            // and the stable id) and tolerantly accepts a bare array. This is the
            // inverse of the erasure adapter, so an `unknown -> Set` cast (e.g.
            // `data as unknown as Set` in a deep-equality walk) recovers the real
            // members instead of an empty set. `SmeltPrimSet` has the same impl.
            Some(Type::Set(_)) => {
                let target_text = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())", text = text.delimited()
                ))))
            }
            Some(Type::Class { .. }) => {
                Ok(RustExpr::atom("Default::default()"))
            }
            Some(Type::Function(function)) => self.extract_function(target, function, scope, &smelt_owned_text),
            // An already-erased `SmeltUnknown` at a `Type::Future` position is a
            // `SmeltUnknown::Promise` (e.g. `await`ing the result of an erased
            // vitest-mock call, or an `x as Promise<T>` cast). It CAN be recovered:
            // `SmeltFuture::from_erased` is the same promise seen at `T`, settling
            // when the erased one does (flattening a nested promise) with its value
            // extracted to the declared `Output`, so its reactions keep their turn. Discarding it for `SmeltFuture::resolved`
            // of a default would drop the real resolved value and leave the shared
            // settle state unobserved (breaking, e.g., mock result matchers and
            // chained awaits on the recovered promise).
            Some(Type::Future(output)) => {
                let extracted = self.extract_rendered(&RustExpr::path("smelt_awaited"), *output, scope)?;
                // The erased source is read OUTSIDE the `async move` block. The
                // block is `'static`, so a `{text}` that borrows — a callback
                // parameter passed by shared reference
                // (`callback_param_is_shared_reference`) is the case that reaches
                // here — cannot be named inside it (E0521). Reading it eagerly is
                // also what the source does: the promise value already exists at
                // this point; only the `await` is deferred.
                Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_erased_future = ({text}).into_smelt_unknown(); SmeltFuture::from_erased(smelt_erased_future, move |smelt_awaited| Ok::<_, {ERROR_BOX}>({extracted})) }}", text = text.delimited(), extracted = extracted.delimited()
                ))))
            }
            other => Err(EmitError::new(format!(
                "checked extraction from unknown expression `{text}` to {other:?} is not implemented yet", text = text.delimited()
            ))),
        }
    }
}
