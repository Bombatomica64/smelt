//! Coercion seam: the one module where a value crosses between its static Rust
//! type and the erased `SmeltUnknown` form. The public surface is a small set of
//! intent-named verbs; the per-type mechanics stay private below them:
//!
//! - `value_at_type(op, target)` / `value_at_type_text(text, src, target)` —
//!   coerce a value to a concrete target type (both endpoints are known and
//!   interned). The general entry; it dispatches to erase/extract internally.
//! - `erase(op)` / `erase_value_text(text, src)` — box a typed value into
//!   `SmeltUnknown`. This direction is *target-free*: it must not require a
//!   `Type::Unknown` to be interned (it often is not), so it cannot be spelled
//!   as `value_at_type(op, <Unknown TypeId>)`.
//! - `extract(op, target)` / `extract_value_text(text, target)` — pull a typed
//!   value back out of `SmeltUnknown`.
//! - `tag_check(op, kind)` — runtime narrowing (`is this tag a String?`), which
//!   is a guard, not value coercion.
//!
//! See CONTEXT.md (## Coercion).

use super::*;
use crate::emitter::rendered_text_rewrite::cloned_value_text;
use crate::rust::RustIdent;
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
        Ok(self.type_text(left)? == self.type_text(right)?)
    }

    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    pub(super) fn value_at_type(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let source_ty = self.operand_ty(operand)?;
        let operand_text = self.operand_text(operand)?;
        if let Some(injected) = self.inject_union_value_text(&operand_text, source_ty, target)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value_text(&operand_text, source_ty, target)? {
            return Ok(projected);
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
                Some(Type::String)
            )
        {
            let field_text = self.string_field_text(&self.local_value_text(*base)?, *field)?;
            let field_source_ty = match self.symbol_name(*field)? {
                "source" => self.type_id(Type::String)?,
                "global" | "ignoreCase" | "ignore_case" | "multiline" => {
                    self.type_id(Type::Bool)?
                }
                "length" => self.type_id(Type::Int)?,
                _ => self.type_id(Type::Unknown)?,
            };
            return self.erase_value_text(&field_text, field_source_ty);
        }
        if source_ty == target
            && !matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
            return self.operand_text(operand);
        }
        if let Some(Type::TypeParam { name }) = self.mir.types.get(target)
            && self.current_function_has_type_param(*name)
        {
            return self.extract(operand, target);
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
            if let Some(read) = self.erased_element_read_text(operand)? {
                return Ok(read);
            }
            return self.erase(operand);
        }
        if self.is_erased_class_type(target) {
            if let Some(read) = self.erased_element_read_text(operand)? {
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
        // `optional_erased_singleton_equality_text` answer `x === null`
        // correctly; folding it into `None` made `x !== null` true for a slot
        // that holds `null`. Slots whose payload cannot carry the tag (a
        // concrete inner or a tagged union with no nullish arm) keep the
        // collapsed `None` encoding.
        if matches!(operand, Operand::Const(Constant::None))
            && let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && self.optional_inner_preserves_erased_singletons(*inner)
        {
            return Ok(format!(
                "Some({})",
                self.value_at_type_text("SmeltUnknown::Null", self.type_id(Type::Unknown)?, *inner)?
            ));
        }
        if matches!(
            operand,
            Operand::Const(Constant::None | Constant::Undefined)
        ) {
            return self.default_value(target);
        }
        if let Some(slot) = self.callable_object_call_slot_text(
            &self.operand_text(operand)?,
            source_ty,
            target,
        )? {
            return Ok(slot);
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_text(operand)?;
            if self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner)) {
                return Ok(format!("{}.flatten()", cloned_value_text(&value_text)));
            }
            if source_inner == target_inner {
                return Ok(value_text);
            }
            let mapped_value = self.value_at_type_text("value", *source_inner, *target_inner)?;
            return Ok(format!("{value_text}.map(|value| {mapped_value})"));
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
            return self.extract(operand, target);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            // A JS element read flowing into an optional slot keeps its own
            // fallibility instead of being made total and then re-wrapped in
            // `Some(..)`. Without this, `last<T>(arr: T[]): T | undefined`
            // returned `Some(Default::default())` for an empty array.
            if let Some(read) = self.optional_element_read_text(operand, *inner)? {
                return Ok(read);
            }
            let operand_ty = self.operand_ty(operand)?;
            if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(source_inner)) if matches!(self.mir.types.get(*source_inner), Some(Type::Unknown)))
                && (matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*inner))
            {
                return self.operand_text(operand);
            }
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if operand_ty == *inner {
                return Ok(format!("Some({})", self.value_at_type(operand, *inner)?));
            }
            if self.can_coerce_to_optional_inner(operand_ty, *inner) {
                return Ok(format!("Some({})", self.value_at_type(operand, *inner)?));
            }
            if matches!(
                self.mir.types.get(*inner),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(*inner)
            {
                return Ok(format!("Some({})", self.value_at_type(operand, *inner)?));
            }
            if matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                && matches!(self.mir.types.get(operand_ty), Some(Type::Function(_)))
            {
                return Ok(format!("Some({})", self.value_at_type(operand, *inner)?));
            }
            if matches!(
                self.mir.types.get(operand_ty),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            ) {
                return Ok("None".to_owned());
            }
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(self.operand_ty(operand)?)
        {
            return self.extract(operand, target);
        }
        if let Some(adapter) = self.structural_record_adapter_text(
            &self.operand_text(operand)?,
            self.operand_ty(operand)?,
            target,
        )? {
            return Ok(adapter);
        }
        if let (Some(Type::Class { .. }), Some(Type::Dict(target_key, target_value))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && let Some(adapter) = self.structural_record_to_string_dict_adapter_text(
            &self.operand_text(operand)?,
            self.operand_ty(operand)?,
            *target_key,
            *target_value,
        )? {
            return Ok(adapter);
        }
        if matches!(self.mir.types.get(target), Some(Type::String))
            && let Some(Type::Class { name, .. }) = self.mir.types.get(self.operand_ty(operand)?)
            && self.is_regexp_class_symbol(*name)?
        {
            return Ok(Self::regexp_literal_text(&self.operand_text(operand)?));
        }
        if self.is_match_fn_result_type(self.operand_ty(operand)?)?
            && !self.is_match_fn_result_class_type(target)?
        {
            return self.extract_value_text(
                &format!("{}.value.clone()", self.operand_text(operand)?),
                target,
            );
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::String)
        {
            return Ok(format!(
                "SmeltRegExp::new({}, String::new())",
                self.operand_text(operand)?
            ));
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Function(_))
        ) && !matches!(
            self.mir.types.get(target),
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && !self.is_erased_class_type(target)
        {
            return self.default_value(target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Int)
        {
            // `operand_text` is always a primary/postfix expression (an
            // identifier, literal, `.clone()`, field, or index), so the cast
            // never reassociates against a surrounding operator and needs no
            // defensive wrapping. Emitting it bare avoids the `unused_parens`
            // warning it drew in every argument/return/let position. The only
            // context that would need parentheses is a following method call,
            // which no consumer of this seam appends to a coercion result.
            return Ok(format!("{} as f64", self.operand_text(operand)?));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Float)
        {
            // Keep the inner parentheses around the `as f64` cast — `.trunc()`
            // is a method call whose receiver must be grouped — but drop the
            // redundant outer pair the whole `... as i64` expression carried,
            // which produced `unused_parens` wherever the value stood alone.
            return Ok(format!(
                "({} as f64).trunc() as i64",
                self.operand_text(operand)?
            ));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::String)
        {
            return Ok(format!(
                "{}.parse::<f64>().unwrap_or(0.0)",
                self.operand_text(operand)?
            ));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Function(_))
            )
        {
            if let Some(adapter) = self.erased_rest_function_value_text(operand, target)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target, false)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.function_shape_adapter_text(operand, target, false, None)? {
                return Ok(adapter);
            }
            let text = self.operand_text(operand)?;
            if let Operand::Copy(place) | Operand::Move(place) = operand
                && self.is_function_parameter_place(place)?
            {
                return self.borrowed_function_handle_text(&text, target);
            }
            if self.is_borrowed_callback_capture_name(&text) {
                return self.borrowed_function_handle_text(&text, target);
            }
            // `text` already renders an owned value here; a second clone would
            // deep-copy the temporary for nothing.
            return Ok(cloned_value_text(&text));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
            && matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
            return Ok(format!(
                "{}.clone().unwrap_or({})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner))
        {
            return Ok(format!("{}.clone().flatten()", self.operand_text(operand)?));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
        {
            return Ok(format!(
                "{}.clone().unwrap_or({})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?) {
            let value_text = self.value_at_type_text("value", *inner, target)?;
            return Ok(format!(
                "{}.clone().map_or({}, |value| {value_text})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && source_item != target_item
        {
            // A `List<None>` erased element-wise would become `Null`, but an
            // `[undefined, …]` literal must erase to `Undefined` (the type lost
            // the distinction; only the defining constants carry it). Recover it
            // from the def-site so genuine `null` arrays keep the `Null` path.
            let target_item_is_erased = matches!(
                self.mir.types.get(*target_item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(*target_item);
            if matches!(self.mir.types.get(*source_item), Some(Type::None))
                && target_item_is_erased
                && self.container_local_all_undefined_constants(operand)?
            {
                return Ok(format!(
                    "{{ let smelt_l: SmeltList<_> = {op}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Undefined).collect::<Vec<_>>()) }}",
                    op = cloned_value_text(&self.operand_text(operand)?)
                ));
            }
            let value_text = if matches!(self.mir.types.get(*source_item), Some(Type::List(_)))
                && (matches!(
                    self.mir.types.get(*target_item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*target_item))
            {
                "value.into_smelt_unknown()".to_owned()
            } else {
                self.value_at_type_text("value", *source_item, *target_item)?
            };
            return Ok(format!(
                "{{ let smelt_l: SmeltList<_> = {op}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| {value_text}).collect::<Vec<_>>()) }}",
                op = cloned_value_text(&self.operand_text(operand)?)
            ));
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
            return Ok(format!(
                "SmeltList::from(vec![{}])",
                self.value_at_type(operand, *target_item)?
            ));
        }
        if let (Some(Type::Dict(_, _)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && matches!(self.mir.types.get(*target_item), Some(Type::Function(_)))
        {
            return Ok("SmeltList::new(Vec::new())".to_owned());
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.value_at_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{}.into_iter().map(|(_, value)| {value_text}).collect::<SmeltList<_>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Dict(target_key, target_value))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let int_ty = self.type_id(Type::Int)?;
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                "index.to_string()".to_owned()
            } else {
                self.value_at_type_text("index as i64", int_ty, *target_key)?
            };
            let value_text = self.value_at_type_text("value", *source_item, *target_value)?;
            let target_text = self.type_text_with_impl_trait(target, false)?;
            return Ok(format!(
                "{}.into_iter().enumerate().map(|(index, value)| ({key_text}, {value_text})).collect::<{target_text}>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_text(operand)?;
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.value_at_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.to_vec(); {tuple_text} }}"
            ));
        }
        if let (
            Some(
                source_map_ty @ (Type::Dict(source_key, source_value)
                | Type::JsMap(source_key, source_value)),
            ),
            Some(
                target_map_ty @ (Type::Dict(target_key, target_value)
                | Type::JsMap(target_key, target_value)),
            ),
        ) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source_key != target_key
            || source_value != target_value
            || self.map_backing_differs(source_map_ty, target_map_ty))
        {
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                self.property_key_to_string_text("key", *source_key)?
            } else {
                self.value_at_type_text("key", *source_key, *target_key)?
            };
            let mapped_value_text =
                self.value_at_type_text("value", *source_value, *target_value)?;
            let target_text = self.type_text_with_impl_trait(target, false)?;
            return Ok(format!(
                "{}.into_iter().map(|(key, value)| ({key_text}, {mapped_value_text})).collect::<{target_text}>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::Dict(source_key, source_value)), Some(Type::Class { .. })) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && let Some(adapter) = self.string_dict_record_adapter_text(
            &self.operand_text(operand)?,
            *source_key,
            *source_value,
            target,
        )? {
            return Ok(adapter);
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
                .function_shape_adapter_text(operand, target, false, None)?
                .ok_or_else(|| EmitError::new("function adapter was unexpectedly unavailable"));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        self.operand_text(operand)
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
    fn callable_object_call_slot_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(None);
        }
        let Some(call_ty) = self.callable_interface_call_field_ty(source) else {
            return Ok(None);
        };
        let Some(_guard) = self.enter_type_expansion(source, target) else {
            return Ok(None);
        };
        let slot_text = format!("{value_text}.__smelt_call.clone()");
        Ok(Some(self.value_at_type_text(&slot_text, call_ty, target)?))
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
        if self.type_expansion_stack.borrow().contains(&(source, target)) {
            return None;
        }
        self.type_expansion_stack.borrow_mut().push((source, target));
        Some(TypeExpansionGuard { stack: &self.type_expansion_stack })
    }

    /// Returns whether rendered value text is a trivial place that can be
    /// duplicated for free.
    ///
    /// A trivial place is a bare local name or a dotted field/tuple path such as
    /// `foo` or `foo.bar.0`. Duplicating one re-reads the same binding with no
    /// re-evaluation and no repeated move of by-value operands. Anything
    /// containing call/operator/whitespace syntax is treated as a non-trivial
    /// expression that must be materialized into a temporary before it is used
    /// more than once (see the tuple-to-tuple coercion in
    /// [`Self::value_at_type_text`]).
    fn expression_text_is_trivial_place(value_text: &str) -> bool {
        !value_text.is_empty()
            && value_text.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == ':'
            })
            && !value_text.starts_with(|ch: char| ch.is_ascii_digit())
    }

    /// Coerces already-rendered Rust value text from a known source type to a destination type.
    pub(super) fn value_at_type_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if let Some(injected) = self.inject_union_value_text(value_text, source, target)? {
            return Ok(injected);
        }
        if let Some(projected) = self.project_union_value_text(value_text, source, target)? {
            return Ok(projected);
        }
        // A bare `Default::default()` is an ambiguous inference source for the
        // collection adapters below, which drive `.clone().into_iter().map(…)`
        // off the receiver and cannot infer its element type (E0282). When the
        // source is a collection, substitute its concrete typed default (e.g.
        // `SmeltList::new(Vec::new())`): the same value, but element-typed so the
        // adapter's iterator resolves.
        let normalized_default;
        let value_text = if value_text == "Default::default()"
            && matches!(
                self.mir.types.get(source),
                Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _))
            ) {
            normalized_default = self.default_value(source)?;
            normalized_default.as_str()
        } else {
            value_text
        };
        // `value_text` is usually already an owned temporary (an operand render clones
        // the local it reads), so take an owned copy rather than deep-copying it a
        // second time. Bound AFTER the `Default::default()` normalization above, which
        // rebinds `value_text`.
        let smelt_owned_value = cloned_value_text(value_text);
        if source == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(value_text.to_owned());
        }
        if source == target && matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            if self.is_borrowed_callback_capture_name(value_text) {
                return self.borrowed_function_handle_text(value_text, target);
            }
            return Ok(format!("{smelt_owned_value}"));
        }
        if let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let awaited =
                self.value_at_type_text("smelt_future_value", *source_item, *target_item)?;
            // Evaluate the source future expression BEFORE the `async move`
            // block: `value_text` (e.g. `reduce_async(arr.clone(), ..)`) only
            // borrows its outer captures, but an `async move` block would move
            // every named binding it references into the returned task (E0382).
            // Binding the source future outside moves just that handle in.
            return Ok(format!(
                "{{ let smelt_source_future = {value_text}; SmeltFuture::from_future(Box::pin(async move {{ let smelt_future_value = smelt_source_future.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }})) }}"
            ));
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
            let source_item = *source_item;
            let target_item = *target_item;
            let element_text = if matches!(self.mir.types.get(source_item), Some(Type::List(_)))
                && (matches!(
                    self.mir.types.get(target_item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(target_item))
            {
                "value.into_smelt_unknown()".to_owned()
            } else {
                self.value_at_type_text("value", source_item, target_item)?
            };
            // `value_text` may be a bare `Vec` (e.g. an inlined spread/concat
            // `[...list, ...args]`) rather than a `SmeltList`. Normalize through
            // `.into()` (reflexive for `SmeltList`, `From<Vec>` otherwise) so the
            // `.id()` identity read is always available.
            return Ok(format!(
                "{{ let smelt_l: SmeltList<_> = {smelt_owned_value}.into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| {element_text}).collect::<Vec<_>>()) }}"
            ));
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
            let (prefix, base) = if Self::expression_text_is_trivial_place(value_text) {
                (String::new(), value_text.to_owned())
            } else {
                ("let smelt_tuple_src = ".to_owned(), "smelt_tuple_src".to_owned())
            };
            let items_text = source_items
                .iter()
                .zip(target_items.iter())
                .enumerate()
                .map(|(index, (source_item, target_item))| {
                    self.value_at_type_text(
                        &format!("{base}.{index}"),
                        *source_item,
                        *target_item,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return if prefix.is_empty() {
                Ok(tuple_text)
            } else {
                Ok(format!("{{ {prefix}{value_text}; {tuple_text} }}"))
            };
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && value_text == "Default::default()"
        {
            return self.default_value(target);
        }
        if let Some(slot) = self.callable_object_call_slot_text(value_text, source, target)? {
            return Ok(slot);
        }
        if let Some(adapter) =
            self.rendered_function_shape_adapter_text(value_text, source, target)?
        {
            return Ok(adapter);
        }
        if let Some(adapter) = self.structural_record_adapter_text(value_text, source, target)? {
            return Ok(adapter);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(target).is_none()
            || self.is_erased_class_type(target)
        {
            return self.erase_value_text(value_text, source);
        }
        if let (Some(Type::Class { .. }), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.structural_record_to_string_dict_adapter_text(
                value_text,
                source,
                *target_key,
                *target_value,
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
            // above by `project_union_value_text`.)
            if self.concrete_union_members(source).is_some() {
                let erased = self.erase_concrete_union_text(value_text, source);
                return self.extract_value_text(&erased, target);
            }
            return self.extract_value_text(value_text, target);
        }
        if self.is_match_fn_result_type(source)? && !self.is_match_fn_result_class_type(target)? {
            let value_ty = match self.match_fn_result_value_type(source)? {
                Some(value_ty) => value_ty,
                None => self.type_id(Type::Unknown)?,
            };
            return self.value_at_type_text(
                &format!("{value_text}.value.clone()"),
                value_ty,
                target,
            );
        }
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
            return Ok(format!("({value_text} as f64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(source) == Some(&Type::Float)
        {
            return Ok(format!("(({value_text} as f64).trunc() as i64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::String)
        {
            return Ok(format!("{value_text}.parse::<f64>().unwrap_or(0.0)"));
        }
        if self.mir.types.get(target) == Some(&Type::String)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Bool | Type::Int | Type::Float)
            )
        {
            return Ok(format!("{value_text}.to_string()"));
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) && self.mir.types.get(source) == Some(&Type::String)
        {
            return Ok(format!("SmeltRegExp::new({value_text}, String::new())"));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner))
        {
            return Ok(format!("{smelt_owned_value}.flatten()"));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            if source_inner == target_inner {
                return Ok(format!("{smelt_owned_value}"));
            }
            let mapped_value = self.value_at_type_text("value", *source_inner, *target_inner)?;
            return Ok(format!("{smelt_owned_value}.map(|value| {mapped_value})"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
        {
            let mapped_value = self.value_at_type_text("value", source, *inner)?;
            if self.optional_inner_preserves_erased_singletons(*inner) {
                return Ok(format!("Some({mapped_value})"));
            }
            return Ok(format!(
                "match {smelt_owned_value} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some({mapped_value}) }}"
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && source == *inner
        {
            return Ok(format!("Some({value_text})"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            if self.mir.types.get(source) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if self.can_coerce_to_optional_inner(source, *inner) {
                let mapped_value = self.value_at_type_text(value_text, source, *inner)?;
                return Ok(format!("Some({mapped_value})"));
            }
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source)
            && *inner == target
        {
            return Ok(format!(
                "{smelt_owned_value}.unwrap_or({})",
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source) {
            let mapped_value = self.value_at_type_text("value", *inner, target)?;
            return Ok(format!(
                "{smelt_owned_value}.map_or({}, |value| {mapped_value})",
                self.default_value(target)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_item != target_item
        {
            let item_text = self.value_at_type_text("value", *source_item, *target_item)?;
            return Ok(format!(
                "{smelt_owned_value}.into_iter().map(|value| {item_text}).collect::<SmeltList<_>>()"
            ));
        }
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
            let (prefix, base) = if Self::expression_text_is_trivial_place(value_text) {
                (String::new(), value_text.to_owned())
            } else {
                ("let smelt_tuple_src = ".to_owned(), "smelt_tuple_src".to_owned())
            };
            let items_text = source_items
                .iter()
                .enumerate()
                .map(|(index, source_item)| {
                    self.value_at_type_text(&format!("{base}.{index}"), *source_item, target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let list_text =
                format!("SmeltList::with_id(smelt_next_object_id(), vec![{items_text}])");
            return if prefix.is_empty() {
                Ok(list_text)
            } else {
                Ok(format!("{{ {prefix}{value_text}; {list_text} }}"))
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
            let item_text = self.value_at_type_text(value_text, source, *target_item)?;
            return Ok(format!("SmeltList::from(vec![{item_text}])"));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.value_at_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.to_vec(); {tuple_text} }}"
            ));
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let item_text = self.value_at_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(_, value)| {item_text}).collect::<SmeltList<_>>()"
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let int_ty = self.type_id(Type::Int)?;
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                "index.to_string()".to_owned()
            } else {
                self.value_at_type_text("index as i64", int_ty, *target_key)?
            };
            let item_text = self.value_at_type_text("value", *source_item, *target_value)?;
            let target_text = self.type_text_with_impl_trait(target, false)?;
            return Ok(format!(
                "{value_text}.into_iter().enumerate().map(|(index, value)| ({key_text}, {item_text})).collect::<{target_text}>()"
            ));
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
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                self.property_key_to_string_text("key", *source_key)?
            } else {
                self.value_at_type_text("key", *source_key, *target_key)?
            };
            let mapped_value_text =
                self.value_at_type_text("value", *source_value, *target_value)?;
            let target_text = self.type_text_with_impl_trait(target, false)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(key, value)| ({key_text}, {mapped_value_text})).collect::<{target_text}>()"
            ));
        }
        if let (Some(Type::Dict(source_key, source_value)), Some(Type::Class { .. })) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.string_dict_record_adapter_text(
                value_text,
                *source_key,
                *source_value,
                target,
            )?
        {
            return Ok(adapter);
        }
        Ok(value_text.to_owned())
    }

    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    ///
    /// The operand-shaped twin of [`Self::erase_value`]; the two must agree
    /// about every `Type`, because the same MIR value reaches one or the other
    /// depending only on whether the caller already had it rendered.
    pub(super) fn erase(&self, operand: &Operand) -> Result<String, EmitError> {
        if matches!(operand, Operand::Const(Constant::Undefined)) {
            return Ok("SmeltUnknown::Undefined".to_owned());
        }
        let text = self.operand_text(operand)?;
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = cloned_value_text(&text);
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Unknown) => Ok(text),
            // Whether a `Type::TypeParam` is a REAL Rust type parameter or is
            // itself spelled `SmeltUnknown` is one decision, taken for the whole
            // signature by `current_function_type_params`. Every seam that
            // renders such a value must ask the same question, or the emitter
            // disagrees with itself about one value: an erased `T` is already a
            // `SmeltUnknown` and erases to itself, but a MONOMORPHIZED `T` is a
            // distinct Rust type that reaches `SmeltUnknown` only through the
            // `IntoSmeltUnknown` bound its own signature declares.
            //
            // Passing the text through unconditionally is what made a callee
            // whose parameters monomorphized but whose return erased (a union
            // return mentioning `T` has no concrete Rust spelling, so
            // `rust_type` renders `SmeltUnknown`) emit `return out;` for an
            // `out: T` against a `-> SmeltUnknown` signature (E0308). The
            // rendered-value twin below has always converted here; this is the
            // same conversion, gated on the same scope the signature used.
            //
            // Telling the truth here is also what makes the PARAMETERS and the
            // RETURN one decision again rather than two. The crate-wide gate
            // already demotes any generic free function whose trial-rendered
            // body needs the erased carrier (`body_needs_erased_carrier`, whose
            // token list includes `into_smelt_unknown`); a body that erases a
            // `T` is precisely the case that rule exists for, and it never saw
            // this one because the conversion was silently dropped. With the
            // conversion emitted, such a signature demotes whole — parameters,
            // return and body land on the erased ABI together, which is also
            // what the call site independently concluded (a substituted union
            // return is not an interned MIR type, so
            // `static_call_monomorphization` demotes the site).
            //
            // This is an explicit `IntoSmeltUnknown` boundary adapter at a
            // genuine dynamic boundary — a union with a type-parameter member
            // has no concrete Rust spelling (`emitter::union`) — not a way to
            // make generated Rust type-check: it erases nothing that was not
            // already erased by the return type it is converting into, and the
            // three compat corpora emit byte-identical Rust across the change.
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name) => {
                Ok(format!("({text}).into_smelt_unknown()"))
            }
            Some(Type::TypeParam { .. }) => Ok(text),
            Some(Type::None) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({text})")),
            Some(Type::Int | Type::Float) => Ok(format!("SmeltUnknown::Number({text} as f64)")),
            Some(Type::String) => Ok(crate::rust::erased_string(&text)),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                // `From<SmeltList<SmeltUnknown>> for SmeltArray` carries the list's
                // own JS reference id AND aliases its buffer, so erasing the same
                // binding twice reuses one id (arrays compare `===` by id), an
                // erase/extract round-trip stays identity-stable, and a write made
                // through either handle is observed by the other — which is what
                // passing an array into an `unknown` slot means in JavaScript.
                Ok(format!("SmeltUnknown::Array({text}.into())"))
            }
            Some(Type::List(item)) => {
                // A `List<None>` erases its elements to `Null`, but an
                // `[undefined, …]` literal must erase to `Undefined` (the type
                // lost the distinction; only the defining constants carry it).
                // Recover it from the def-site so genuine `null` arrays keep the
                // `Null` path. This mirrors the same recovery in `value_at_type`
                // for the `List<None> -> List<Unknown>` coercion shape.
                let value_wrap = if matches!(self.mir.types.get(*item), Some(Type::None))
                    && self.container_local_all_undefined_constants(operand)?
                {
                    "SmeltUnknown::Undefined".to_owned()
                } else {
                    self.erase_value_text("value", *item)?
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
                Ok(format!(
                    "{{ let smelt_l = {text}; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>())) }}"
                ))
            }
            // A source-spelled `Map` erases through its own `SmeltJsMap`
            // `IntoSmeltUnknown` adapter, which stamps the `__smelt_map` marker
            // object (entries as `[k, v]` pair arrays + stable id). This is the
            // one place `Map` spelling diverges from `Record`: a `Record` erases
            // to a plain object, a `Map` to a marker so `isMap`/`isEqual`/
            // `[object Map]` observe it as a Map.
            Some(Type::JsMap(_, _)) => Ok(format!("{smelt_owned_text}.into_smelt_unknown()")),
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                // The object arm ALIASES the erased object's field store
                // (`smelt_shared_record`) rather than rebuilding its entries: a
                // JavaScript object is a reference value, so a write through the
                // recovered record must reach the object it came from.
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record({smelt_owned_text}))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(format!(
                    "{{ let smelt_record = {smelt_owned_text}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                ))
            }
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_text("key", *key)?;
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::new({text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))"
                ))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(format!("{smelt_owned_text}.into_smelt_unknown()"))
            }
            // A concrete match value crossing into `unknown` is erased through the
            // single explicit `IntoSmeltUnknown` adapter.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => {
                Ok(format!("{smelt_owned_text}.into_smelt_unknown()"))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(self.operand_ty(operand)?)
                    && self.symbol_name(*name)? == "Date" =>
            {
                Ok(self.date_unknown_identity_text(&text))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(self.operand_ty(operand)?) => {
                Ok(text)
            }
            Some(Type::Class { .. }) => {
                self.class_unknown_object_text(&text, self.operand_ty(operand)?)
            }
            Some(Type::Set(item)) => {
                // A `SmeltJsSet`-backed Set carries JS identity: erase through the
                // single `IntoSmeltUnknown` adapter, which stamps the `__smelt_set`
                // marker (and preserves the set's stable object id) so
                // `isSet`/`instanceof Set`/`Object.prototype.toString` recognize the
                // erased value and `SmeltFromUnknown` round-trips it. Mirrors the
                // `SmeltJsMap` erasure. A `HashSet`-backed Set (value-equality
                // primitives) has no such adapter, so it keeps the bare-array
                // projection below.
                if !self.type_is_hash_set_key_safe(*item) {
                    return Ok(format!("{smelt_owned_text}.into_smelt_unknown()"));
                }
                let value_wrap = self.erase_value_text("value", *item)?;
                // A Set erases to an array; like list bindings, the SAME source
                // Set binding erased twice must compare `===` equal (arrays
                // compare by id). `HashSet` has no `as_ptr`, so key the stable id
                // on the binding's own address (`&set`). Temps / fresh sets keep
                // `SmeltArray::new`.
                if let Some(bare_local) = self.list_local_identity_key(operand)? {
                    return Ok(format!(
                        "{{ let smelt_list_id = smelt_list_identity(&({bare_local}) as *const _ as *const () as usize); let mut values = {smelt_owned_text}.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(SmeltArray::with_id(smelt_list_id, values)) }}"
                    ));
                }
                Ok(format!(
                    "{{ let mut values = {smelt_owned_text}.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }}"
                ))
            }
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.erase_value_text(&format!("{text}.{index}.clone()"), *item)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("SmeltUnknown::Array(vec![{values}].into())"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.erase_value_text("value", *inner)?;
                Ok(format!(
                    "{smelt_owned_text}.map_or(SmeltUnknown::Undefined, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(_)) => {
                // A bare function-item-as-value wrapper has a stable item key.
                // Every reference to the same named function value lowers to its
                // own closure, so erasing each one inline would build a distinct
                // `SmeltUnknown::Function` wrapper that compares unequal under
                // JavaScript reference identity (`===`). Route the erased value
                // through a per-item accessor so all references share ONE cached
                // erased value. User arrows have no key and fall through to the
                // ordinary fresh-erasure logic below.
                if let Some((key, accessor_body)) = self.function_item_erased_accessor(operand)? {
                    self.context
                        .function_item_accessors
                        .borrow_mut()
                        .entry(key)
                        .or_insert(accessor_body);
                    return Ok(format!("__smelt_fn_value_{key}()"));
                }
                if let Some(erased_call) = self.erased_call_assignment_text(operand)? {
                    return Ok(erased_call);
                }
                if let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(operand)?)
                    && self.is_erased_unknown_rest_function(function)
                    && !function.may_throw
                {
                    let function_text = self.operand_text(operand)?;
                    return Ok(format!("{function_text}.clone().into_smelt_unknown()"));
                }
                let adapter = self
                    .rest_vector_unknown_adapter_text(operand)?
                    .unwrap_or_else(|| "::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null))".to_owned());
                Ok(format!("SmeltUnknown::Function({adapter})"))
            }
            Some(Type::Union(_)) if self.concrete_union_members(self.operand_ty(operand)?).is_some() => {
                Ok(format!("{text}.into_smelt_unknown()"))
            }
            Some(Type::Union(_)) => Ok(text),
            Some(Type::Future(item)) => {
                if let Some(bare_local) = self.future_local_identity_key(operand)? {
                    return Ok(format!(
                        "{{ let smelt_promise_id = smelt_promise_identity(&({bare_local}) as *const _ as *const () as usize); SmeltUnknown::Promise(SmeltPromise::pending_with_id(smelt_promise_id)) }}"
                    ));
                }
                self.promise_future_unknown_text(&text, *item)
            }
            // Genuine dynamic boundary: a generator crossing into source
            // `unknown` is a live resumable state machine that no concrete
            // type, generated union, or scoped generic can represent on the
            // erased side. The runtime prelude's `IntoSmeltUnknown` adapter
            // reproduces the JavaScript iterator protocol (`next` →
            // `{ value, done }` steps) over the same shared state machine.
            Some(Type::Generator { .. }) => Ok(format!("{smelt_owned_text}.into_smelt_unknown()")),
            Some(Type::GeneratorResult { .. }) => Err(EmitError::new(
                "generator results require typed done/value projection before erasure",
            )),
            Some(Type::Never) | None => Ok("SmeltUnknown::Null".to_owned()),
        }
    }

    /// Return the BARE name of a source list local being erased, if any.
    ///
    /// When a list value crosses into `SmeltUnknown::Array` straight from a
    /// `Place::Local`, the local's backing `Vec` is still alive and has a stable
    /// storage address. That address keys [`smelt_list_identity`] so every
    /// erasure of the one binding reuses a single id (arrays compare `===` by
    /// id). The returned name is the local WITHOUT the trailing `.clone()` that
    /// [`Self::operand_text`] adds, because the identity key reads `&local`
    /// (the live binding) while the erased values come from the cloned text.
    /// Non-local operands (list literals, transform temporaries) return `None`
    /// and keep the fresh-id `SmeltArray::new` path, matching JS semantics where
    /// distinct array expressions are never `===`.
    fn list_local_identity_key(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        // Only real source bindings (params / user `let`/`const`) get stable JS
        // reference identity. A temp holds a fresh intermediate array (e.g. a
        // transform/projection result like `Object.entries(..)`), which JS treats
        // as a new array each time — it keeps the fresh-id `SmeltArray::new` path.
        if matches!(self.local_decl(*local)?.kind, LocalKind::Temp) {
            return Ok(None);
        }
        // Use the SAME in-scope reference `operand_text` emits, minus the trailing
        // `.clone()` it appends — the local's *source* name (`local_name`) is not
        // always the emitted Rust variable (temps/renames), which produced
        // "cannot find value" errors. The identity key reads the live binding
        // (via `.as_ptr()`, which auto-(de)refs both `Vec` and `&Vec` locals);
        // the erased values still come from the cloned text.
        let text = self.operand_text(operand)?;
        let bare = text.strip_suffix(".clone()").unwrap_or(text.as_str());
        Ok(Some(bare.to_owned()))
    }

    /// Return the BARE name of a source future local being erased, if any.
    ///
    /// Future values cannot be cloned. When the same future local is erased
    /// more than once for identity-only operations, moving it into every
    /// `SmeltPromise::from_future` wrapper would double-move the future. Key a
    /// pending promise identity on the live local address instead; expression
    /// futures still move into an awaitable promise wrapper.
    fn future_local_identity_key(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        if matches!(self.local_decl(*local)?.kind, LocalKind::Temp)
            || !matches!(
                self.mir.types.get(self.local_decl(*local)?.ty),
                Some(Type::Future(_))
            )
        {
            return Ok(None);
        }
        let text = self.operand_text(operand)?;
        let bare = text.strip_suffix(".clone()").unwrap_or(text.as_str());
        Ok(Some(bare.to_owned()))
    }

    /// The per-entry value erasure for a dict operand crossing into `unknown`.
    ///
    /// Identical to `erase_value_text` except for the one case the type cannot
    /// answer: a `Dict<_, None>` whose defining literal held `undefined`, not
    /// `null`. `{ a: undefined }` must erase to `SmeltUnknown::Undefined` so
    /// `typeof o.a === 'undefined'` and `Object.values(o)[0] !== null` stay
    /// true across the boundary. This is the dict twin of the recovery the list
    /// arm does, sharing
    /// [`Self::container_local_all_undefined_constants`] with it.
    fn dict_erased_value_wrap(
        &self,
        operand: &Operand,
        item: TypeId,
    ) -> Result<String, EmitError> {
        if matches!(self.mir.types.get(item), Some(Type::None))
            && self.container_local_all_undefined_constants(operand)?
        {
            return Ok("SmeltUnknown::Undefined".to_owned());
        }
        self.erase_value_text("value", item)
    }

    /// Report whether a `Type::None`-holding CONTAINER operand was defined by a
    /// literal whose held values are *all* the `undefined` literal.
    ///
    /// `null` and `undefined` both collapse to MIR `Type::None`, so a
    /// `List<None>`, a `Tuple` of `None` or a `Dict<_, None>` carries no
    /// type-level hint about which JS singleton it holds; the distinction
    /// survives only as the per-value [`Constant::Undefined`] (see
    /// `specs/distinct-undefined.md`). When such a container is erased, the
    /// generic per-type erase would pick `SmeltUnknown::Null` for every held
    /// value — wrong for an `[undefined, …]` / `{ k: undefined }` literal,
    /// whose values must compare equal to the `undefined` any other producer
    /// yields (`typeof o.k === 'undefined'`, `isJSONValue(undefined) === false`).
    ///
    /// We recover the lost distinction by inspecting the operand's *defining*
    /// rvalue (mirroring [`Self::erased_call_assignment_text`]): if every held
    /// value is the `undefined` constant, the whole container erases to
    /// `Undefined`. Mixed or all-`null` literals keep the historical `Null`
    /// erasure, so genuine `null` containers are untouched, as does any operand
    /// with no single literal definition (the same conservatism the list path
    /// has always had).
    ///
    /// One helper for all three container shapes on purpose: the list arm grew
    /// this recovery first, the dict arm reproduced the bug because it did not,
    /// and a tuple arm would have been the third.
    fn container_local_all_undefined_constants(
        &self,
        operand: &Operand,
    ) -> Result<bool, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(false);
        };
        let mut defining_values: Option<Vec<&Operand>> = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, value } = statement
                    && dest == local
                {
                    // A reassignment of the same local would make the erasure
                    // ambiguous; only trust a single defining container literal.
                    match value {
                        Rvalue::List(items) | Rvalue::Tuple(items) => {
                            defining_values = Some(items.iter().collect());
                        }
                        Rvalue::Dict(entries) => {
                            defining_values =
                                Some(entries.iter().map(|(_, value)| value).collect());
                        }
                        _ => return Ok(false),
                    }
                }
            }
        }
        let Some(values) = defining_values else {
            return Ok(false);
        };
        Ok(!values.is_empty()
            && values
                .iter()
                .all(|value| matches!(value, Operand::Const(Constant::Undefined))))
    }

    /// Re-render a typed callback local from its erased callable source when it
    /// is immediately being boxed back into `SmeltUnknown`.
    ///
    /// Generic JavaScript helpers such as Remeda's purry utilities return
    /// first-class callable values through an erased `unknown` ABI. If codegen
    /// first adapts such a value to a concrete Rust callback and then wraps that
    /// adapter back into `SmeltUnknown::Function`, the adapter's static return
    /// type can erase real dynamic shapes. Reusing the original erased call
    /// preserves the runtime callable and its result.
    fn erased_call_assignment_text(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        // Re-inlining the defining call here is only sound when its typed-callback
        // binding is *also* suppressed (see
        // `emit_call_terminator_statement`/`emit_statement`). Otherwise the binding
        // is emitted *and* the call is re-rendered at this erase site, evaluating
        // the call twice and double-moving its arguments (E0382). Pair the two
        // decisions: when the binding survives, fall through so the erase reads the
        // existing binding instead of re-inlining.
        if !self.function_call_result_dead_when_erased(*local)? {
            return Ok(None);
        }
        let mut found = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest != local {
                    continue;
                }
                let Rvalue::ClosureCall { .. } = value else {
                    return Ok(None);
                };
                found = Some(value);
            }
        }
        let Some(value) = found else {
            for block in &self.function.blocks {
                if let Some(Terminator::Call {
                    callee, args, dest, ..
                }) = &block.terminator
                    && dest == local
                {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    return Ok(Some(self.call_text_for_dest(callee, args, unknown_ty)?));
                }
            }
            return Ok(None);
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        Ok(Some(self.rvalue_text_for_dest(value, unknown_ty)?))
    }

    /// Wrap a rendered value expression with a known static type into `SmeltUnknown`.
    ///
    /// Transitional wrapper: callers that still thread bare `(text, type)` reach
    /// the [`RenderedValue`]-based [`FunctionEmitter::erase_value`] through here
    /// by treating the text as a primary/postfix [`Precedence::Atom`]. New
    /// callers should build a [`RenderedValue`] with the right precedence and
    /// call [`FunctionEmitter::erase_value`] directly so erase can parenthesize
    /// loose operands itself.
    pub(super) fn erase_value_text(
        &self,
        value_text: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        self.erase_value(&RenderedValue::atom(value_text, ty))
    }

    /// Box a typed [`RenderedValue`] into the erased `SmeltUnknown` runtime form.
    ///
    /// This is the real per-`Type` mechanics behind the `erase` verb. Unlike the
    /// old text-only entry, the rendered value knows its own precedence, so erase
    /// wraps the operand itself wherever it inlines the value as a cast operand
    /// (`… as f64`) or a method receiver (`.into()`, `.into_iter()`, `.clone()`,
    /// `.into_smelt_unknown()`). For an [`Precedence::Atom`] those wraps are
    /// no-ops, so output stays byte-identical to the historical inlining.
    pub(super) fn erase_value(&self, value: &RenderedValue) -> Result<String, EmitError> {
        let ty = value.ty();
        let value_text = value.text();
        // `value_text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_value = cloned_value_text(value_text);
        match self.mir.types.get(ty) {
            Some(Type::Unknown) => Ok(value_text.to_owned()),
            Some(Type::TypeParam { .. }) if value_text == "Default::default()" => {
                Ok("SmeltUnknown::Null".to_owned())
            }
            // Unconditional, and correct for both spellings of a type
            // parameter: a monomorphized `T` converts through the
            // `IntoSmeltUnknown` bound its signature declares, and an erased one
            // is a `SmeltUnknown` whose `IntoSmeltUnknown` impl is the identity.
            // The operand twin (`erase`) has to gate the same conversion on
            // scope because it must not perturb the erased spelling's bytes.
            Some(Type::TypeParam { .. }) => Ok(format!("({value_text}).into_smelt_unknown()")),
            Some(Type::None | Type::Never) | None => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({value_text})")),
            Some(Type::Int | Type::Float)
                if value_text == "Default::default()" || value_text == "(Default::default())" =>
            {
                Ok("SmeltUnknown::Number(0.0)".to_owned())
            }
            Some(Type::Int | Type::Float) => {
                // Cast operand: a loose top-level operator must be parenthesized
                // so `as f64` does not reassociate across it.
                Ok(format!(
                    "SmeltUnknown::Number({} as f64)",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::String) => Ok(crate::rust::erased_string(value_text)),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                // Method receiver: wrap a loose operand before `.into()`.
                Ok(format!(
                    "SmeltUnknown::Array({}.into())",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::List(item)) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                // Carry the list's own JS reference id across erasure (so an
                // erase/extract round-trip stays identity-stable, e.g. the array
                // a forEach/reduce callback receives `===` the input array).
                //
                // `Into<Vec<_>>` rather than `into_iter()`, for the same reason as the
                // operand-based erasure above: the value can be a `&SmeltList<_>` (a
                // by-reference callback parameter), whose `into_iter()` yields `&T`.
                Ok(format!(
                    "{{ let smelt_l = {}; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>())) }}",
                    value.parenthesized_if_needed()
                ))
            }
            // A source-spelled `Map` erases through its `SmeltJsMap`
            // `IntoSmeltUnknown` adapter, stamping the `__smelt_map` marker so
            // the erased value stays observable as a Map (see the operand-based
            // erasure above).
            Some(Type::JsMap(_, _)) => {
                Ok(format!("{smelt_owned_value}.into_smelt_unknown()"))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record({smelt_owned_value}))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                if self.mir.types.get(*item) == Some(&Type::Float) {
                    return Ok(format!(
                        "{{ let smelt_record = {smelt_owned_value}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                    ));
                }
                Ok(format!(
                    "{{ let smelt_record = {smelt_owned_value}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                ))
            }
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_text("key", *key)?;
                let value_wrap = self.erase_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::new({}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(format!("{smelt_owned_value}.into_smelt_unknown()"))
            }
            // A concrete match value crossing into a dynamic `unknown` boundary
            // is erased through the single explicit `IntoSmeltUnknown` adapter,
            // reproducing the JavaScript match-array-with-properties shape.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => {
                Ok(format!("{smelt_owned_value}.into_smelt_unknown()"))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(ty) && self.symbol_name(*name)? == "Date" =>
            {
                Ok(self.date_unknown_identity_text(value_text))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(ty) => Ok(value_text.to_owned()),
            Some(Type::Class { .. }) => self.class_unknown_object_text(value_text, ty),
            Some(Type::Set(item)) => {
                // See the sibling `Type::Set` arm above: a `SmeltJsSet`-backed Set
                // erases through the `__smelt_set` marker adapter; only a
                // `HashSet`-backed primitive Set falls through to a bare array.
                if !self.type_is_hash_set_key_safe(*item) {
                    return Ok(format!(
                        "({}).clone().into_smelt_unknown()",
                        value.parenthesized_if_needed()
                    ));
                }
                let value_wrap = self.erase_value_text("value", *item)?;
                Ok(format!(
                    "{{ let mut values = {}.clone().into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }}",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.erase_value_text(
                            &format!("{}.{index}.clone()", value.parenthesized_if_needed()),
                            *item,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("SmeltUnknown::Array(vec![{values}].into())"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.erase_value_text("value", *inner)?;
                Ok(format!(
                    "{}.clone().map_or(SmeltUnknown::Undefined, |value| {value_wrap})",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::Function(function)) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok(format!(
                        "{}.clone().into_smelt_unknown()",
                        value.parenthesized_if_needed()
                    ));
                }
                let args = self.function_args_from_smelt_args_text(function, ErasedCallTargetAbi::Declared)?;
                let call_text = format!("(smelt_function_value)({args})");
                let return_text = if self.mir.types.get(function.return_ty) == Some(&Type::None) {
                    // A `void`-returning callback erased to a callable value
                    // returns JavaScript `undefined`, not `null`, so `!== undefined`
                    // guards on the result behave as in JS.
                    if function.may_throw {
                        format!(
                            "{{ {call_text}?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Undefined) }}"
                        )
                    } else {
                        format!(
                            "{{ {call_text}; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Undefined) }}"
                        )
                    }
                } else if matches!(self.mir.types.get(function.return_ty), Some(Type::Future(_))) {
                    // A throwing async callback's call yields `Result<Future, _>`,
                    // so the fallible call must be unwrapped with `?` to recover the
                    // bare future before it is erased into a promise. Erasing the
                    // `Result` directly would double-wrap the future (the promise
                    // task then awaits a `Result<Future, _>` instead of a future).
                    let future_call = if function.may_throw {
                        format!("{call_text}?")
                    } else {
                        call_text
                    };
                    let erased_return = self.erase_value_text(&future_call, function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({erased_return})")
                } else if self.class_has_no_known_fields(function.return_ty) {
                    if function.may_throw {
                        call_text
                    } else {
                        format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({call_text})")
                    }
                } else if function.may_throw {
                    let erased_return =
                        self.erase_value_text(&format!("{call_text}?"), function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({erased_return})")
                } else {
                    let erased_return = self.erase_value_text(&call_text, function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({erased_return})")
                };
                Ok(format!(
                    "{{ let smelt_function_value = {value_text}; if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) {{ smelt_callable_object }} else {{ let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); SmeltUnknown::Function(smelt_erased_function) }} }}"
                ))
            }
            Some(Type::Union(_)) if self.concrete_union_members(ty).is_some() => {
                Ok(format!("{value_text}.into_smelt_unknown()"))
            }
            Some(Type::Union(_)) => Ok(value_text.to_owned()),
            Some(Type::Future(item)) => self.promise_future_unknown_text(value_text, *item),
            // Same dynamic boundary as the operand-based erase above: route the
            // generator through the prelude's `IntoSmeltUnknown` iterator
            // adapter rather than failing emission.
            Some(Type::Generator { .. }) => Ok(format!(
                "{}.clone().into_smelt_unknown()",
                value.parenthesized_if_needed()
            )),
            Some(Type::GeneratorResult { .. }) => Err(EmitError::new(
                "generator results require typed done/value projection before erasure",
            )),
        }
    }

    /// The erased `null` tag of `SmeltUnknown`.
    ///
    /// This is the canonical boxed *value* `null`: a `Type::None` value the
    /// source spelled, a `None` return, the default of an erased target. It is
    /// NOT the answer for a property that does not exist — JavaScript
    /// distinguishes the two, and `===`/`Object.is` see the difference, so an
    /// absent property asks for [`Self::absent_value_text`] instead. Keeping
    /// the variant text here means callers outside the seam never spell
    /// `SmeltUnknown::Null` by hand; they ask for the null tag by intent.
    pub(super) fn null_value_text(&self) -> String {
        "SmeltUnknown::Null".to_owned()
    }

    /// The erased tag for a property that is ABSENT.
    ///
    /// Reading a property an object does not have evaluates to `undefined` in
    /// JavaScript, never to `null`. The two used to share
    /// [`Self::null_value_text`], which made a miss on a class receiver
    /// (`instance['nope']`) compare `=== null` and `!== undefined` — both
    /// backwards. Absence and the `null` value are different values, so they
    /// get different helpers and callers state which one they mean.
    pub(super) fn absent_value_text(&self) -> String {
        "SmeltUnknown::Undefined".to_owned()
    }

    /// Wrap a live concrete future in an erased, cloneable promise handle.
    ///
    /// The source future remains the only owner of the async computation; the
    /// `SmeltPromise` stores it behind shared state so erased identity and later
    /// awaits observe the same resolved value. The concrete future expression is
    /// evaluated before the `async move` block so callback captures owned by an
    /// outer `Fn` closure are not moved into the returned promise task.
    pub(super) fn promise_future_unknown_text(
        &self,
        future_text: &str,
        item_ty: TypeId,
    ) -> Result<String, EmitError> {
        let value_text = self.erase_value_text("smelt_value", item_ty)?;
        Ok(format!(
            "{{ let smelt_future = {future_text}; SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move {{ let smelt_value = smelt_future.await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value_text}) }}))) }}"
        ))
    }

    /// Box an iterator of already-rendered `String` values into a `SmeltUnknown`
    /// string array, owning the per-element `SmeltUnknown::String` construction.
    ///
    /// `value_iter_text` must evaluate to something iterable yielding owned
    /// `String`s (e.g. a `Vec<String>` or a chained iterator). Callers that have
    /// produced a list of strings and need the erased array form route through
    /// here instead of writing the `SmeltUnknown::Array(..)` / `SmeltUnknown::String`
    /// variants themselves.
    pub(super) fn erase_string_array_text(&self, value_iter_text: &str) -> String {
        format!(
            "SmeltUnknown::Array({value_iter_text}.into_iter().map(|value| SmeltUnknown::String(value.into())).collect())"
        )
    }

    /// Box an already-`f64` numeric expression into a `SmeltUnknown` number,
    /// owning the `SmeltUnknown::Number` construction.
    ///
    /// Unlike [`erase_value_text`](Self::erase_value_text) for `Int`/`Float`,
    /// this assumes `value_text` already evaluates to an `f64` and so adds no
    /// `as f64` cast. Use it where the numeric value was already coerced to
    /// `f64` before crossing the seam.
    pub(super) fn erase_f64_text(&self, value_text: &str) -> String {
        format!("SmeltUnknown::Number({value_text})")
    }

    /// Box a fixed list of already-erased element expressions into a
    /// `SmeltUnknown` array.
    ///
    /// `elements_text` is the comma-joined Rust text of elements that have each
    /// already crossed the seam (e.g. via [`erase`](Self::erase)). This owns the
    /// outer `SmeltUnknown::Array(vec![..].into())` construction so literal-array
    /// erase paths need not spell the variant.
    pub(super) fn erase_array_text(&self, elements_text: &str) -> String {
        format!("SmeltUnknown::Array(vec![{elements_text}].into())")
    }

    /// Box a fixed set of already-erased `(key, value)` entry expressions into a
    /// `SmeltUnknown` object.
    ///
    /// `entries_text` is the comma-joined Rust text of `(String, SmeltUnknown)`
    /// tuples whose values have already crossed the seam. This owns the outer
    /// `SmeltUnknown::Object(SmeltObject::new(..))` construction so literal-object
    /// erase paths need not spell the variant.
    pub(super) fn erase_object_text(&self, entries_text: &str) -> String {
        format!(
            "SmeltUnknown::Object(SmeltObject::new(Vec::from([{entries_text}])))"
        )
    }

    /// Mark a timestamp-backed `Date` only when it crosses an erased value boundary.
    ///
    /// Internally dates stay as numeric timestamps for compact date arithmetic. The
    /// marker preserves JavaScript object identity for later dynamic `instanceof Date`
    /// checks without changing ordinary typed Date storage or comparisons.
    fn date_unknown_identity_text(&self, value_text: &str) -> String {
        // `value_text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_value = cloned_value_text(value_text);
        format!(
            "match {smelt_owned_value} {{ SmeltUnknown::Object(value) if value.contains_key(\"__smelt_date\") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(value))]))), value => value }}"
        )
    }

    /// Wrap a generated class or interface value into an erased object.
    ///
    /// TypeScript structural objects often reach erased helper surfaces through
    /// callbacks. Preserving their known fields keeps those values observable
    /// after the type is widened to `unknown` instead of silently replacing the
    /// object with an empty map.
    fn class_unknown_object_text(
        &self,
        value_text: &str,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if self.record_conversion_stack.borrow().contains(&target) {
            return Ok("SmeltUnknown::Null".to_owned());
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(target) else {
            return Ok("SmeltUnknown::Null".to_owned());
        };
        self.record_conversion_stack.borrow_mut().push(target);
        let fields = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == *name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
            .or_else(|| {
                self.mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                    .map(|interface| {
                        crate::classes::effective_interface_fields(self.mir, interface)
                    })
            })
            .unwrap_or_default();

        // A reference class stores its fields inside the shared `Rc<RefCell<Inner>>`
        // cell, so erasing one to a plain object must read each field through
        // `.0.borrow()` rather than a direct named-field access against the
        // newtype (was E0609).
        let field_base = if self.is_reference_class_type(target) {
            "smelt_object_value.0.borrow()"
        } else {
            "smelt_object_value"
        };
        let entries_result = fields
            .iter()
            .filter(|field| !matches!(field.visibility, smelt_hir::Visibility::Private))
            .map(|field| {
                let source_name = self.symbol_source_name(field.name)?;
                let field_name = sanitize_ident(self.symbol_name(field.name)?);
                if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                    let field_value = self.erase_value_text("value", *inner)?;
                    return Ok(format!(
                        "if let Some(value) = {field_base}.{field_name}.clone() {{ smelt_object_entries.push(({source_name:?}.to_owned(), {field_value})); }}"
                    ));
                }
                let field_value = if let Some(value) =
                    self.virtual_method_storage_field_text(target, target, field.name)?
                {
                    self.erase_value_text(&value, field.ty)?
                } else if self.is_reference_class_type(target) {
                    // Reading through `.0.borrow()` yields a `Ref` guard; the value
                    // must be cloned out rather than moved (was E0507).
                    self.erase_value_text(
                        &format!("{field_base}.{field_name}.clone()"),
                        field.ty,
                    )?
                } else {
                    self.erase_value_text(
                        &format!("{field_base}.{field_name}"),
                        field.ty,
                    )?
                };
                Ok(format!(
                    "smelt_object_entries.push(({source_name:?}.to_owned(), {field_value}));"
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>();
        self.record_conversion_stack.borrow_mut().pop();
        let entries = entries_result?.join(" ");

        // Inject a hidden provenance marker so an erased class instance is no longer
        // indistinguishable from a plain object of the same shape. The marker is
        // VISIBLE to structural equality (so a class instance is not `==` to an
        // equal-shape plain object) but INVISIBLE to key enumeration / JSON via the
        // `__smelt_class` filters, and drives the `"__smelt_proto:class"` sentinel in
        // `smelt_prototype_sentinel`. See `blocker-logs/plan-class-prototype-2026-06-23.md`.
        // A user class whose base chain reaches a modeled host object (e.g.
        // `class File extends Blob`) IS an instance of that host in JavaScript, so
        // its erased record carries the host base's identity marker(s). This keeps
        // `value instanceof Blob` (a marker check on the erased value) honest for
        // host subclasses — including override classes assigned into a
        // `globalThis.<Name>` slot — without any globalThis special-casing.
        let mut host_markers = String::new();
        for (marker, value_text) in self.host_base_markers(*name) {
            use std::fmt::Write as _;
            // `marker` is a fixed `__smelt_*` identifier, so wrap it in explicit
            // quotes rather than Debug-formatting it into a Rust string literal.
            let _ = write!(
                host_markers,
                "smelt_object_entries.push((\"{marker}\".to_owned(), {value_text})); "
            );
        }
        // The marker VALUE is the class name, not a bare `true`. JavaScript exposes
        // `instance.constructor`, and es-toolkit `isEqualWith` gates instance
        // comparison on `areObjectsEqual(a.constructor, b.constructor)` — with no
        // name recorded, two instances of DIFFERENT classes both answered
        // `undefined` for `.constructor`, `Object.is(undefined, undefined)` held,
        // and they compared equal. `smelt_get_object_field` reads the name back to
        // intern one constructor value per class.
        //
        // An INTERFACE-backed record is not a class instance: in JavaScript it is
        // an ordinary object literal, with no constructor and no prototype to
        // report. Stamping it would make `{ a: 1 }` typed as `{ a: number }`
        // unequal to the same plain object and give it a bogus `.constructor`.
        // The interface's own generated `IntoSmeltUnknown` (see
        // `emit_record_into_smelt_unknown_impl`) already omits the marker; this
        // inline adapter must agree, or the same value erases two different ways
        // depending on which path the emitter took.
        let class_marker = if self.is_interface_record_type(target) {
            String::new()
        } else {
            let class_name = self.symbol_source_name(*name)?;
            format!(
                "smelt_object_entries.push((\"__smelt_class\".to_owned(), SmeltUnknown::String({class_name:?}.into()))); "
            )
        };
        // A reference record's JS identity is its shared cell, so every erasure
        // of any handle on that cell must produce the same object id — otherwise
        // `const b = a; erase(a) === erase(b)` (and `expect(x).toBe(obj)` after a
        // mutation) is false for what is one object. A by-value record has no
        // identity to preserve and keeps minting a fresh id per erasure.
        let object_ctor = if self.is_reference_class_type(target) {
            "SmeltObject::with_id(smelt_reference_object_identity(::std::rc::Rc::as_ptr(&smelt_struct_value.0) as usize), smelt_object_entries)"
        } else {
            "SmeltObject::new(smelt_object_entries)"
        };
        // A class instance keeps its methods across the erasure seam, as
        // prototype-carried members (`__smelt_proto:<name>`). Without them an
        // erased instance answered `undefined` for every method read and the
        // erased call site substituted a fabricated default. Interfaces have no
        // method bodies to bind, so they contribute nothing here — which is why
        // this consults `mir.classes` rather than the record type.
        let proto_entries = match self
            .mir
            .classes
            .iter()
            .find(|class| class.name == *name)
        {
            Some(class)
                if crate::class_proto::class_has_proto_entries(self.mir, self.context, class) =>
            {
                format!(
                    "smelt_object_entries.extend(smelt_struct_value.{method}()); ",
                    method = crate::class_proto::PROTO_ENTRIES_METHOD,
                )
            }
            _ => String::new(),
        };
        Ok(format!(
            "{{ let smelt_object_value = {value_text}; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::new(); {entries} {host_markers}{class_marker}{proto_entries}SmeltUnknown::Object({object_ctor}) }}"
        ))
    }

    /// Identity markers a class carries because its base chain reaches a modeled
    /// host object or a builtin `Error`.
    ///
    /// Walks the single-inheritance base chain through `mir.classes`; the first
    /// base that names a registered host object (`smelt_stdlib::host_object_by_class`)
    /// contributes its marker. `File` additionally contributes `Blob`'s marker,
    /// matching the host subtype relationship the native `new File(...)` records
    /// stamp. Returns an empty vector for a class with no host base (the common
    /// case, which keeps existing erased output byte-identical).
    ///
    /// A builtin `Error` class is not a `HOST_OBJECTS` entry — errors are modeled
    /// by the `__smelt_error: "<ClassName>"` convention instead — so a class whose
    /// chain reaches one contributes that marker with the NEAREST BUILTIN base's
    /// name as its value. That is what makes an erased `class CustomError extends
    /// Error` instance answer `instanceof Error` (the marker-presence probe) while
    /// `class MyTypeError extends TypeError` also satisfies `instanceof TypeError`
    /// (the recorded-name equality arm) and not `instanceof RangeError`; the user
    /// class itself keeps resolving through `__smelt_class`. `instance_of_text`
    /// answers those probes off the same `smelt_stdlib::is_error_class_name` list,
    /// so neither side carries a hand-maintained copy of the error hierarchy.
    ///
    /// Each entry is the marker key plus the Rust text of its VALUE: an identity
    /// marker is a bare `true`, while the error marker records which builtin
    /// error class the chain reached.
    fn host_base_markers(&self, class_name: Symbol) -> Vec<(&'static str, String)> {
        let mut markers: Vec<(&'static str, String)> = Vec::new();
        let mut current = Some(class_name);
        for _ in 0u32..64 {
            let Some(name_sym) = current else { break };
            let Some(class) = self.mir.classes.iter().find(|class| class.name == name_sym) else {
                break;
            };
            let Some(base) = class.base else { break };
            let Some(base_name) = self.mir.symbols.get(base) else { break };
            if let Some(entry) = smelt_stdlib::host_object_by_class(base_name) {
                markers.push((entry.marker, "SmeltUnknown::Bool(true)".to_owned()));
                if base_name == "File"
                    && let Some(blob) = smelt_stdlib::host_object_marker("Blob")
                {
                    markers.push((blob, "SmeltUnknown::Bool(true)".to_owned()));
                }
                break;
            }
            if smelt_stdlib::is_error_class_name(base_name) {
                markers.push((
                    "__smelt_error",
                    format!("SmeltUnknown::String({base_name:?}.into())"),
                ));
                break;
            }
            current = Some(base);
        }
        markers
    }

    /// Emits a runtime tag check for `SmeltUnknown`.
    /// Emits a runtime tag check for `SmeltUnknown`.
    pub(super) fn tag_check(
        &self,
        value: &Operand,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        let value_ty = self.operand_ty(value)?;
        // A statically-`None` operand is an absent/`undefined` value (for
        // example a reference to an ambient host global that Smelt's non-DOM
        // profile does not model, such as `window`/`process`). It renders as the
        // Rust unit `()`, which cannot be matched against a `SmeltUnknown` tag
        // pattern (E0308). Fold the tag check to a compile-time constant with JS
        // semantics: an `undefined` value is nullish (`== null`) and `typeof`
        // `"undefined"`, but is not a boolean/number/string/object/etc.
        if matches!(self.mir.types.get(value_ty), Some(Type::None)) {
            let is_nullish = matches!(
                kind,
                smelt_hir::UnknownKind::Null | smelt_hir::UnknownKind::Undefined
            );
            return Ok(if is_nullish { "true" } else { "false" }.to_owned());
        }
        if let Some(check) = self.concrete_union_tag_check(&text, value_ty, kind) {
            return Ok(check);
        }
        if let Some(&Type::Optional(inner)) = self.mir.types.get(value_ty) {
            if kind == smelt_hir::UnknownKind::Null {
                return Ok(format!("{text}.is_none()"));
            }
            // A concrete-union `Option` payload is a tagged enum, so the present
            // value is narrowed against its `SmeltUnion…` variants rather than
            // erased `SmeltUnknown` tags.
            let check = match self.concrete_union_tag_check("smelt_value", inner, kind) {
                Some(check) => check,
                None => self.tag_check_raw("smelt_value", kind)?,
            };
            return Ok(format!(
                "{text}.as_ref().is_some_and(|smelt_value| {check})"
            ));
        }
        self.tag_check_raw(&text, kind)
    }

    /// Emits the JavaScript `typeof` string for a runtime-erased value.
    pub(super) fn typeof_value_text(&self, value: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = cloned_value_text(&text);
        let value_ty = self.operand_ty(value)?;
        if let Some(members) = self.concrete_union_members(value_ty) {
            let name = union::union_name(value_ty);
            let arms = members
                .iter()
                .enumerate()
                .map(|(index, member)| {
                    let kind = self.typeof_static_type_text(*member);
                    format!("{name}::M{index}(_) => {kind:?}.to_owned()")
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(format!("match {text} {{ {arms} }}"));
        }
        Ok(format!(
            "match {smelt_owned_text} {{ SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(_) => \"boolean\".to_owned(), SmeltUnknown::Number(_) => \"number\".to_owned(), SmeltUnknown::String(_) => \"string\".to_owned(), SmeltUnknown::Symbol(_) => \"symbol\".to_owned(), SmeltUnknown::Function(_) => \"function\".to_owned(), SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Promise(_) => \"object\".to_owned() }}"
        ))
    }

    /// Return the JavaScript `typeof` spelling for one concrete union member.
    fn typeof_static_type_text(&self, ty: TypeId) -> &'static str {
        match self.mir.types.get(ty) {
            Some(Type::None | Type::Optional(_)) => "undefined",
            Some(Type::Bool) => "boolean",
            Some(Type::Int | Type::Float) => "number",
            Some(Type::String) => "string",
            Some(Type::Function(_)) => "function",
            Some(
                Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Future(_),
            ) => "object",
            _ => "undefined",
        }
    }

    /// Emits the opaque `Object.getPrototypeOf` sentinel for an erased value.
    ///
    /// Defers all prototype discrimination to the `smelt_prototype_sentinel`
    /// runtime helper so the array/null/plain-object/class branches stay in one
    /// place. Class instances carry a hidden `__smelt_class` marker and map to a
    /// distinct sentinel, while non-class values keep their existing sentinels.
    /// The operand is rendered through the erased-`unknown` coercion seam so a
    /// source spelling that narrows the receiver first (e.g. an
    /// `Optional(unknown)` from `as typeof Object.prototype | null`) still
    /// hands the helper a plain `SmeltUnknown`.
    /// With `own_slot_shadows` set the emission goes through
    /// `smelt_proto_accessor` instead, which answers an own `__proto__` slot
    /// before falling back to the sentinel. That is the `v.__proto__` accessor
    /// rather than `Object.getPrototypeOf(v)`; see
    /// [`smelt_hir::ExprKind::PrototypeSentinel`] for why the two differ.
    pub(super) fn prototype_sentinel_text(
        &self,
        value: &Operand,
        own_slot_shadows: bool,
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        let helper = if own_slot_shadows {
            "smelt_proto_accessor"
        } else {
            "smelt_prototype_sentinel"
        };
        Ok(format!("{helper}(&({text}))"))
    }

    /// Emits `Object(value)` as a boxed primitive.
    ///
    /// Defers to the `smelt_box_value` runtime helper, which wraps a primitive in
    /// the same marker shape `new Number(..)` / `new Boolean(..)` /
    /// `new String(..)` build and passes objects through unchanged. The branch is
    /// necessarily a runtime one — a `Type::Unknown` argument's tag is only known
    /// then — so the operand goes through the erased-`unknown` coercion seam.
    pub(super) fn box_primitive_text(&self, value: &Operand) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(format!("smelt_box_value({text})"))
    }

    /// Emits `Object.create(proto)` as a fresh erased object.
    ///
    /// Defers to the `smelt_object_from_prototype` runtime helper so the
    /// null-prototype, opaque-`__smelt_proto:*`-sentinel and concrete-prototype
    /// branches stay in one place. The operand goes through the erased-`unknown`
    /// coercion seam because a prototype is typically the `SmeltUnknown` returned
    /// by `smelt_prototype_sentinel`.
    pub(super) fn object_from_prototype_text(
        &self,
        prototype: &Operand,
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(prototype, unknown_ty)?;
        Ok(format!("smelt_object_from_prototype({text})"))
    }

    /// Emits `Object.defineProperty` / `Object.defineProperties` as a property
    /// installation on an erased object.
    ///
    /// Both operands cross the erased-`unknown` seam because a descriptor table
    /// is an ordinary JavaScript object whose values have no common static
    /// shape (a data descriptor holds `value`, an accessor descriptor holds
    /// `get`). The runtime helper returns the target, which is what the two
    /// `Object` statics answer.
    pub(super) fn define_properties_text(
        &self,
        target: &Operand,
        descriptors: &Operand,
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let target_text = self.value_at_type(target, unknown_ty)?;
        let descriptors_text = self.value_at_type(descriptors, unknown_ty)?;
        Ok(format!(
            "smelt_define_properties({target_text}, {descriptors_text})"
        ))
    }

    /// Emits the JavaScript `Object.prototype.toString.call(x)` tag probe.
    ///
    /// Defers the tag resolution to the `smelt_object_to_string_tag` runtime
    /// helper so the primitive-variant and host-identity-marker branches stay
    /// in one place. The operand is rendered through the erased-`unknown`
    /// coercion seam because the probe is only lowered for erased values.
    pub(super) fn object_to_string_tag_text(&self, value: &Operand) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(format!("smelt_object_to_string_tag(&({text}))"))
    }

    /// Emits `structuredClone(x)` as a fresh-identity deep copy.
    ///
    /// Defers to the `smelt_structured_clone` runtime helper, which rebuilds the
    /// object graph with new identities while preserving host markers. Only
    /// lowered for erased (`unknown`) values, so the operand and result flow
    /// through the erased-`unknown` coercion seam.
    pub(super) fn structured_clone_text(&self, value: &Operand) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(format!("smelt_structured_clone({text})"))
    }

    /// Emits a runtime tag check for already-rendered `SmeltUnknown` text.
    pub(super) fn tag_check_raw(
        &self,
        text: &str,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let pattern = match kind {
            smelt_hir::UnknownKind::Null => "SmeltUnknown::Null",
            smelt_hir::UnknownKind::Undefined => "SmeltUnknown::Undefined",
            smelt_hir::UnknownKind::Bool => "SmeltUnknown::Bool(_)",
            smelt_hir::UnknownKind::Number => "SmeltUnknown::Number(_)",
            smelt_hir::UnknownKind::String => "SmeltUnknown::String(_)",
            smelt_hir::UnknownKind::Symbol => "SmeltUnknown::Symbol(_)",
            smelt_hir::UnknownKind::Function => {
                return Ok(format!("matches!({text}, SmeltUnknown::Function(_))"));
            }
            smelt_hir::UnknownKind::Array => "SmeltUnknown::Array(_)",
            smelt_hir::UnknownKind::Object => {
                // `typeof x === "object"` is true for plain objects, arrays,
                // `null`, and built-in object wrappers such as promises (whose
                // own representation is the dedicated `Promise` variant).
                return Ok(format!(
                    "matches!({text}, SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Null | SmeltUnknown::Promise(_))"
                ));
            }
            smelt_hir::UnknownKind::Promise => {
                return Ok(format!("matches!({text}, SmeltUnknown::Promise(_))"));
            }
        };
        Ok(format!("matches!({text}, {pattern})"))
    }

    /// Emits extraction from `SmeltUnknown` into a concrete Rust type.
    ///
    /// JavaScript and Python code often narrows dynamic values through guards
    /// the frontend cannot fully preserve after generic or regex surfaces erase
    /// the shape. Keep primitive extraction total where the source language has
    /// a defined coercion/default instead of turning those paths into generated
    /// Rust panics.
    pub(super) fn extract(&self, value: &Operand, target: TypeId) -> Result<String, EmitError> {
        // The primitive-target arms of `extract_value_text` (`Bool`/`Int`/
        // `Float`/`String`) match directly on a `SmeltUnknown` scrutinee. A
        // caller that reaches here with an already-concrete `Option` source whose
        // inner type is itself concrete — e.g. an `Option<String>` array element
        // coerced to a `String` method receiver — must instead go through the
        // general concrete coercion, which unwraps the `Option`. (List/Dict and
        // other targets normalize their source through `into_smelt_unknown`
        // first, so they still accept a non-erased source.) `value_at_type`
        // handles a non-erased `Option` source directly without delegating back
        // to `extract`, so this cannot recurse.
        let source_ty = self.operand_ty(value)?;
        let target_is_primitive = matches!(
            self.mir.types.get(target),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        );
        let source_inner_is_erased = matches!(
            self.mir.types.get(source_ty),
            Some(Type::Optional(inner)) if matches!(
                self.mir.types.get(*inner),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(*inner)
        );
        if target_is_primitive
            && matches!(self.mir.types.get(source_ty), Some(Type::Optional(_)))
            && !source_inner_is_erased
        {
            return self.value_at_type(value, target);
        }
        // A bare function value (`Rc<dyn Fn ...>`) crossing into a record/list
        // extraction cannot be fed straight to the `into_smelt_unknown()` arms:
        // the erased-function Rust type does not implement `IntoSmeltUnknown`.
        // Wrap it into a `SmeltUnknown::Function` at the boundary first, then
        // extract from that erased value (was E0599 in `isMatchWith`/`toolkit`).
        if matches!(self.mir.types.get(source_ty), Some(Type::Function(_))) {
            let erased = self.erase(value)?;
            return self.extract_value_text(&erased, target);
        }
        let text = self.operand_text(value)?;
        self.extract_value_text(&text, target)
    }

    /// Emits checked extraction from an already-rendered `SmeltUnknown` value.
    pub(super) fn extract_value_text(
        &self,
        text: &str,
        target: TypeId,
    ) -> Result<String, EmitError> {
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = cloned_value_text(text);
        if text == "Default::default()" {
            return match self.mir.types.get(target) {
                Some(Type::None) => Ok("()".to_owned()),
                Some(Type::Bool) => Ok("false".to_owned()),
                Some(Type::Float) => Ok("0.0".to_owned()),
                Some(Type::Int) => Ok("0_i64".to_owned()),
                Some(Type::String) => Ok("String::new()".to_owned()),
                Some(Type::List(_)) => Ok("Vec::new()".to_owned()),
                Some(Type::Dict(key, _)) if self.dict_uses_smelt_record(*key) => {
                    Ok("SmeltRecord::new()".to_owned())
                }
                Some(Type::Dict(key, _)) if self.dict_uses_js_key_map(*key) => {
                    Ok("SmeltJsMap::new()".to_owned())
                }
                Some(Type::Dict(_, _)) => Ok("::std::collections::HashMap::new()".to_owned()),
                Some(Type::Optional(_)) => Ok("None".to_owned()),
                _ => self.default_value(target),
            };
        }
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text.to_owned()),
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "Date" => {
                Ok(text.to_owned())
            }
            Some(Type::List(_)) if text.contains(".concat(") => Ok(text.to_owned()),
            // Extracting an erased value into the unit type means the source
            // language discards it (a `=> void` callback ignores whatever it
            // returns; JS does not assert the value is null). Drop it instead of
            // panicking — the old assert turned e.g. `tap(identity)` and a
            // `vi.fn<(x) => void>` transformer into runtime panics.
            Some(Type::None) => Ok(format!("{{ let _ = {smelt_owned_text}; () }}")),
            Some(Type::Bool) => Ok(format!(
                "match {smelt_owned_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}"
            )),
            Some(Type::Float) => Ok(format!(
                "match {smelt_owned_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
            )),
            Some(Type::Int) => Ok(format!(
                "match {smelt_owned_text} {{ SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value {{ 1_i64 }} else {{ 0_i64 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => 0_i64 }}"
            )),
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
            Some(Type::String) => Ok(format!(
                "match {smelt_owned_text} {{ SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}"
            )),
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
            Some(Type::List(_)) if self.list_items_render_as_unknown(target) => {
                Ok(erased_to_list_text(text, None, "SmeltUnknown::String(ch.to_string().into())"))
            }
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::String) => {
                Ok(erased_to_list_text(
                    text,
                    Some(
                        "if let SmeltUnknown::String(value) = value { value.to_string() } else { value.to_string() }",
                    ),
                    "ch.to_string()",
                ))
            }
            Some(Type::List(item)) => {
                let item_text = self.extract_value_text("value", *item)?;
                let char_text =
                    self.extract_value_text("SmeltUnknown::String(ch.to_string().into())", *item)?;
                Ok(erased_to_list_text(text, Some(&item_text), &char_text))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(value) => value.smelt_shared_record(), SmeltUnknown::Array(values) => values.own_entries().into_iter().collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| (index.to_string(), SmeltUnknown::String(ch.to_string().into()))).collect(), SmeltUnknown::Function(value) => SmeltRecord::from([(\"__smelt_call\".to_owned(), SmeltUnknown::Function(value))]), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let item_text = self.extract_value_text("value", *item)?;
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => SmeltRecord::with_id_from_entries(values.id, values.into_iter().map(|(key, value)| (key, {item_text}))), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), {item_text})).collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| {{ let value = SmeltUnknown::String(ch.to_string().into()); (index.to_string(), {item_text}) }}).collect(), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_text = self.value_at_type_text("key", self.type_id(Type::String)?, *key)?;
                let item_text = self.extract_value_text("value", *item)?;
                if self.dict_uses_js_key_map(*key) {
                    return Ok(format!(
                        "if let SmeltUnknown::Object(values) = {smelt_owned_text} {{ SmeltJsMap::from_iter(values.into_iter().map(|(key, value)| ({key_text}, {item_text}))) }} else {{ SmeltJsMap::new() }}"
                    ));
                }
                Ok(format!(
                    "if let SmeltUnknown::Object(values) = {smelt_owned_text} {{ values.into_iter().map(|(key, value)| ({key_text}, {item_text})).collect::<::std::collections::HashMap<_, _>>() }} else {{ ::std::collections::HashMap::new() }}"
                ))
            }
            // A source `Map` recovers through `SmeltJsMap`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_map` marker: an erased Map
            // rebuilds its entries (and stable id) from the marker payload, and a
            // plain object falls back to string-keyed entries. This is the
            // inverse of the `SmeltJsMap` erasure adapter, so `unknown -> Map`
            // extraction stays lossless.
            Some(Type::JsMap(_, _)) => {
                let target_text = self.type_text_with_impl_trait(target, false)?;
                Ok(format!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))
            }
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name) => {
                let param_name = RustIdent::new(self.symbol_name(*name)?).into_string();
                Ok(format!(
                    "<{param_name} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))
            }
            Some(Type::TypeParam { .. }) => Ok(format!("({text}).into_smelt_unknown()")),
            // A concrete union stores a tagged `SmeltUnion…` enum, so an erased
            // `SmeltUnknown` value extracted into that destination must be
            // reconstructed into the matching variant rather than passed through
            // as `SmeltUnknown`. This mirrors the `from_smelt_unknown` boundary
            // `inject_union_value_text` applies for `Unknown`/`TypeParam` sources;
            // here the source is a wider (non-concrete) union still stored erased.
            Some(Type::Union(_)) if self.concrete_union_members(target).is_some() => Ok(format!(
                "{}::from_smelt_unknown(({text}).into_smelt_unknown())",
                union::union_name(target)
            )),
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(Type::Optional(inner)) => {
                if self.optional_inner_preserves_erased_singletons(*inner) {
                    let inner_text = self.extract_value_text(text, *inner)?;
                    return Ok(format!("Some({inner_text})"));
                }
                // The nullish guard and the `Some(...)` arm both read the source,
                // so a side-effecting `text` (e.g. an erased `.call(..)` that
                // consumes its argument list) would be evaluated twice and move
                // its inputs. Bind non-trivial sources to a single temporary so
                // the value is produced exactly once; a bare identifier is cheap
                // to re-read and keeps generated output stable.
                if is_trivial_reeval_expr(text) {
                    let inner_text = self.extract_value_text(text, *inner)?;
                    return Ok(format!(
                        "if smelt_unknown_is_nullish(&{text}) {{ None }} else {{ Some({inner_text}) }}"
                    ));
                }
                let inner_text = self.extract_value_text("smelt_optional_source", *inner)?;
                Ok(format!(
                    "{{ let smelt_optional_source = {text}; if smelt_unknown_is_nullish(&smelt_optional_source) {{ None }} else {{ Some({inner_text}) }} }}"
                ))
            }
            Some(Type::Tuple(items)) => {
                let items_text = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let value = format!(
                            "smelt_tuple_values.get({index}).cloned().unwrap_or(SmeltUnknown::Null)"
                        );
                        self.extract_value_text(&value, *item)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                let tuple_text = if items.len() == 1 {
                    format!("({items_text},)")
                } else {
                    format!("({items_text})")
                };
                Ok(format!(
                    "if let SmeltUnknown::Array(smelt_tuple_values) = {smelt_owned_text} {{ {tuple_text} }} else {{ panic!(\"unknown is not tuple\") }}"
                ))
            }
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "PropertyKey" => {
                Ok(text.to_owned())
            }
            // The prelude `SmeltFromUnknown` impl is the single inverse of the
            // `SmeltRegExp` erasure adapter, so `source`, `flags` AND the
            // writable `lastIndex` all survive a round trip through erased
            // dataflow (`clone(re)`, `structuredClone`, an `unknown` bag).
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(format!(
                "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})"
            )),
            // Both synthetic match-result classes are backed by the concrete
            // `SmeltMatch` Rust type, whose prelude `SmeltFromUnknown` impl is
            // the inverse of its erasure adapter. Without this arm the generic
            // class fallback below answers `Default::default()`, silently
            // turning a round-tripped match into an EMPTY match.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => Ok(format!(
                "<SmeltMatch as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
            )),
            Some(Type::Class { .. })
                if self.type_text_with_impl_trait(target, false)? == "SmeltUnknown" =>
            {
                Ok(text.to_owned())
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(target) => Ok(text.to_owned()),
            Some(Type::Class { .. }) if self.can_extract_unknown_object_record(target) => {
                if self.record_conversion_stack.borrow().contains(&target) {
                    return Ok("Default::default()".to_owned());
                }
                // The string-dict record adapter needs the prelude `String` and
                // `SmeltUnknown` types interned in this program's type table. A
                // program that never uses them (e.g. a concrete class union whose
                // generated `from_smelt_unknown` reconstruction reaches this class
                // member) has neither interned, so fall back to the same default
                // the no-adapter arm below produces rather than failing emission.
                let (Some(string_ty), Some(unknown_ty)) = (
                    self.find_type_id(&Type::String),
                    self.find_type_id(&Type::Unknown),
                ) else {
                    return Ok("Default::default()".to_owned());
                };
                self.record_conversion_stack.borrow_mut().push(target);
                let adapter_result = self.string_dict_record_adapter_text(
                    "smelt_record_map",
                    string_ty,
                    unknown_ty,
                    target,
                );
                self.record_conversion_stack.borrow_mut().pop();
                if let Some(adapter) = adapter_result? {
                    return Ok(format!(
                        "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => {{ let smelt_record_map = SmeltRecord::with_id_from_entries(values.id, values.into_iter()); {adapter} }}, _ => Default::default() }}"
                    ));
                }
                Ok("Default::default()".to_owned())
            }
            // A source `Set` recovers through `SmeltJsSet`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_set` marker (rebuilding members
            // and the stable id) and tolerantly accepts a bare array. This is the
            // inverse of the `SmeltJsSet` erasure adapter, so an `unknown -> Set`
            // cast (e.g. `data as unknown as Set` in a deep-equality walk) recovers
            // the real members instead of an empty set. Only `SmeltJsSet`-backed
            // sets have this impl; a `HashSet`-backed primitive set keeps the
            // `Default::default()` fallback below.
            Some(Type::Set(item)) if !self.type_is_hash_set_key_safe(*item) => {
                let target_text = self.type_text_with_impl_trait(target, false)?;
                Ok(format!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))
            }
            Some(Type::Set(_) | Type::Dict(_, _) | Type::Class { .. }) => {
                Ok("Default::default()".to_owned())
            }
            Some(Type::Function(function)) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    let length = function
                        .required_params
                        .unwrap_or_else(|| function.rest.unwrap_or(function.params.len()));
                    let default_callback = self.default_value(target)?;
                    return Ok(format!(
                        "{{ let smelt_value = {smelt_owned_text}; let smelt_function = match smelt_value.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ SmeltErasedFunction {{ callback: ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| (smelt_function)(smelt_args).unwrap_or_else(|error| panic!(\"{{}}\", error))), length: {length}.0, object: match smelt_value {{ SmeltUnknown::Object(object) => Some(object), _ => None }} }} }} else {{ {default_callback} }} }}"
                    ));
                }
                let target_text = self.type_text_with_impl_trait(target, false)?;
                let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
                // The adapted callback's PARAMETER types erase while
                // `target_text` and `return_ty` above use the caller's lexical
                // scope. Same split as `param_type_text`; threaded as a named
                // empty substitution rather than reconciled.
                let params = self
                    .callback_arg_decls(
                        function,
                        &TypeSubstitution::erased(),
                        MutablePrefix::Apply,
                    )?
                    .join(", ");
                let args = self.unknown_function_call_args_text(function)?;
                let call_text = if function.may_throw {
                    format!("(smelt_function)({args})?")
                } else {
                    format!(
                        "(smelt_function)({args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                    )
                };
                let converted_return_text = if let Some(Type::Future(item)) =
                    self.mir.types.get(function.return_ty)
                {
                    let _ = self.type_text_with_impl_trait(*item, false)?;
                    // JavaScript `await` flattens promise chains: an erased
                    // callable adapted to a promise-returning signature may
                    // itself return an erased promise (e.g. a Vitest
                    // `mockResolvedValue`/`mockRejectedValue` mock, or an erased
                    // async function), and awaiting the adapter must resolve —
                    // or reject as `Err` — through that inner promise instead of
                    // stringifying/extracting the promise value itself.
                    // `smelt_await_flatten` is an identity pass-through for every
                    // non-promise value, so already-settled plain results are
                    // unchanged; the flattened value is then extracted to the
                    // declared promise item type as before.
                    let converted_item = self.extract_value_text("smelt_flattened", *item)?;
                    format!(
                        "SmeltFuture::from_future(Box::pin(async move {{ let smelt_flattened = smelt_await_flatten((smelt_result).into_smelt_unknown()).await?; Ok::<_, Box<dyn std::error::Error>>({converted_item}) }}))"
                    )
                } else if return_ty == "SmeltUnknown" {
                    "smelt_result".to_owned()
                } else {
                    self.extract_value_text("smelt_result", function.return_ty)?
                };
                let return_text = if function.may_throw {
                    format!("Ok::<_, Box<dyn std::error::Error>>({converted_return_text})")
                } else {
                    converted_return_text
                };
                let default_callback = self.default_value(target)?;
                Ok(format!(
                    "{{ let smelt_source_value = {smelt_owned_text}; let smelt_function = match smelt_source_value.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: {target_text} = if let Some(smelt_original) = smelt_restore_function_origin::<{target_text}>(&smelt_function) {{ smelt_original }} else {{ ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let smelt_result = {call_text}; {return_text} }}) }}; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback }} else {{ {default_callback} }} }}"
                ))
            }
            // An already-erased `SmeltUnknown` at a `Type::Future` position is a
            // `SmeltUnknown::Promise` (e.g. `await`ing the result of an erased
            // vitest-mock call, or an `x as Promise<T>` cast). It CAN be recovered:
            // wrap a fresh promise-value handle whose body awaits the erased
            // promise through `smelt_await_flatten` and extracts its settled value
            // to the declared `Output`. Discarding it for `SmeltFuture::resolved`
            // of a default would drop the real resolved value and leave the shared
            // settle state unobserved (breaking, e.g., mock result matchers and
            // chained awaits on the recovered promise).
            Some(Type::Future(output)) => {
                let extracted = self.extract_value_text("smelt_awaited", *output)?;
                // The erased source is read OUTSIDE the `async move` block. The
                // block is `'static`, so a `{text}` that borrows — a callback
                // parameter passed by shared reference
                // (`callback_param_is_shared_reference`) is the case that reaches
                // here — cannot be named inside it (E0521). Reading it eagerly is
                // also what the source does: the promise value already exists at
                // this point; only the `await` is deferred.
                Ok(format!(
                    "{{ let smelt_erased_future = ({text}).into_smelt_unknown(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_awaited = smelt_await_flatten(smelt_erased_future).await?; Ok::<_, Box<dyn std::error::Error>>({extracted}) }})) }}"
                ))
            }
            other => Err(EmitError::new(format!(
                "checked extraction from unknown expression `{text}` to {other:?} is not implemented yet"
            ))),
        }
    }

    /// Render the erased argument vector used when a `SmeltUnknown::Function`
    /// is called through a concrete function type.
    ///
    /// Explicit rest metadata controls whether a packed list parameter is spread.
    fn unknown_function_call_args_text(
        &self,
        function: &FunctionType,
    ) -> Result<String, EmitError> {
        let mut statements = Vec::new();
        for (index, param_ty) in function.params.iter().enumerate() {
            if function.rest == Some(index)
                && let Some(Type::List(item_ty)) = self.mir.types.get(*param_ty)
            {
                let item_text = if matches!(
                    self.mir.types.get(*item_ty),
                    Some(Type::Unknown | Type::Never | Type::None | Type::TypeParam { .. })
                ) {
                    "value".to_owned()
                } else {
                    self.erase_value_text("value", *item_ty)?
                };
                statements.push(format!(
                    "smelt_call_args.extend(arg{index}.clone().into_iter().map(|value| {item_text}));"
                ));
            } else {
                // `arg{index}` is a binding in the adapter closure, and the
                // erased vector owns its elements. A `&mut T` parameter binds a
                // reference, and so does a by-shared-reference parameter
                // (`callback_param_is_shared_reference`) — both have to be
                // cloned out before they can be pushed, or the vector infers
                // `Vec<&SmeltUnknown>` and every later push mismatches (E0308).
                let arg_text = if function.mutable_params.contains(&index)
                    || self.callback_param_is_shared_reference(function, index, *param_ty)
                {
                    format!("arg{index}.clone()")
                } else {
                    format!("arg{index}")
                };
                // A list parameter whose elements are already `SmeltUnknown`
                // here erases by ALIASING the array, not by rebuilding it. In
                // JavaScript, passing an array where `unknown` is expected hands
                // over the same object: the erased value the callee sees must be
                // the array the caller holds, so a write through either is
                // visible through the other. `From<SmeltList<SmeltUnknown>> for
                // SmeltArray` is exactly that re-wrap — it carries both halves
                // of the reference (the `id` and the shared buffer) — whereas
                // the generic `erase_value_text` path walks the elements and
                // builds a detached copy.
                //
                // `erase_value_text` cannot reach that conclusion on its own:
                // it is handed the raw MIR type, sees a `Type::TypeParam` it
                // cannot prove is `Unknown`, and misses its own
                // `List<Unknown>` fast path. The predicate below is sound only
                // because these `arg{index}` bindings were declared under
                // `TypeSubstitution::erased()` (see `callback_arg_decls` at the
                // caller), which renders every type parameter as
                // `SmeltUnknown`.
                let item_text = if self.erased_param_is_unknown_list(*param_ty) {
                    format!("SmeltUnknown::Array(({arg_text}).into())")
                } else {
                    self.erase_value_text(&arg_text, *param_ty)?
                };
                statements.push(format!("smelt_call_args.push({item_text});"));
            }
        }
        Ok(format!(
            "{{ let mut smelt_call_args = Vec::new(); {} smelt_call_args }}",
            statements.join(" ")
        ))
    }

    /// Whether `list_ty` is a list whose elements render as `SmeltUnknown` in
    /// the *current* emission scope.
    ///
    /// `Type::Unknown` is that outright. A `Type::TypeParam` is too whenever the
    /// enclosing Rust item does not declare it: `rust_type` resolves such a name
    /// to [`crate::type_substitution::Resolved::Erased`], so `List<T>` is
    /// emitted as `SmeltList<SmeltUnknown>` and the per-element conversion the
    /// general path would run is `into_smelt_unknown()` on a value that already
    /// IS a `SmeltUnknown` — the identity impl. An *in-scope* `T` is a real Rust
    /// generic and must keep going through the general path, which converts each
    /// element through `SmeltFromUnknown`.
    ///
    /// Scope-aware rather than unconditional, and deliberately so: every
    /// destination Rust type is rendered by `type_text` under
    /// `TypeSubstitution::lexical(self.current_function_type_params())`, so this
    /// asks exactly the question that renderer answers.
    pub(super) fn list_items_render_as_unknown(&self, list_ty: TypeId) -> bool {
        let Some(Type::List(item)) = self.mir.types.get(list_ty) else {
            return false;
        };
        match self.mir.types.get(*item) {
            Some(Type::Unknown) => true,
            Some(Type::TypeParam { name }) => !self.current_function_has_type_param(*name),
            _ => false,
        }
    }

    /// Whether `param_ty` is a list whose elements are `SmeltUnknown` once the
    /// erased substitution is applied.
    ///
    /// `Type::Unknown` is that outright; a `Type::TypeParam` is too, because
    /// `TypeSubstitution::erased()` renders every type parameter as
    /// `SmeltUnknown` regardless of what the enclosing item declares. Only use
    /// this where the surrounding declaration was itself rendered under that
    /// substitution — elsewhere a type parameter may stand for a concrete Rust
    /// generic, and treating it as `Unknown` would be wrong. The scope-aware
    /// question is [`Self::list_items_render_as_unknown`].
    fn erased_param_is_unknown_list(&self, param_ty: TypeId) -> bool {
        let Some(Type::List(item)) = self.mir.types.get(param_ty) else {
            return false;
        };
        matches!(
            self.mir.types.get(*item),
            Some(Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Return whether `Option<T>` stores erased values that can carry explicit
    /// JavaScript singleton tags.
    ///
    /// This is only true when the `Option`'s payload is still stored as an erased
    /// `SmeltUnknown` (a bare `unknown`, an unscoped type parameter, or an erased
    /// host union), because such a payload can itself hold a `SmeltUnknown::Null`
    /// / `SmeltUnknown::Undefined` singleton without collapsing the `Option`. A
    /// *concrete* union inner (`Optional(string | RegExp)` lowered to
    /// `Option<SmeltUnion…>`) does **not** qualify: the tagged enum has no arm for
    /// `undefined`, so wrapping an erased `undefined` in `Some(from_smelt_unknown(…))`
    /// would fabricate a bogus member and lose the `undefined`. Those must fall
    /// through to the nullish-guarded extraction so `null`/`undefined` become
    /// `None` (fixes remeda `truncate`'s `separator?: string | RegExp` default).
    pub(super) fn optional_inner_preserves_erased_singletons(&self, inner: TypeId) -> bool {
        if self.concrete_union_members(inner).is_some() {
            return false;
        }
        matches!(
            self.mir.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(inner)
    }

    // Converts an awaited future operand without cloning it.
}

/// Scope guard that pops a type pair off
/// [`FunctionEmitter::type_expansion_stack`] when the expansion finishes.
///
/// Held by value at the expansion site (see
/// [`FunctionEmitter::enter_type_expansion`]) so the stack stays balanced even
/// when the expansion returns an `EmitError` through `?`.
pub(super) struct TypeExpansionGuard<'emit> {
    stack: &'emit RefCell<Vec<(TypeId, TypeId)>>,
}

impl Drop for TypeExpansionGuard<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().pop();
    }
}

/// Whether an expression is cheap and side-effect-free to evaluate more than
/// once. Bare locals and dotted field reads (`x`, `foo.bar.baz`) qualify; any
/// expression containing a call (`(`) may run side effects or move its inputs
/// and must be bound to a temporary before being read twice.
fn is_trivial_reeval_expr(text: &str) -> bool {
    !text.contains('(')
}

/// Emits the ONE arm set that rebuilds a typed `SmeltList` from an erased value.
///
/// Every element type funnels through here, so the set of sources a lowering
/// accepts does not depend on which element type it happens to carry. It used to:
/// the `SmeltUnknown`-element and `String`-element forms iterated a source string
/// into characters while the general form panicked on one, so the same JavaScript
/// value converted fine in `groupBy`'s adapter and blew up in `map`'s. The arms
/// mirror what JavaScript's iteration protocol accepts: nullish (empty, as
/// `Array.from` treats it), an array, a string (by character), a host byte
/// buffer, an `arguments` object, a `Map`/`Set`, and finally any object exposing
/// `Symbol.iterator`.
///
/// `item_text` converts one erased element bound to `value`; `None` means the
/// element type IS `SmeltUnknown`, so elements pass through unconverted.
/// `char_text` converts one `char` bound to `ch` for the string arm.
///
/// # Why the array arm aliases when `item_text` is `None`
///
/// A JavaScript array is a reference value, and `SmeltList<SmeltUnknown>` and
/// `SmeltArray` model that identically: an `id` plus a *shared*
/// `Rc<RefCell<Vec<SmeltUnknown>>>`. When no element conversion is needed the
/// two representations are byte-identical, so extracting an erased array into a
/// typed list is a re-wrap of the SAME array, not the construction of a new one.
/// Rebuilding the element vector (`values.into_vec()` into a fresh `Rc`) kept
/// the `id` but detached the storage, producing a half reference: a stale
/// snapshot wearing the live array's identity. A write through the typed handle
/// — `arr.push(x)` inside a callback that received the array being iterated —
/// then went to a dead copy and was invisible through the erased value, even
/// though JavaScript says both names denote one object.
///
/// It is also where the time went. Every crossing of an erased boundary paid an
/// O(n) memcpy plus n `SmeltUnknown` clones and n drops; a library that routes
/// each call through a runtime dispatcher (so the boundary is crossed once per
/// callback invocation) turned an O(n) operation into O(n^2).
///
/// When `item_text` is `Some(..)` the elements genuinely have to be converted,
/// so that arm still rebuilds — but it keeps `values.id`, exactly as before.
/// Every non-array arm builds a list that did not exist in the source program,
/// so it mints a fresh identity through `SmeltList::new`.
fn erased_to_list_text(text: &str, item_text: Option<&str>, char_text: &str) -> String {
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = cloned_value_text(text);
    // Converts a `Vec<SmeltUnknown>`-producing expression into the element type,
    // then wraps it in a list with a FRESH JS reference identity. Only the array
    // arm has a source identity to carry over.
    let convert = |elements: &str| match item_text {
        None => format!("SmeltList::new({elements})"),
        Some(item) => format!(
            "SmeltList::new({elements}.into_iter().map(|value| {item}).collect::<Vec<_>>())"
        ),
    };
    let byte_buffer_elements = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENTS;
    let arguments_elements = smelt_stdlib::runtime_symbols::host::ARGUMENTS_ELEMENTS;
    let array_arm = match item_text {
        // The aliasing re-wrap: same identity, same buffer, no copy.
        None => "SmeltList::with_storage(values.id, values.storage())".to_owned(),
        Some(item) => format!(
            "SmeltList::with_id(values.id, values.into_iter().map(|value| {item}).collect::<Vec<_>>())"
        ),
    };
    let string_arm =
        format!("SmeltList::new(value.chars().map(|ch| {char_text}).collect::<Vec<_>>())");
    let bytes_arm = convert("smelt_bytes");
    let arguments_arm = convert("smelt_args");
    let map_arm = convert("pairs.into_vec()");
    let set_arm = convert("members.into_vec()");
    let iterator_arm =
        convert("smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null))");
    format!(
        "{{ let smelt_src = {smelt_owned_text}.into_smelt_unknown(); \
         match smelt_src {{ \
         SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), \
         SmeltUnknown::Array(values) => {array_arm}, \
         SmeltUnknown::String(value) => {string_arm}, \
         SmeltUnknown::Object(value) => \
         if let Some(smelt_bytes) = {byte_buffer_elements}(&SmeltUnknown::Object(value.clone())) {{ {bytes_arm} }} \
         else if let Some(smelt_args) = {arguments_elements}(&value) {{ {arguments_arm} }} \
         else if let Some(SmeltUnknown::Array(pairs)) = value.get(\"__smelt_map\") {{ {map_arm} }} \
         else if let Some(SmeltUnknown::Array(members)) = value.get(\"__smelt_set\") {{ {set_arm} }} \
         else {{ match value.get(\"__smelt_symbol_iterator\") {{ \
         Some(SmeltUnknown::Function(iterator)) => {iterator_arm}, \
         _ => panic!(\"unknown is not iterable\") }} }}, \
         _ => panic!(\"unknown is not iterable\") }} }}"
    )
}
