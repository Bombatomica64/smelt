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
use crate::rust::RustIdent;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    pub(super) fn value_at_type(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let target_is_erased = matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(target);
        if target_is_erased
            && let Operand::Copy(Place::Field { base, field })
            | Operand::Move(Place::Field { base, field }) = operand
            && matches!(
                self.mir.types.get(self.local_decl(*base)?.ty),
                Some(Type::String)
            )
        {
            let field_text = self.string_field_text(&self.local_value_text(*base)?, *field)?;
            let source_ty = match self.symbol_name(*field)? {
                "source" => self.type_id(Type::String)?,
                "global" | "ignoreCase" | "ignore_case" | "multiline" => {
                    self.type_id(Type::Bool)?
                }
                "length" => self.type_id(Type::Int)?,
                _ => self.type_id(Type::Unknown)?,
            };
            return self.erase_value_text(&field_text, source_ty);
        }
        if self.operand_ty(operand)? == target
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
        ) {
            return self.erase(operand);
        }
        if self.is_erased_class_type(target) {
            return self.erase(operand);
        }
        if matches!(operand, Operand::Const(Constant::None)) {
            return self.default_value(target);
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_text(operand)?;
            if self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner)) {
                return Ok(format!("{value_text}.clone().flatten()"));
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
            return Ok(format!("{}.source.clone()", self.operand_text(operand)?));
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
            return Ok(format!("({} as f64)", self.operand_text(operand)?));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Float)
        {
            return Ok(format!(
                "(({} as f64).trunc() as i64)",
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
            if let Some(adapter) = self.function_shape_adapter_text(operand, target, false)? {
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
            return Ok(format!("{text}.clone()"));
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
                "{}.into_iter().map(|value| {value_text}).collect::<Vec<_>>()",
                self.operand_text(operand)?
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
                "vec![{}]",
                self.value_at_type(operand, *target_item)?
            ));
        }
        if let (Some(Type::Dict(_, _)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && matches!(self.mir.types.get(*target_item), Some(Type::Function(_)))
        {
            return Ok("Vec::new()".to_owned());
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.value_at_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{}.into_iter().map(|(_, value)| {value_text}).collect::<Vec<_>>()",
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
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source_key != target_key || source_value != target_value)
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
                .function_shape_adapter_text(operand, target, false)?
                .ok_or_else(|| EmitError::new("function adapter was unexpectedly unavailable"));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        self.operand_text(operand)
    }

    /// Coerces already-rendered Rust value text from a known source type to a destination type.
    pub(super) fn value_at_type_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if source == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(value_text.to_owned());
        }
        if source == target && matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            if self.is_borrowed_callback_capture_name(value_text) {
                return self.borrowed_function_handle_text(value_text, target);
            }
            return Ok(format!("{value_text}.clone()"));
        }
        if let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let awaited =
                self.value_at_type_text("smelt_future_value", *source_item, *target_item)?;
            return Ok(format!(
                "Box::pin(async move {{ let smelt_future_value = {value_text}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }})"
            ));
        }
        if let (Some(Type::Tuple(source_items)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_items.len() == target_items.len()
        {
            let items_text = source_items
                .iter()
                .zip(target_items.iter())
                .enumerate()
                .map(|(index, (source_item, target_item))| {
                    self.value_at_type_text(
                        &format!("{value_text}.{index}"),
                        *source_item,
                        *target_item,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return if target_items.len() == 1 {
                Ok(format!("({items_text},)"))
            } else {
                Ok(format!("({items_text})"))
            };
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && value_text == "Default::default()"
        {
            return self.default_value(target);
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
        ) || self.is_erased_class_type(target)
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
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(source)
        {
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
            return Ok(format!("{value_text}.clone().flatten()"));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            if source_inner == target_inner {
                return Ok(format!("{value_text}.clone()"));
            }
            let mapped_value = self.value_at_type_text("value", *source_inner, *target_inner)?;
            return Ok(format!("{value_text}.clone().map(|value| {mapped_value})"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
        {
            let mapped_value = self.value_at_type_text("value", source, *inner)?;
            return Ok(format!(
                "match {value_text}.clone() {{ SmeltUnknown::Null => None, value => Some({mapped_value}) }}"
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
                "{value_text}.clone().unwrap_or({})",
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source) {
            let mapped_value = self.value_at_type_text("value", *inner, target)?;
            return Ok(format!(
                "{value_text}.clone().map_or({}, |value| {mapped_value})",
                self.default_value(target)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_item != target_item
        {
            let item_text = self.value_at_type_text("value", *source_item, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>()"
            ));
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
            return Ok(format!("vec![{item_text}]"));
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
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let item_text = self.value_at_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(_, value)| {item_text}).collect::<Vec<_>>()"
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
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
            && (source_key != target_key || source_value != target_value)
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
    pub(super) fn erase(&self, operand: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(operand)?;
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Unknown | Type::TypeParam { .. }) => Ok(text),
            Some(Type::None) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({text})")),
            Some(Type::Int | Type::Float) => Ok(format!("SmeltUnknown::Number({text} as f64)")),
            Some(Type::String) => Ok(format!("SmeltUnknown::String({text})")),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                // A bare list local has a stable storage address, so erasing the
                // SAME source binding twice must reuse one id (arrays compare
                // `===` by id). Key the identity on the live `Vec`'s address;
                // a fresh list (literal/transform result) keeps `SmeltArray::new`.
                if let Some(bare_local) = self.list_local_identity_key(operand)? {
                    return Ok(format!(
                        "{{ let smelt_list_id = smelt_list_identity(({bare_local}).as_ptr() as *const () as usize); SmeltUnknown::Array(SmeltArray::with_id(smelt_list_id, {text}.into())) }}"
                    ));
                }
                Ok(format!("SmeltUnknown::Array({text}.into())"))
            }
            Some(Type::List(item)) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                // See the unknown-item arm: a list local reuses a stable id keyed
                // on its `Vec` address; a fresh list keeps `SmeltArray::new`.
                if let Some(bare_local) = self.list_local_identity_key(operand)? {
                    return Ok(format!(
                        "{{ let smelt_list_id = smelt_list_identity(({bare_local}).as_ptr() as *const () as usize); SmeltUnknown::Array(SmeltArray::with_id(smelt_list_id, {text}.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>())) }}"
                    ));
                }
                Ok(format!(
                    "SmeltUnknown::Array({text}.into_iter().map(|value| {value_wrap}).collect())"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record(({text}).clone()))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                if self.mir.types.get(*item) == Some(&Type::Float) {
                    return Ok(format!(
                        "{{ let smelt_record = ({text}).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                    ));
                }
                Ok(format!(
                    "{{ let smelt_record = ({text}).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                ))
            }
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_text("key", *key)?;
                let value_wrap = self.erase_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::new({text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))"
                ))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(format!("({text}).clone().into_smelt_unknown()"))
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
                let value_wrap = self.erase_value_text("value", *item)?;
                // A Set erases to an array; like list bindings, the SAME source
                // Set binding erased twice must compare `===` equal (arrays
                // compare by id). `HashSet` has no `as_ptr`, so key the stable id
                // on the binding's own address (`&set`). Temps / fresh sets keep
                // `SmeltArray::new`.
                if let Some(bare_local) = self.list_local_identity_key(operand)? {
                    return Ok(format!(
                        "{{ let smelt_list_id = smelt_list_identity(&({bare_local}) as *const _ as *const () as usize); let mut values = {text}.clone().into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(SmeltArray::with_id(smelt_list_id, values)) }}"
                    ));
                }
                Ok(format!(
                    "{{ let mut values = {text}.clone().into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }}"
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
                    "{text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
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
            Some(Type::Union(_)) => Ok(text),
            Some(Type::Never | Type::Future(_)) | None => Ok("SmeltUnknown::Null".to_owned()),
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
        match self.mir.types.get(ty) {
            Some(Type::Unknown) => Ok(value_text.to_owned()),
            Some(Type::TypeParam { .. }) if value_text == "Default::default()" => {
                Ok("SmeltUnknown::Null".to_owned())
            }
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
            Some(Type::String) => Ok(format!("SmeltUnknown::String({value_text})")),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                // Method receiver: wrap a loose operand before `.into()`.
                Ok(format!(
                    "SmeltUnknown::Array({}.into())",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::List(item)) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Array({}.into_iter().map(|value| {value_wrap}).collect())",
                    value.parenthesized_if_needed()
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record(({value_text}).clone()))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.erase_value_text("value", *item)?;
                if self.mir.types.get(*item) == Some(&Type::Float) {
                    return Ok(format!(
                        "{{ let smelt_record = ({value_text}).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                    ));
                }
                Ok(format!(
                    "{{ let smelt_record = ({value_text}).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
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
                Ok(format!("({value_text}).clone().into_smelt_unknown()"))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(ty) && self.symbol_name(*name)? == "Date" =>
            {
                Ok(self.date_unknown_identity_text(value_text))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(ty) => Ok(value_text.to_owned()),
            Some(Type::Class { .. }) => self.class_unknown_object_text(value_text, ty),
            Some(Type::Set(item)) => {
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
                    "{}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})",
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
                let args = self.function_args_from_smelt_args_text(function)?;
                let call_text = format!("(smelt_function_value)({args})");
                let return_text = if self.mir.types.get(function.return_ty) == Some(&Type::None) {
                    if function.may_throw {
                        format!(
                            "{{ {call_text}?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null) }}"
                        )
                    } else {
                        format!(
                            "{{ {call_text}; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null) }}"
                        )
                    }
                } else if self.class_has_no_known_fields(function.return_ty) {
                    if function.may_throw {
                        call_text
                    } else {
                        format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({call_text})")
                    }
                } else if function.may_throw {
                    let value =
                        self.erase_value_text(&format!("{call_text}?"), function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                } else {
                    let value = self.erase_value_text(&call_text, function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                };
                Ok(format!(
                    "{{ let smelt_function_origin = {clone_receiver}.clone(); let smelt_function_value = {value_text}; let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); SmeltUnknown::Function(smelt_erased_function) }}",
                    clone_receiver = value.parenthesized_if_needed()
                ))
            }
            Some(Type::Union(_)) => Ok(value_text.to_owned()),
            Some(Type::Future(_)) => Ok("SmeltUnknown::Null".to_owned()),
        }
    }

    /// The erased `null`/`undefined` tag of `SmeltUnknown`.
    ///
    /// This is the canonical boxed "no value" the seam hands back when a typed
    /// value has nothing observable to carry across the boundary (a `None`
    /// return, an absent field, the default of an erased target). Keeping the
    /// variant text here means callers outside the seam never spell
    /// `SmeltUnknown::Null` by hand; they ask for the null tag by intent.
    pub(super) fn null_value_text(&self) -> String {
        "SmeltUnknown::Null".to_owned()
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
            "SmeltUnknown::Array({value_iter_text}.into_iter().map(SmeltUnknown::String).collect())"
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

    /// Box an expression that already evaluates to a collection of erased
    /// `SmeltUnknown` values into a `SmeltUnknown` array.
    ///
    /// `values_text` must be `Into<SmeltArray>` (e.g. a `Vec<SmeltUnknown>`); the
    /// elements have already crossed the seam. This owns the
    /// `SmeltUnknown::Array(.. .into())` construction so array-building paths that
    /// already hold erased values need not spell the variant.
    pub(super) fn erase_unknown_array_text(&self, values_text: &str) -> String {
        format!("SmeltUnknown::Array({values_text}.into())")
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
            "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([{entries_text}])))"
        )
    }

    /// Mark a timestamp-backed `Date` only when it crosses an erased value boundary.
    ///
    /// Internally dates stay as numeric timestamps for compact date arithmetic. The
    /// marker preserves JavaScript object identity for later dynamic `instanceof Date`
    /// checks without changing ordinary typed Date storage or comparisons.
    fn date_unknown_identity_text(&self, value_text: &str) -> String {
        format!(
            "match {value_text}.clone() {{ SmeltUnknown::Object(value) if value.contains_key(\"__smelt_date\") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(value))]))), value => value }}"
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

        let entries_result = fields
            .iter()
            .filter(|field| !matches!(field.visibility, smelt_hir::Visibility::Private))
            .map(|field| {
                let source_name = self.symbol_source_name(field.name)?;
                let field_name = sanitize_ident(self.symbol_name(field.name)?);
                if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                    let field_value = self.erase_value_text("value", *inner)?;
                    return Ok(format!(
                        "if let Some(value) = smelt_object_value.{field_name}.clone() {{ smelt_object_entries.insert({source_name:?}.to_owned(), {field_value}); }}"
                    ));
                }
                let field_value = if let Some(value) =
                    self.virtual_method_storage_field_text(target, target, field.name)?
                {
                    self.erase_value_text(&value, field.ty)?
                } else {
                    self.erase_value_text(
                        &format!("smelt_object_value.{field_name}"),
                        field.ty,
                    )?
                };
                Ok(format!(
                    "smelt_object_entries.insert({source_name:?}.to_owned(), {field_value});"
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
        Ok(format!(
            "{{ let smelt_object_value = {value_text}; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = ::std::collections::HashMap::new(); {entries} smelt_object_entries.insert(\"__smelt_class\".to_owned(), SmeltUnknown::Bool(true)); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }}"
        ))
    }

    /// Emits a runtime tag check for `SmeltUnknown`.
    /// Emits a runtime tag check for `SmeltUnknown`.
    pub(super) fn tag_check(
        &self,
        value: &Operand,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        if matches!(
            self.mir.types.get(self.operand_ty(value)?),
            Some(Type::Optional(_))
        ) {
            if kind == smelt_hir::UnknownKind::Null {
                return Ok(format!("{text}.is_none()"));
            }
            let check = self.tag_check_raw("smelt_value", kind)?;
            return Ok(format!(
                "{text}.as_ref().is_some_and(|smelt_value| {check})"
            ));
        }
        self.tag_check_raw(&text, kind)
    }

    /// Emits the JavaScript `typeof` string for a runtime-erased value.
    pub(super) fn typeof_value_text(&self, value: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        Ok(format!(
            "match {text}.clone() {{ SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(_) => \"boolean\".to_owned(), SmeltUnknown::Number(_) => \"number\".to_owned(), SmeltUnknown::String(_) => \"string\".to_owned(), SmeltUnknown::Symbol(_) => \"symbol\".to_owned(), SmeltUnknown::Function(_) => \"function\".to_owned(), SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"object\".to_owned() }}"
        ))
    }

    /// Emits the opaque `Object.getPrototypeOf` sentinel for an erased value.
    ///
    /// Defers all prototype discrimination to the `smelt_prototype_sentinel`
    /// runtime helper so the array/null/plain-object/class branches stay in one
    /// place. Class instances carry a hidden `__smelt_class` marker and map to a
    /// distinct sentinel, while non-class values keep their existing sentinels.
    pub(super) fn prototype_sentinel_text(&self, value: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        Ok(format!("smelt_prototype_sentinel(&({text}))"))
    }

    /// Emits a runtime tag check for already-rendered `SmeltUnknown` text.
    pub(super) fn tag_check_raw(
        &self,
        text: &str,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let pattern = match kind {
            smelt_hir::UnknownKind::Null => "SmeltUnknown::Null",
            smelt_hir::UnknownKind::Bool => "SmeltUnknown::Bool(_)",
            smelt_hir::UnknownKind::Number => "SmeltUnknown::Number(_)",
            smelt_hir::UnknownKind::String => "SmeltUnknown::String(_)",
            smelt_hir::UnknownKind::Symbol => "SmeltUnknown::Symbol(_)",
            smelt_hir::UnknownKind::Function => {
                return Ok(format!("matches!({text}, SmeltUnknown::Function(_))"));
            }
            smelt_hir::UnknownKind::Array => "SmeltUnknown::Array(_)",
            smelt_hir::UnknownKind::Object => {
                return Ok(format!(
                    "matches!({text}, SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Null)"
                ));
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
        let text = self.operand_text(value)?;
        self.extract_value_text(&text, target)
    }

    /// Emits checked extraction from an already-rendered `SmeltUnknown` value.
    pub(super) fn extract_value_text(
        &self,
        text: &str,
        target: TypeId,
    ) -> Result<String, EmitError> {
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
            Some(Type::None) => Ok(format!("{{ let _ = {text}.clone(); () }}")),
            Some(Type::Bool) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            Some(Type::Float) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }}"
            )),
            Some(Type::Int) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value {{ 1_i64 }} else {{ 0_i64 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => 0_i64 }}"
            )),
            Some(Type::String) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned() }}"
            )),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(value) => value.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), SmeltUnknown::Object(value) => match value.get(\"__smelt_symbol_iterator\") {{ Some(SmeltUnknown::Function(iterator)) => match iterator(vec![]).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Array(values) => values.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), _ => panic!(\"unknown iterator did not return iterable\") }}, _ => panic!(\"unknown is not iterable\") }}, _ => panic!(\"unknown is not iterable\") }}"
                ))
            }
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::String) => {
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| if let SmeltUnknown::String(value) = value {{ value }} else {{ value.to_string() }}).collect::<Vec<_>>(), SmeltUnknown::String(value) => value.chars().map(|ch| ch.to_string()).collect::<Vec<_>>(), SmeltUnknown::Object(value) => match value.get(\"__smelt_symbol_iterator\") {{ Some(SmeltUnknown::Function(iterator)) => match iterator(vec![]).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| if let SmeltUnknown::String(value) = value {{ value }} else {{ value.to_string() }}).collect::<Vec<_>>(), SmeltUnknown::String(value) => value.chars().map(|ch| ch.to_string()).collect::<Vec<_>>(), _ => panic!(\"unknown iterator did not return iterable\") }}, _ => panic!(\"unknown is not iterable\") }}, _ => panic!(\"unknown is not iterable\") }}"
                ))
            }
            Some(Type::List(item)) => {
                let item_text = self.extract_value_text("value", *item)?;
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| {item_text}).collect::<Vec<_>>(), SmeltUnknown::Object(value) => match value.get(\"__smelt_symbol_iterator\") {{ Some(SmeltUnknown::Function(iterator)) => match iterator(vec![]).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| {item_text}).collect::<Vec<_>>(), _ => panic!(\"unknown iterator did not return array\") }}, _ => panic!(\"unknown is not array\") }}, _ => panic!(\"unknown is not array\") }}"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(value) => SmeltRecord::with_id_from_entries(value.id, value.into_iter()), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| (index.to_string(), SmeltUnknown::String(ch.to_string()))).collect(), SmeltUnknown::Function(value) => SmeltRecord::from([(\"__smelt_call\".to_owned(), SmeltUnknown::Function(value))]), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let item_text = self.extract_value_text("value", *item)?;
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => SmeltRecord::with_id_from_entries(values.id, values.into_iter().map(|(key, value)| (key, {item_text}))), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), {item_text})).collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| {{ let value = SmeltUnknown::String(ch.to_string()); (index.to_string(), {item_text}) }}).collect(), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_text = self.value_at_type_text("key", self.type_id(Type::String)?, *key)?;
                let item_text = self.extract_value_text("value", *item)?;
                if self.dict_uses_js_key_map(*key) {
                    return Ok(format!(
                        "if let SmeltUnknown::Object(values) = {text}.clone() {{ SmeltJsMap::from_iter(values.into_iter().map(|(key, value)| ({key_text}, {item_text}))) }} else {{ SmeltJsMap::new() }}"
                    ));
                }
                Ok(format!(
                    "if let SmeltUnknown::Object(values) = {text}.clone() {{ values.into_iter().map(|(key, value)| ({key_text}, {item_text})).collect::<::std::collections::HashMap<_, _>>() }} else {{ ::std::collections::HashMap::new() }}"
                ))
            }
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name) => {
                let param_name = RustIdent::new(self.symbol_name(*name)?).into_string();
                Ok(format!(
                    "<{param_name} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))
            }
            Some(Type::TypeParam { .. }) => Ok(format!("({text}).into_smelt_unknown()")),
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(Type::Optional(inner)) => {
                let inner_text = self.extract_value_text(text, *inner)?;
                Ok(format!(
                    "if matches!({text}.clone(), SmeltUnknown::Null) {{ None }} else {{ Some({inner_text}) }}"
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
                    "if let SmeltUnknown::Array(smelt_tuple_values) = {text}.clone() {{ {tuple_text} }} else {{ panic!(\"unknown is not tuple\") }}"
                ))
            }
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "PropertyKey" => {
                Ok(text.to_owned())
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Object(value) => SmeltRegExp::new(match value.get(\"source\") {{ Some(SmeltUnknown::String(source)) => source, _ => String::new() }}, match value.get(\"flags\") {{ Some(SmeltUnknown::String(flags)) => flags, _ => String::new() }}), _ => SmeltRegExp::default() }}"
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
                self.record_conversion_stack.borrow_mut().push(target);
                let string_ty = self.type_id(Type::String)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
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
                        "{{ let smelt_value = {text}.clone(); let smelt_function = match smelt_value.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ SmeltErasedFunction {{ callback: ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| (smelt_function)(smelt_args).unwrap_or_else(|error| panic!(\"{{}}\", error))), length: {length}.0, object: match smelt_value {{ SmeltUnknown::Object(object) => Some(object), _ => None }} }} }} else {{ {default_callback} }} }}"
                    ));
                }
                let target_text = self.type_text_with_impl_trait(target, false)?;
                let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        Ok(format!(
                            "arg{index}: {}",
                            self.function_type_param_text(
                                function,
                                index,
                                *param,
                                &HashSet::new(),
                            )?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
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
                    let item_text = self.type_text_with_impl_trait(*item, false)?;
                    let converted_item = self.extract_value_text("smelt_result", *item)?;
                    format!(
                        "Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({converted_item}) }}) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<{item_text}, Box<dyn std::error::Error>>>>>"
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
                    "{{ let smelt_function = match {text}.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ if let Some(smelt_original) = smelt_restore_function_origin::<{target_text}>(&smelt_function) {{ smelt_original }} else {{ let smelt_callback: {target_text} = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let smelt_result = {call_text}; {return_text} }}); smelt_callback }} }} else {{ {default_callback} }} }}"
                ))
            }
            Some(Type::Future(_)) => Ok("Default::default()".to_owned()),
            _ => Err(EmitError::new(
                "checked extraction from unknown to this type is not implemented yet",
            )),
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
                let arg_text = if function.mutable_params.contains(&index) {
                    format!("arg{index}.clone()")
                } else {
                    format!("arg{index}")
                };
                let item_text = self.erase_value_text(&arg_text, *param_ty)?;
                statements.push(format!("smelt_call_args.push({item_text});"));
            }
        }
        Ok(format!(
            "{{ let mut smelt_call_args = Vec::new(); {} smelt_call_args }}",
            statements.join(" ")
        ))
    }

    // Converts an awaited future operand without cloning it.
}
