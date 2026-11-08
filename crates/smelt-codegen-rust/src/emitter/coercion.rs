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

mod erase;
mod extract;
mod function_adapter;
mod prop_key;
pub(crate) mod record;
mod record_adapter;
mod tag;

use crate::rust::RustType;
use crate::rust::RustExpr;
use super::*;

use crate::erased_list_alias;
use crate::rust::RustShape;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// A list value at a tuple type: each position read out of the list, or the
    /// source item's default past its end.
    fn tuple_from_list_expr(
        &self,
        value_text: &RustExpr,
        source_item: TypeId,
        target_items: &[TypeId],
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        let items_text = target_items
            .iter()
            .enumerate()
            .map(|(index, target_item)| {
                let item = RustExpr::chain(format!(
                    "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                    self.default_value(source_item)?
                ));
                self.rendered_at_type(&item, source_item, *target_item, scope)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let tuple_text = if target_items.len() == 1 {
            format!("({items_text},)")
        } else {
            format!("({items_text})")
        };
        Ok(RustExpr::atom(format!("{{ let smelt_tuple_values = {value_text}.to_vec(); {tuple_text} }}")))
    }

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

    /// Render a collected container's target type so it names what the element
    /// mappers above it actually produce.
    ///
    /// The four map-and-collect arms in this module build each entry by coercing
    /// it to the target's key/value type, then annotate the `collect::<..>()`
    /// with the container type. Entries and annotation are two sides of ONE
    /// render position, so both are rendered under the same `scope`: whatever
    /// `scope` decided about a `Type::TypeParam` in the entries
    /// (`rendered_at_type`'s type-parameter arm, `extract_rendered`'s
    /// recovery arm) is what the turbofish spells.
    ///
    /// That is the whole of H42. Before it, this rendered under
    /// `TypeSubstitution::erased()` UNCONDITIONALLY — installed by H41a so the
    /// annotation would agree with the coercion path's own unconditional
    /// erasure — and the two still disagreed whenever an entry took the
    /// recovery path instead, because that path respected the lexical scope.
    /// The entries then carried `T` while the annotation said `SmeltUnknown`,
    /// which is the `E0277` the Hono router slice failed on. Narrowing the
    /// annotation ALONE to the lexical scope was measured and made things worse
    /// (the slice went from 15 errors to 27): no choice of substitution for one
    /// side can satisfy a rule the other side derives independently. Threading
    /// the scope is what removes the independence.
    ///
    /// On `SmeltUnknown`: this still writes `SmeltUnknown` into a turbofish
    /// wherever `scope` cannot spell the type parameter — a hoisted module-level
    /// item, or a callee the crate-wide generic-emission gate demoted to an
    /// erased signature. That is a genuine erased-interop boundary decided
    /// upstream of this function, and the annotation is only naming what the
    /// iterator really holds. It is no longer imprecision: where the parameter
    /// IS spellable the entries keep it and so does this.
    fn collected_container_type(
        &self,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustType, EmitError> {
        self.rust_type(target, false, &scope.substitution())
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
        let operand_expr = self.operand_expr(operand)?;
        if let Some(key) = self.prop_key_conversion(&operand_expr, source_ty, target)? {
            return Ok(key);
        }
        // A function operand renders as a borrow of its binding, and a union arm
        // owns its value: the binding may be read again after the injection.
        let owned_text = if matches!(self.mir.types.get(source_ty), Some(Type::Function(_)))
            && matches!(operand, Operand::Copy(Place::Local(_)))
        {
            RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.clone()")).ending_in_clone()
        } else {
            operand_expr.clone()
        };
        if let Some(injected) = self.inject_union_value(&owned_text, source_ty, target, scope)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value(&operand_expr, source_ty, target)? {
            return Ok(projected);
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
        if source_ty == target
            && !matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
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
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && self.optional_inner_preserves_erased_singletons(*inner)
        {
            return Ok(RustExpr::chain(format!(
                "Some({})",
                self.rendered_at_type(&self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null")), self.type_id(Type::Unknown)?, *inner, scope)?
            )));
        }
        // The same distinction in a `null | undefined` slot (`Option<()>`, see
        // the frontend's `null_or_undefined_join`): `null` is the present unit.
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && *inner == self.none_ty
        {
            return Ok(RustExpr::chain("Some(())"));
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

    /// Optional rules of [`Self::value_at_type_in`].
    fn value_at_optional(
        &self,
        operand: &Operand,
        target: TypeId,
        scope: &RenderScope,
        source_ty: TypeId,
        operand_expr: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_expr(operand)?;
            if self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner)) {
                return Ok({ let receiver = value_text.cloned_value(); RustExpr::postfix_on(&receiver, format!("{receiver}.flatten()")) });
            }
            if source_inner == target_inner {
                return Ok(value_text);
            }
            return self.optional_value_map_expr(&value_text, *source_inner, *target_inner, scope);
        }
        if matches!(self.mir.types.get(target), Some(Type::Optional(_)))
            && matches!(
                operand,
                Operand::Copy(Place::Field { .. }) | Operand::Move(Place::Field { .. })
            )
            && (matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(self.operand_ty(operand)?))
        {
            return self.extract(operand, target, scope);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            // A JS element read flowing into an optional slot keeps its own
            // fallibility instead of being made total and then re-wrapped in
            // `Some(..)`. Without this, `last<T>(arr: T[]): T | undefined`
            // returned `Some(Default::default())` for an empty array.
            if let Some(read) = self.optional_element_read_expr(operand, *inner)? {
                return Ok(read);
            }
            let operand_ty = self.operand_ty(operand)?;
            if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(source_inner)) if matches!(self.mir.types.get(*source_inner), Some(Type::Unknown)))
                && (self.has_dynamic_shape(*inner))
            {
                return self.operand_expr(operand);
            }
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok(RustExpr::path("None"));
            }
            // A union with a `null`/`undefined` arm is stored erased, and its
            // nullish values are the slot's ABSENT case. Wrapping the whole
            // value in `Some(..)` sent `null` into the inner type's
            // reconstruction instead, which picked whichever arm accepts it:
            // `new Response(null as Data | null)` built a
            // `URLSearchParams` body and answered a 204 with
            // `content-type: application/x-www-form-urlencoded`. `extract`
            // tests for nullish first.
            if matches!(
                self.mir.types.get(operand_ty),
                Some(Type::Union(items)) if items.iter().any(|item| matches!(self.mir.types.get(*item), Some(Type::None)))
            ) {
                return self.extract(operand, target, scope);
            }
            if operand_ty == *inner {
                return Ok(RustExpr::chain(format!("Some({})", self.value_at_type_in(operand, *inner, scope)?)));
            }
            if self.can_coerce_to_optional_inner(operand_ty, *inner) {
                return Ok(RustExpr::chain(format!("Some({})", self.value_at_type_in(operand, *inner, scope)?)));
            }
            if self.has_dynamic_shape(*inner)
            {
                return Ok(RustExpr::chain(format!("Some({})", self.value_at_type_in(operand, *inner, scope)?)));
            }
            if matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                && matches!(self.mir.types.get(operand_ty), Some(Type::Function(_)))
            {
                return Ok(RustExpr::chain(format!("Some({})", self.value_at_type_in(operand, *inner, scope)?)));
            }
            if matches!(
                self.mir.types.get(operand_ty),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            ) {
                return Ok(RustExpr::path("None"));
            }
        }
        self.value_at_callable_or_list(operand, target, scope, source_ty, operand_expr)
    }

    /// Scalar, function and list rules of [`Self::value_at_type_in`].
    fn value_at_callable_or_list(
        &self,
        operand: &Operand,
        target: TypeId,
        scope: &RenderScope,
        source_ty: TypeId,
        operand_expr: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(self.operand_ty(operand)?)
        {
            return self.extract(operand, target, scope);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Int)
        {
            // `operand_expr` is always a primary/postfix expression (an
            // identifier, literal, `.clone()`, field, or index), so the cast
            // never reassociates against a surrounding operator and needs no
            // defensive wrapping. Emitting it bare avoids the `unused_parens`
            // warning it drew in every argument/return/let position. The only
            // context that would need parentheses is a following method call,
            // which no consumer of this seam appends to a coercion result.
            return Ok(RustExpr::cast(format!("{} as f64", self.operand_expr(operand)?)));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Float)
        {
            // Keep the inner parentheses around the `as f64` cast: `.trunc()` is a
            // method call whose receiver must be grouped.
            return Ok(RustExpr::loose(format!(
                "({} as f64).trunc() as i64",
                self.operand_expr(operand)?
            )));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Function(_))
            )
        {
            if let Some(adapter) = self.erased_rest_function_value(operand, target)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.rest_vector_function_adapter(operand, target, false)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.function_shape_adapter(operand, target, false, None)? {
                return Ok(adapter);
            }
            let text = self.operand_expr(operand)?;
            if let Operand::Copy(place) | Operand::Move(place) = operand
                && self.is_function_parameter_place(place)?
            {
                return self.borrowed_function_handle_expr(&text, target);
            }
            if self.is_borrowed_callback_capture_name(&text) {
                return self.borrowed_function_handle_expr(&text, target);
            }
            // `text` already renders an owned value here; a second clone would
            // deep-copy the temporary for nothing.
            return Ok(text.cloned_value());
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
        {
            // As above: one clone, and the default only on the absent path.
            return Ok({ let receiver = self.operand_borrow_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone().unwrap_or_else(|| {})", self.default_value(target)?)) });
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?) {
            let value_text = self.rendered_at_type(&RustExpr::path("value"), *inner, target, scope)?;
            return Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone().map_or({}, |value| {value_text})", self.default_value(target)?)) });
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && source_item != target_item
        {
            // Distinct MIR item types can already share the same erased Rust
            // representation. Keep the buffer when no element adapter is needed.
            // The two sides answer to different scopes: the operand is a value
            // of the item being emitted, so its type was rendered at that item's
            // own lexical scope, while the target is rendered at `scope`, which a
            // callee argument narrows to what the callee also spells.
            //
            // Boundary: when the source element is an erased type parameter,
            // the value leaves the erased-generic ABI through the named alias
            // helper, which adds no erasure. A source `unknown[]` keeps the
            // general conversion. See `crate::erased_list_alias`.
            if self.list_items_render_as_unknown(self.operand_ty(operand)?, &self.render_scope())
                && self.list_items_render_as_unknown(target, scope)
            {
                let text = self.operand_expr(operand)?;
                if erased_list_alias::leaves_erased_generic(self.mir.types.get(*source_item)) {
                    return Ok(erased_list_alias::list_alias_expr(&text));
                }
                return Ok(RustExpr::atom(format!("Into::<SmeltList<_>>::into({text})")));
            }
            // A `List<None>` erased element-wise would become `Null`, but an
            // `[undefined, …]` literal must erase to `Undefined` (the type lost
            // the distinction; only the defining constants carry it). Recover it
            // from the def-site so genuine `null` arrays keep the `Null` path.
            let target_item_is_erased = self.has_dynamic_shape(*target_item);
            if matches!(self.mir.types.get(*source_item), Some(Type::None))
                && target_item_is_erased
                && self.container_local_all_undefined_constants(operand)?
            {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_l: SmeltList<_> = {op}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Undefined).collect::<Vec<_>>()) }}",
                    op = self.operand_expr(operand)?.cloned_value()
                ))));
            }
            // A target element that is a GENERATED union (`number | number[]`
            // stored as `SmeltUnionN`) is concrete storage, not the erased
            // carrier: a list element injects into its arm through
            // `rendered_at_type` (`flat([[1, 2], [[3]]])`, E0308 when it
            // erased instead).
            let value_text = if matches!(self.mir.types.get(*source_item), Some(Type::List(_)))
                && self.has_dynamic_shape(*target_item)
                && self.concrete_union_members(*target_item).is_none()
            {
                self.carrier_expr(&RustExpr::chain("value.into_smelt_unknown()"))
            } else {
                self.rendered_at_type(&RustExpr::path("value"), *source_item, *target_item, scope)?
            };
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_l: SmeltList<_> = {op}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| {value_text}).collect::<Vec<_>>()) }}",
                op = self.operand_expr(operand)?.cloned_value()
            )));
        }
        if let Some(Type::List(target_item)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(*target_item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            )
        {
            return Ok(RustExpr::atom(format!(
                "SmeltList::from(vec![{}])",
                self.value_at_type_in(operand, *target_item, scope)?
            )));
        }
        if let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source.params.len() < target_function.params.len()
            || (source.may_throw || self.operand_closure_can_throw(operand)?)
                != target_function.may_throw
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            ))
        {
            return self
                .function_shape_adapter(operand, target, false, None)?
                .ok_or_else(|| EmitError::new("function adapter was unexpectedly unavailable"));
        }
        // A value asked for at `bool` is a TRUTHINESS test — the same rule
        // `rendered_at_type` states, reached here because this operand entry
        // point has its own arms and then falls through to the raw operand.
        // A `&&` chain whose middle operand is a string is the case that found
        // it: `nextP === undefined && p && …` lowers the chain's value into a
        // `bool` local, and the branch handed back `p.clone()` (E0308, hono's
        // trie-router `insert`). Stated last so it only catches the
        // fallthrough; every arm above already answered for its own shape.
        if matches!(self.mir.types.get(target), Some(Type::Bool))
            && !matches!(
                self.mir.types.get(source_ty),
                Some(Type::Bool | Type::Unknown | Type::TypeParam { .. })
            )
        {
            return self.value_truthy_expr(operand_expr, source_ty);
        }
        self.rendered_at_type(operand_expr, source_ty, target, scope)
    }

    /// Coerces a callable-object value into a function-typed destination by
    /// reading its synthetic `__smelt_call` slot.
    ///
    /// A TypeScript interface (or class) carrying a call signature lowers to a
    /// record struct whose declared members become fields and whose underlying
    /// callable is stored in a synthetic `__smelt_call` field (frontend
    /// `add_interface_call_signature_field`). Invoking such a value — `f(..)`
    /// where `f: DebouncedFunction` — lowers to a `closure_call` whose callee
    /// temporary is *function*-typed, so MIR asks the coercion seam for a
    /// record → function conversion. Without this rule the record source falls
    /// through to the function-typed fallbacks below, which fabricate a
    /// `default_value` stub: an empty closure that is then called, silently
    /// dropping the real call.
    ///
    /// Returns `None` unless the destination is a function type and the source
    /// is a record with a `__smelt_call` slot, so ordinary record and function
    /// coercions are untouched. The slot's own declared type drives the nested
    /// coercion, which reuses the normal function → function adapters (arity,
    /// throw, erased-rest) instead of duplicating them.
    ///
    /// That nested coercion can come back around: a call signature may return
    /// the callable interface itself (`interface Curried { (a: A): Curried }`),
    /// and the return-value adapter then asks for the same record → function
    /// conversion. [`Self::enter_type_expansion`] bounds that: a pair already
    /// being expanded yields `None` and falls through to the outer coercion's
    /// remaining rules rather than recursing forever.
    pub(super) fn callable_object_call_slot_expr(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(None);
        }
        let Some(call_ty) = self.callable_interface_call_field_ty(source) else {
            return Ok(None);
        };
        let Some(_guard) = self.enter_type_expansion(source, target) else {
            return Ok(None);
        };
        let slot_text = RustExpr::postfix_on(&value_text, format!("{value_text}.{SMELT_CALL}.clone()")).ending_in_clone();
        Ok(Some(self.rendered_at_type(&slot_text, call_ty, target, scope)?))
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
        if self.conversions.type_expansion_stack.borrow().contains(&(source, target)) {
            return None;
        }
        Some(helper_reservation::push_entry(
            &self.conversions.type_expansion_stack,
            (source, target),
        ))
    }

    /// Wrap a function value into a callable-interface record.
    ///
    /// A callable interface (`interface Use { (h: H): App; (p: string, h: H):
    /// App }`) is emitted as a record whose synthetic `__smelt_call` slot holds
    /// the implementation; assigning a function to it (`this.use = (arg1,
    /// ...handlers) => ..`, Hono's `HonoBase` constructor) stores that function
    /// in the slot. Without this the coercion found no adapter and emitted the
    /// record's `Default::default()`, so every later `app.use(..)` called an
    /// inert default and did nothing.
    ///
    /// Declines (`None`) unless `source` is a function type and `target` is a
    /// record carrying `__smelt_call` whose every OTHER field is optional (a
    /// bare function has no value for a required data member; those records
    /// keep the `Object.assign` construction path).
    fn callable_interface_from_function_expr(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !matches!(self.mir.types.get(source), Some(Type::Function(_))) {
            return Ok(None);
        }
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let Some(fields) = self.structural_record_fields(target) else {
            return Ok(None);
        };
        let mut call_slot = None;
        for field in &fields {
            if self.symbol_name(field.name)? == SMELT_CALL {
                call_slot = Some(field.ty);
            } else if !matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                return Ok(None);
            }
        }
        let Some(call_slot_ty) = call_slot else {
            return Ok(None);
        };
        let mut field_text = Vec::new();
        for field in &fields {
            let raw_name = self.symbol_name(field.name)?;
            let value = if raw_name == SMELT_CALL {
                self.callable_interface_slot_value(value_text, source, call_slot_ty, scope)?
            } else {
                self.default_value(field.ty)?
            };
            field_text.push(format!("{}: {value}", self.context.names().field_text(self.mir, field.name)));
        }
        self.callable_record_literal(field_text, *name, args.len()).map(Some)
    }

    /// Render a function value at a callable interface's `__smelt_call` slot.
    ///
    /// An OVERLOADED interface stores its implementation in the erased
    /// variadic slot (`SmeltErasedFunction`), because no single typed signature
    /// covers every overload. The typed source is therefore first erased to a
    /// runtime callable — whose adapter converts each positional runtime
    /// argument to the source's own parameter, and gathers a trailing rest
    /// parameter from the remaining ones — and that callable fills the slot.
    /// Coercing typed-to-erased-variadic directly instead read the slot's own
    /// `(...args: unknown[])` view and handed the whole argument vector to the
    /// source's FIRST parameter (E0308). A typed slot takes the ordinary
    /// function-to-function coercion.
    fn callable_interface_slot_value(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        slot_ty: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        let slot_is_erased = matches!(
            self.mir.types.get(slot_ty),
            Some(Type::Function(slot)) if self.is_erased_unknown_rest_function(slot)
        );
        if slot_is_erased {
            let unknown_ty = self.type_id(Type::Unknown)?;
            let erased = self.rendered_at_type(value_text, source, unknown_ty, scope)?;
            return self.rendered_at_type(&erased, unknown_ty, slot_ty, scope);
        }
        self.rendered_at_type(value_text, source, slot_ty, scope)
    }

    /// Coerce a regex match value to a LIST: the match's array view.
    ///
    /// A JavaScript match result IS an array, so wherever one is used at an
    /// array type — an array-method receiver, `for...of`, spread, a `string[]`
    /// parameter — its value is the list of numbered groups. The frontend
    /// types that view `(string | undefined)[]`; `SmeltMatch::to_array_view`
    /// builds exactly it (an unmatched group is `None`), and any other list
    /// target is reached from the view by the ordinary element-wise list
    /// coercion. This replaces erasing the match to `SmeltUnknown` and walking
    /// the erased array back into a list.
    ///
    /// When the view type is not in the program's type table the only target
    /// reached is `string[]` (TypeScript's own `RegExpMatchArray` element
    /// type): an unmatched group then reads `""`, the missing value an
    /// `Option<String>` takes at a `String` slot everywhere else.
    ///
    /// Returns `None` when `source` is not the match class, `target` is not
    /// a list, or the list's element slot is erased, leaving the caller's
    /// other rules (the erasure adapter) in charge.
    fn match_array_view_expr(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(source) else {
            return Ok(None);
        };
        if self.match_class_kind(*name)? != Some(smelt_stdlib::StdlibClass::Match) {
            return Ok(None);
        }
        let Some(Type::List(target_item)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        // An ERASED element slot keeps the erasure adapter: it records
        // `index`/`input`/`groups` against the array's identity, which a
        // `T[]` reader that narrows with `Array.isArray` still consults.
        if matches!(
            self.mir.types.get(*target_item),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return Ok(None);
        }
        let view = RustExpr::atom(format!("({value_text}).to_array_view()"));
        let interned_view_ty = self
            .find_type_id(&Type::String)
            .and_then(|string_ty| self.find_type_id(&Type::Optional(string_ty)))
            .and_then(|group_ty| self.find_type_id(&Type::List(group_ty)));
        if let Some(view_ty) = interned_view_ty {
            if view_ty == target {
                return Ok(Some(view));
            }
            return self.rendered_at_type(&view, view_ty, target, scope).map(Some);
        }
        if self.mir.types.get(*target_item).is_some_and(Type::is_text) {
            return Ok(Some(RustExpr::atom(format!(
                "SmeltList::new({view}.into_iter().map(|group| group.unwrap_or_default()).collect::<Vec<_>>())"
            ))));
        }
        Ok(None)
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
        if let Some(key) = self.key_union_at_string(value_text, source, target)? {
            return Ok(key);
        }
        if let Some(injected) = self.inject_union_value(value_text, source, target, scope)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value(value_text, source, target)? {
            return Ok(projected);
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
        // Text crossing between the two languages of a mixed crate: a
        // JavaScript string is UTF-16 (`JsString`), a Python `str` is UTF-8
        // (`String`). A frontend marks each crossing with a `TypeAssert` to the
        // receiving language's view of the type; containers reach here per
        // element through their own arms.
        match (self.mir.types.get(source), self.mir.types.get(target)) {
            (Some(Type::String), Some(Type::PyStr)) => {
                return Ok(RustExpr::postfix_on(&smelt_owned_value, format!("{smelt_owned_value}.to_string_lossy()")));
            }
            (Some(Type::PyStr), Some(Type::String)) => {
                return Ok(RustExpr::atom(format!("JsString::from({smelt_owned_value})")));
            }
            _ => {}
        }
        // A Python exception (or its `args`) at a `str` slot is its `str`: the
        // pytest `raises` lowering keeps the caught message as a string.
        if (self.is_py_exception_type(source) || self.is_py_exception_args_type(source))
            && matches!(self.mir.types.get(target), Some(Type::PyStr))
        {
            return Ok(RustExpr::postfix_on(value_text, format!("{value_text}.to_string()")));
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
        // It became reachable when `instanceof` started narrowing an erased
        // local (round 10): inside `x instanceof Blob ? (x as { type: string }) : ..`
        // the operand is now the concrete class rather than `unknown`, so the
        // cast has a concrete source where it used to have an erased one. The
        // blob runtime tier is what caught it.
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
            let erased = RustExpr::atom(self.carrier_format(format_args!("({smelt_owned_value}).into_smelt_unknown()")));
            return self.rendered_at_type(&erased, self.type_id(Type::Unknown)?, target, scope);
        }
        if source == target && matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            if self.is_borrowed_callback_capture_name(value_text) {
                return self.borrowed_function_handle_expr(value_text, target);
            }
            return Ok(smelt_owned_value);
        }
        self.rendered_at_same_container(value_text, source, target, scope, smelt_owned_value)
    }

    /// Rules of [`Self::rendered_at_type`] for two containers of one kind, element by element.
    fn rendered_at_same_container(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
        smelt_owned_value: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let awaited =
                self.rendered_at_type(&RustExpr::path("smelt_future_value"), *source_item, *target_item, scope)?;
            // Evaluate the source future expression BEFORE the `async move`
            // block: `value_text` (e.g. `reduce_async(arr.clone(), ..)`) only
            // borrows its outer captures, but an `async move` block would move
            // every named binding it references into the returned task (E0382).
            // Binding the source future outside moves just that handle in.
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_source_future = {value_text}; SmeltFuture::from_future(Box::pin(async move {{ let smelt_future_value = smelt_source_future.await?; Ok::<_, {ERROR_BOX}>({awaited}) }})) }}"
            )));
        }
        // Element-wise `List<A>` -> `List<B>` re-mapping. The operand-based
        // coercion (`coerce_operand_text`) already handles this for a place
        // operand, but this string-based entry point is reached when the source
        // list flows through an expression (e.g. an awaited erased future whose
        // static item is `SmeltList<SmeltUnknown>` coerced into the call site's
        // `SmeltList<f64>`). Drive the same `.into_iter().map(..)` rebuild off the
        // value expression, coercing each element from the source item type.
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_item != target_item
        {
            // Match operand coercion: an erased generic and unknown have the
            // same element representation, so conversion must preserve aliases.
            // As there, the source value was rendered at the emitted item's own
            // scope and the target at the position's `scope`. Same boundary
            // split as the operand twin: see `crate::erased_list_alias`.
            if self.list_items_render_as_unknown(source, &self.render_scope())
                && self.list_items_render_as_unknown(target, scope)
            {
                if erased_list_alias::leaves_erased_generic(self.mir.types.get(*source_item)) {
                    return Ok(erased_list_alias::list_alias_expr(&smelt_owned_value));
                }
                return Ok(RustExpr::atom(format!("Into::<SmeltList<_>>::into({smelt_owned_value})")));
            }
            let source_item = *source_item;
            let target_item = *target_item;
            let element_text = if matches!(self.mir.types.get(source_item), Some(Type::List(_)))
                && (self.has_dynamic_shape(target_item))
            {
                self.carrier_expr(&RustExpr::chain("value.into_smelt_unknown()"))
            } else {
                self.rendered_at_type(&RustExpr::path("value"), source_item, target_item, scope)?
            };
            // `value_text` may be a bare `Vec` (e.g. an inlined spread/concat
            // `[...list, ...args]`) rather than a `SmeltList`. Normalize through
            // `.into()` (reflexive for `SmeltList`, `From<Vec>` otherwise) so the
            // `.id()` identity read is always available.
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_l: SmeltList<_> = {smelt_owned_value}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| {element_text}).collect::<Vec<_>>()) }}"
            )));
        }
        if let (Some(Type::Tuple(source_items)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_items.len() == target_items.len()
        {
            // Element-wise coercion references `{value_text}.{index}` once per
            // field. When `value_text` is a non-trivial expression (e.g. an
            // inlined call `partition(arr, cb)`), duplicating it would evaluate
            // the call — and re-move any by-value arguments — once per element,
            // producing E0382. Materialize such an expression into a single
            // temporary and project the binding instead. A trivial place (a bare
            // name or field path) is safe to duplicate and kept inline to avoid
            // churn.
            let (prefix, base) = if value_text.is_trivial_place() {
                (String::new(), value_text.clone())
            } else {
                ("let smelt_tuple_src = ".to_owned(), RustExpr::path("smelt_tuple_src"))
            };
            let items_text = source_items
                .iter()
                .zip(target_items.iter())
                .enumerate()
                .map(|(index, (source_item, target_item))| {
                    self.rendered_at_type(
                        &RustExpr::tuple_field(&base, index),
                        *source_item,
                        *target_item,
                                            scope,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                RustExpr::atom(format!("({items_text},)"))
            } else {
                RustExpr::atom(format!("({items_text})"))
            };
            return if prefix.is_empty() {
                Ok(tuple_text)
            } else {
                Ok(RustExpr::atom(format!("{{ {prefix}{value_text}; {tuple_text} }}")))
            };
        }
        self.rendered_at_callable_or_record(value_text, source, target, scope, smelt_owned_value)
    }

    /// Callable-object, record and dict rules of [`Self::rendered_at_type`].
    fn rendered_at_callable_or_record(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
        smelt_owned_value: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if let Some(slot) = self.callable_object_call_slot_expr(value_text, source, target, scope)? {
            return Ok(slot);
        }
        if let Some(adapter) =
            self.rendered_function_shape_adapter(value_text, source, target, scope)?
        {
            return Ok(adapter);
        }
        if let Some(adapter) = self.structural_record_adapter(value_text, source, target, scope)? {
            return Ok(adapter);
        }
        // H42. A `Type::TypeParam` target erases ONLY where the render position
        // cannot spell it. This used to erase unconditionally, at any depth,
        // which contradicted `extract_rendered`'s recovery arm: the recovery
        // rebuilt the value as the Rust generic wherever the emitted item
        // declares it, so a container whose entries took one path and whose
        // annotation took the other could not type-check (the Hono router
        // slice's `E0277` over a `SmeltRecord<.., SmeltList<(T, String)>>`, and
        // its `SmeltList<(T, ..)>` vs `SmeltList<(SmeltUnknown, ..)>` twin
        // reached from the other side). Both sides now ask `scope`, so the
        // position's answer is the same wherever it is asked.
        //
        // This introduces no new erasure and removes some: where `scope` does
        // spell the name the value is recovered as that real Rust generic
        // instead of being boxed into the runtime carrier, which is exactly the
        // "concrete types, then scoped generics, then erasure" order CLAUDE.md
        // asks for. Where it does not spell the name — a hoisted module item, a
        // callee the generic-emission gate demoted — the erased rendering is
        // still the only spelling available, and is unchanged.
        if let Some(Type::TypeParam { name }) = self.mir.types.get(target)
            && scope.spells(*name)
        {
            return self.extract_rendered(value_text, target, scope);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(target).is_none()
            || self.is_erased_class_type(target)
        {
            return self.erase_rendered(value_text, source);
        }
        if let (Some(Type::Class { .. }), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.structural_record_to_string_dict_adapter(
                value_text,
                source,
                *target_key,
                *target_value,
                            scope,
            )?
        {
            return Ok(adapter);
        }
        if matches!(
            self.mir.types.get(source),
            // A `never`-returning source (e.g. a `(value: never) => value`
            // predicate) still evaluates to a real value at runtime, which the
            // emitter renders as an erased `SmeltUnknown`. Extract it into the
            // concrete target through the same `SmeltUnknown` discriminant path
            // as `Unknown`, so e.g. a `bool` target gets JS-truthiness coercion
            // rather than the raw erased value (E0308).
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Never)
        ) || self.is_erased_class_type(source)
        {
            // A concrete union stores a tagged `SmeltUnion…` enum, but the erased
            // extraction below matches over `SmeltUnknown` discriminants. When the
            // source carries a concrete union, project it back to its erased value
            // first so the extraction sees the `SmeltUnknown` shape it expects.
            // (A concrete-union → concrete-union coercion has already been handled
            // above by `project_union_value`.)
            if self.concrete_union_members(source).is_some() {
                let erased = self.erase_concrete_union_expr(value_text, source);
                return self.extract_rendered(&erased, target, scope);
            }
            return self.extract_rendered(value_text, target, scope);
        }
        self.rendered_at_scalar(value_text, source, target, scope, smelt_owned_value)
    }

    /// Scalar rules of [`Self::rendered_at_type`]: void, numbers, strings and host initializers.
    fn rendered_at_scalar(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
        smelt_owned_value: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if self.mir.types.get(source) == Some(&Type::None) {
            return self.default_value(target);
        }
        if matches!(self.mir.types.get(source), Some(Type::Function(_)))
            && !matches!(
                self.mir.types.get(target),
                Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !self.is_erased_class_type(target)
        {
            return self.default_value(target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::Int)
        {
            return Ok(RustExpr::atom(format!("({value_text} as f64)")));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(source) == Some(&Type::Float)
        {
            return Ok(RustExpr::atom(format!("(({value_text} as f64).trunc() as i64)")));
        }
        // A `FixedInt` is exactly an `f64`. The opposite direction is only
        // reached for a value the TypeScript frontend proved to be in range
        // (a Bun `int32` field of an object literal; see `lowering::int32`),
        // so the narrowing cast is exact.
        if let (Some(Type::Float), Some(Type::FixedInt(_))) =
            (self.mir.types.get(target), self.mir.types.get(source))
        {
            return Ok(RustExpr::atom(format!("({value_text} as f64)")));
        }
        if let (Some(Type::FixedInt(width)), Some(Type::Float)) =
            (self.mir.types.get(target), self.mir.types.get(source))
        {
            return Ok(RustExpr::atom(format!("({value_text} as {})", width.rust_name())));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source).is_some_and(Type::is_text)
        {
            let js_text = self.mir.types.get(source) == Some(&Type::String);
            return Ok(RustExpr::from(primitive_cast::string_to_number_call(value_text, js_text)));
        }
        if self.mir.types.get(target).is_some_and(Type::is_text)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Bool | Type::Int | Type::Float)
            )
        {
            if self.mir.types.get(target) == Some(&Type::String) {
                return Ok(RustExpr::chain(format!("JsString::from({value_text}.to_string())")));
            }
            return Ok(RustExpr::postfix_on(&value_text, format!("{value_text}.to_string()")));
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) && self.mir.types.get(source).is_some_and(Type::is_text)
        {
            return Ok(RustExpr::atom(format!("SmeltRegExp::new({value_text}, String::new())")));
        }
        // A value at a `Headers` slot is the spec's `HeadersInit` conversion of
        // it: WHATWG says a header list is built from a `Headers`, a
        // `Record<string, string>` or a sequence of name/value pairs, and
        // `headers_conversion_expr` is already that conversion — the `new
        // Headers(init)` constructor and every init-dictionary key go through
        // it. A coercion seam reaches the same pairing whenever an init arm
        // flows into a `Headers`-typed slot (Hono's
        // `responseHeaders ?? (headers as Record<string, string> | undefined)`),
        // and without the arm the record was assigned to the header list
        // unconverted.
        if self.is_headers_class_type(target)? && self.is_headers_init_type(source)? {
            return self.headers_conversion_expr(value_text, source);
        }
        self.rendered_at_optional(value_text, source, target, scope, smelt_owned_value)
    }

    /// Optional rules of [`Self::rendered_at_type`]: wrapping into, and unwrapping out of, `Option`.
    fn rendered_at_optional(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
        smelt_owned_value: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner))
        {
            return Ok(RustExpr::postfix_on(&smelt_owned_value, format!("{smelt_owned_value}.flatten()")));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            if source_inner == target_inner {
                return Ok(smelt_owned_value);
            }
            return self.optional_value_map_expr(&smelt_owned_value, *source_inner, *target_inner, scope);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
        {
            let mapped_value = self.rendered_at_type(&RustExpr::path("value"), source, *inner, scope)?;
            if self.optional_inner_preserves_erased_singletons(*inner) {
                return Ok(RustExpr::chain(format!("Some({mapped_value})")));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {smelt_owned_value} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some({mapped_value}) }}"
            ))));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && source == *inner
        {
            return Ok(RustExpr::chain(format!("Some({value_text})")));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            if self.mir.types.get(source) == Some(&Type::None) {
                return Ok(RustExpr::path("None"));
            }
            if self.can_coerce_to_optional_inner(source, *inner) {
                let mapped_value = self.rendered_at_type(value_text, source, *inner, scope)?;
                return Ok(RustExpr::chain(format!("Some({mapped_value})")));
            }
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source)
            && *inner == target
        {
            return Ok(RustExpr::postfix_on(&smelt_owned_value, format!(
                "{smelt_owned_value}.unwrap_or({})",
                self.default_value(target)?
            )));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source) {
            let mapped_value = self.rendered_at_type(&RustExpr::path("value"), *inner, target, scope)?;
            return Ok(RustExpr::postfix_on(&smelt_owned_value, format!(
                "{smelt_owned_value}.map_or({}, |value| {mapped_value})",
                self.default_value(target)?
            )));
        }
        self.rendered_across_containers(value_text, source, target, scope)
    }

    /// Conversions between container kinds: a tuple, list, dict or record read as another.
    fn rendered_across_containers(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // A fixed-arity tuple flowing into a list target (`[T, U] -> V[]`, as
        // when a `zip` result of tuples is passed to `unzipWith`'s
        // `readonly T[][]` parameter with `T` erased). Each tuple field is
        // coerced to the list element type and collected into a `SmeltList`
        // with a fresh id.
        if let (Some(Type::Tuple(source_items)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let source_items = source_items.clone();
            let target_item = *target_item;
            let (prefix, base) = if value_text.is_trivial_place() {
                (String::new(), value_text.clone())
            } else {
                ("let smelt_tuple_src = ".to_owned(), RustExpr::path("smelt_tuple_src"))
            };
            let items_text = source_items
                .iter()
                .enumerate()
                .map(|(index, source_item)| {
                    self.rendered_at_type(&RustExpr::tuple_field(&base, index), *source_item, target_item, scope)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let list_text =
                RustExpr::atom(format!("SmeltList::with_id(smelt_next_object_id(), vec![{items_text}])"));
            return if prefix.is_empty() {
                Ok(list_text)
            } else {
                Ok(RustExpr::atom(format!("{{ {prefix}{value_text}; {list_text} }}")))
            };
        }
        if let Some(Type::List(target_item)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(*target_item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !matches!(
                self.mir.types.get(source),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            )
        {
            let item_text = self.rendered_at_type(value_text, source, *target_item, scope)?;
            return Ok(RustExpr::atom(format!("SmeltList::from(vec![{item_text}])")));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            return self.tuple_from_list_expr(value_text, *source_item, target_items, scope);
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let item_text = self.rendered_at_type(&RustExpr::path("value"), *source_value, *target_item, scope)?;
            return Ok(RustExpr::atom_on(&value_text, format!(
                "{value_text}.into_iter().map(|(_, value)| {item_text}).collect::<SmeltList<_>>()"
            )));
        }
        if let (Some(Type::List(source_item)), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let int_ty = self.type_id(Type::Int)?;
            let key_text = if self.mir.types.get(*target_key).is_some_and(Type::is_text) {
                "index.to_string()".to_owned()
            } else {
                self.rendered_at_type(&RustExpr::cast("index as i64"), int_ty, *target_key, scope)?.into_string()
            };
            let item_text = self.rendered_at_type(&RustExpr::path("value"), *source_item, *target_value, scope)?;
            let target_text = self.collected_container_type(target, scope)?;
            return Ok(RustExpr::opaque(format!(
                "{value_text}.into_iter().enumerate().map(|(index, value)| ({key_text}, {item_text})).collect::<{target_text}>()"
            )));
        }
        if let (
            Some(
                source_ty @ (Type::Dict(source_key, source_value)
                | Type::JsMap(source_key, source_value)),
            ),
            Some(
                target_ty @ (Type::Dict(target_key, target_value)
                | Type::JsMap(target_key, target_value)),
            ),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
            && (source_key != target_key
                || source_value != target_value
                || self.map_backing_differs(source_ty, target_ty))
        {
            let key_text = if self.mir.types.get(*target_key).is_some_and(Type::is_text) {
                self.property_key_to_string_expr(&RustExpr::path("key"), *source_key)?
            } else {
                self.rendered_at_type(&RustExpr::path("key"), *source_key, *target_key, scope)?
            };
            let mapped_value_text =
                self.rendered_at_type(&RustExpr::path("value"), *source_value, *target_value, scope)?;
            let target_text = self.collected_container_type(target, scope)?;
            return Ok(RustExpr::opaque(format!(
                "{value_text}.into_iter().map(|(key, value)| ({key_text}, {mapped_value_text})).collect::<{target_text}>()"
            )));
        }
        if let (Some(Type::Dict(source_key, source_value)), Some(Type::Class { .. })) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.string_dict_record_adapter(
                value_text,
                *source_key,
                *source_value,
                target,
                            scope,
            )?
        {
            return Ok(adapter);
        }
        // Last resort, after every structural rule above has declined: a value
        // asked for at `string` is its STRING CONVERSION. That is what
        // JavaScript does wherever a string is expected, and it is the sibling
        // of the truthiness arm near the top of this function ("a value asked
        // for at `bool` is a truthiness test, not a cast").
        //
        // It is reached only where nothing else had an answer, and where
        // nothing else has an answer the value used to be handed back at its
        // own type — which does not compile, so no working conversion is
        // displaced. tsc has already rejected any genuinely ill-typed
        // assignment before Smelt runs; what survives is the shape TypeScript
        // itself permits, `never` flowing into a string slot:
        //
        // ```ts
        // const bufferToString = (buffer: ArrayBuffer): string => {
        //   if (buffer instanceof ArrayBuffer) { return decoder.decode(buffer); }
        //   return buffer;   // `buffer` is `never` here
        // };
        // ```
        //
        // The CLOSURE-bodied spelling of that same function already emitted
        // exactly this conversion (its return channel erases first), so this
        // makes the two paths agree rather than inventing a rule for one.
        if self.mir.types.get(target).is_some_and(Type::is_text)
            && matches!(
                self.mir.types.get(source),
                Some(
                    Type::Class { .. }
                        | Type::List(_)
                        | Type::Set(_)
                        | Type::Dict(_, _)
                        | Type::JsMap(_, _)
                        | Type::Tuple(_)
                )
            )
        {
            let erased = self.erase_rendered(value_text, source)?;
            return self.extract_rendered(&erased, target, scope);
        }
        Ok(value_text.to_owned())
    }
}

/// Scope guard that pops a type pair off
/// [`FunctionEmitter::type_expansion_stack`] when the expansion finishes.
///
/// Held by value at the expansion site (see
/// [`FunctionEmitter::enter_type_expansion`]) so the stack stays balanced even
/// when the expansion returns an `EmitError` through `?`.
type TypeExpansionGuard<'emit> =
    helper_reservation::StackEntryGuard<'emit, (TypeId, TypeId)>;

impl FunctionEmitter<'_> {
}
