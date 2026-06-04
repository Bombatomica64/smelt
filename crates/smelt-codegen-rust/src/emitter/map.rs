//! Map emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a dictionary key containment operation to Rust text.
    pub(super) fn dict_contains_key_text(
        &self,
        dict: &Operand,
        key: &Operand,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, _)) = self.mir.types.get(dict_ty) else {
            if self.dict_contains_key_uses_erased_object(dict_ty) {
                let dict_text = self.operand_text(dict)?;
                let key_text = self.operand_text(key)?;
                let key_value = match self.mir.types.get(self.operand_ty(key)?) {
                    Some(Type::String) => key_text,
                    Some(Type::Int | Type::Float | Type::Bool) => format!("{key_text}.to_string()"),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        format!("{key_text}.to_string()")
                    }
                    _ => return Ok("false".to_owned()),
                };
                return Ok(format!(
                    "{{ let smelt_key = {key_value}; match {dict_text}.clone() {{ SmeltUnknown::Object(values) => values.contains_key(&smelt_key), SmeltUnknown::Array(values) => smelt_key == \"length\" || smelt_key == \"__smelt_symbol_iterator\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < values.len()), SmeltUnknown::String(value) => smelt_key == \"length\" || smelt_key == \"__smelt_symbol_iterator\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < value.chars().count()), _ => false }} }}"
                ));
            }
            return Ok("false".to_owned());
        };
        if self.operand_ty(key)? != *key_ty {
            return Ok("false".to_owned());
        }
        Ok(format!(
            "{}.contains_key(&{})",
            self.operand_text(dict)?,
            self.operand_text(key)?
        ))
    }

    /// Return whether a dictionary containment check must inspect an erased
    /// JavaScript object value at runtime.
    fn dict_contains_key_uses_erased_object(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::Union(_)) => true,
            Some(Type::TypeParam { name }) => !self.current_function_has_type_param(*name),
            _ => false,
        }
    }

    /// Converts a dictionary get operation to Rust text.
    /// Converts a dictionary get operation to Rust text.
    pub(super) fn dict_get_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("dict get receiver must be a dict"));
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok("Default::default()".to_owned());
        }
        let key_text = self.dict_key_operand_text(key, *key_ty)?;
        if let Some(default_operand) = default {
            if self.operand_ty(default_operand)? != *value_ty {
                return Err(EmitError::new(
                    "dict get default must match the dict value type",
                ));
            }
            if dest_ty != *value_ty {
                return Err(EmitError::new(
                    "dict get destination with default must match the dict value type",
                ));
            }
            let get_text =
                if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                    format!("{}.get(&{})", self.operand_text(dict)?, key_text)
                } else {
                    format!("{}.get(&{}).cloned()", self.operand_text(dict)?, key_text)
                };
            return Ok(format!(
                "{get_text}.unwrap_or({})",
                self.operand_text(default_operand)?
            ));
        }
        match (self.mir.types.get(*value_ty), self.mir.types.get(dest_ty)) {
            (Some(Type::Optional(value_inner)), Some(Type::Optional(dest_inner)))
                if value_inner == dest_inner =>
            {
                let get_text =
                    if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                        format!("{}.get(&{})", self.operand_text(dict)?, key_text)
                    } else {
                        format!("{}.get(&{}).cloned()", self.operand_text(dict)?, key_text)
                    };
                Ok(format!("{get_text}.flatten()"))
            }
            (_, Some(Type::Optional(dest_inner))) if dest_inner == value_ty => {
                if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!("{}.get(&{})", self.operand_text(dict)?, key_text))
                } else {
                    Ok(format!(
                        "{}.get(&{}).cloned()",
                        self.operand_text(dict)?,
                        key_text
                    ))
                }
            }
            _ => Err(EmitError::new(
                "dict get destination without default must be optional value",
            )),
        }
    }

    /// Converts a dictionary setdefault operation to Rust text.
    ///
    /// This mapping supports the explicit-default form only, so generated Rust
    /// can preserve the dictionary value type without inventing a `None`
    /// default for non-optional values.
    /// Converts a dictionary setdefault operation to Rust text.
    ///
    /// This mapping supports the explicit-default form only, so generated Rust
    /// can preserve the dictionary value type without inventing a `None`
    /// default for non-optional values.
    pub(super) fn dict_setdefault_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("dict setdefault receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new(
                "dict setdefault key must match the dict key type",
            ));
        }
        if self.operand_ty(default)? != *value_ty {
            return Err(EmitError::new(
                "dict setdefault default must match the dict value type",
            ));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict setdefault destination must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict setdefault receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let key_text = self.operand_text(key)?;
        let default_text = self.operand_text(default)?;
        if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
            Ok(format!(
                "{{ if let Some(value) = {dict_text}.get(&{key_text}) {{ value }} else {{ {dict_text}.insert({key_text}, {default_text}.clone()); {default_text} }} }}"
            ))
        } else {
            Ok(format!(
                "{{ {dict_text}.entry({key_text}).or_insert({default_text}).clone() }}"
            ))
        }
    }

    /// Converts a dictionary insertion operation to Rust text.
    /// Converts a dictionary insertion operation to Rust text.
    pub(super) fn dict_set_text(
        &self,
        dict: &Operand,
        key: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Ok("Default::default()".to_owned());
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok(self.operand_text(dict)?);
        }
        if self.operand_ty(value)? != *value_ty {
            return Ok(self.operand_text(dict)?);
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict set destination must match the receiver dict type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict set receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let key_text = self.dict_key_operand_text(key, *key_ty)?;
        let value_text = self.value_at_type(value, *value_ty)?;
        Ok(format!(
            "{{ {dict_text}.insert({key_text}, {value_text}); {dict_text}.clone() }}"
        ))
    }

    /// Return whether a key operand can be used for a dictionary operation.
    fn dict_key_operand_is_compatible(
        &self,
        key: &Operand,
        key_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if operand_ty == key_ty {
            return Ok(true);
        }
        Ok(matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) || matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty))
    }

    /// Render a dictionary key, unwrapping optional keys after frontend narrowing.
    fn dict_key_operand_text(&self, key: &Operand, key_ty: TypeId) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty)
        {
            let key_text = self.operand_text(key)?;
            let default = self.default_value(key_ty)?;
            return Ok(format!("{key_text}.clone().unwrap_or({default})"));
        }
        self.value_at_type(key, key_ty)
    }

    /// Converts a dictionary key removal operation to Rust text.
    /// Converts a dictionary key removal operation to Rust text.
    pub(super) fn dict_remove_key_text(
        &self,
        dict: &Operand,
        key: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, _)) = self.mir.types.get(dict_ty) else {
            if matches!(
                self.mir.types.get(dict_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            ) || self.is_erased_class_type(dict_ty)
            {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("dict remove destination must be bool"));
                }
                let rendered_key = self.operand_text(key)?;
                let key_text =
                    self.property_key_to_string_text(&rendered_key, self.operand_ty(key)?)?;
                if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                    dict
                {
                    return Ok(format!(
                        "match &mut {} {{ SmeltUnknown::Object(map) => map.remove(&{key_text}).is_some(), _ => true }}",
                        self.local_mut_value_text(*local)?
                    ));
                }
                return Ok("true".to_owned());
            }
            return Err(EmitError::new("dict remove receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Ok("false".to_owned());
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
            return Err(EmitError::new("dict remove destination must be bool"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict remove receiver must be a mutable local for now",
            ));
        };
        Ok(format!(
            "{}.remove(&{}).is_some()",
            self.local_mut_value_text(*local)?,
            self.operand_text(key)?
        ))
    }

    /// Converts a dictionary pop operation to Rust text.
    /// Converts a dictionary pop operation to Rust text.
    pub(super) fn dict_pop_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("dict pop receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new("dict pop key must match the dict key type"));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict pop destination must match the dict value type",
            ));
        }
        if let Some(default_operand) = default
            && self.operand_ty(default_operand)? != *value_ty
        {
            return Err(EmitError::new(
                "dict pop default must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict pop receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let key_text = self.operand_text(key)?;
        if let Some(default_operand) = default {
            let default_text = self.operand_text(default_operand)?;
            Ok(format!(
                "{dict_text}.remove(&{key_text}).unwrap_or({default_text})"
            ))
        } else {
            Ok(format!(
                "{dict_text}.remove(&{key_text}).expect(\"dict pop missing key\")"
            ))
        }
    }

    /// Converts a dictionary update operation to Rust text.
    /// Converts a dictionary update operation to Rust text.
    pub(super) fn dict_update_text(
        &self,
        dict: &Operand,
        other: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict update receiver must be a dict"));
        }
        if self.operand_ty(other)? != dict_ty {
            return Err(EmitError::new(
                "dict update argument must match the receiver dict type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new("dict update destination must be None"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict update receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let other_text = self.operand_text(other)?;
        Ok(format!(
            "{{ {dict_text}.extend({other_text}.iter().map(|(key, value)| (key.clone(), value.clone()))); () }}"
        ))
    }

    /// Converts a dictionary assign operation to Rust text.
    pub(super) fn dict_assign_text(
        &self,
        target: &Operand,
        sources: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let target_ty = self.operand_ty(target)?;
        if !matches!(self.mir.types.get(target_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict assign target must be a dict"));
        }
        if dest_ty != target_ty {
            return Err(EmitError::new(
                "dict assign destination must match the target dict type",
            ));
        }
        let target_text = self.operand_text(target)?;
        let mut steps = vec![format!("let mut assigned = {target_text}.clone();")];
        for source in sources {
            let source_ty = self.operand_ty(source)?;
            let source_text = if source_ty == target_ty {
                self.operand_text(source)?
            } else if matches!(
                self.mir.types.get(source_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) {
                let value_text = self.operand_text(source)?;
                self.value_at_type_text(&value_text, source_ty, target_ty)?
            } else {
                continue;
            };
            steps.push(format!(
                "assigned.extend({source_text}.iter().map(|(key, value)| (key.clone(), value.clone())));"
            ));
        }
        steps.push("assigned".to_owned());
        Ok(format!("{{ {} }}", steps.join(" ")))
    }

    /// Converts a dictionary copy operation to Rust text.
    /// Converts a dictionary copy operation to Rust text.
    pub(super) fn dict_copy_text(
        &self,
        dict: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Ok("Default::default()".to_owned());
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict copy destination must match the receiver dict type",
            ));
        }
        Ok(format!("{}.clone()", self.operand_text(dict)?))
    }

    /// Converts a dictionary projection operation to Rust text.
    /// Converts a dictionary projection operation to Rust text.
    pub(super) fn dict_projection_text(
        &self,
        op: smelt_hir::DictProjectionOp,
        dict: &Operand,
    ) -> Result<String, EmitError> {
        let dict_text = self.operand_text(dict)?;
        if matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            return match op {
                smelt_hir::DictProjectionOp::FromEntries => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Array(entries) => entries.into_iter().filter_map(|entry| match entry {{ SmeltUnknown::Array(values) if values.len() >= 2 => {{ let mut values = values.into_iter(); let key = match values.next()? {{ SmeltUnknown::String(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), _ => return None }}; Some((key, values.next()?)) }}, _ => None }}).collect::<SmeltRecord<String, SmeltUnknown>>(), _ => SmeltRecord::new() }}"
                )),
                smelt_hir::DictProjectionOp::Keys => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Values => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.values(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Entries => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.into_iter().collect::<Vec<_>>(), _ => Vec::new() }}"
                )),
            };
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Dict(_, _))
        ) {
            return Err(EmitError::new("dict projection receiver must be a dict"));
        }
        match op {
            smelt_hir::DictProjectionOp::FromEntries => {
                Err(EmitError::new("fromEntries receiver must be erased"))
            }
            smelt_hir::DictProjectionOp::Keys => {
                if let Some(Type::Dict(key_ty, _)) = self.mir.types.get(self.operand_ty(dict)?)
                    && (self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty))
                {
                    Ok(format!("{dict_text}.keys().collect::<Vec<_>>()"))
                } else {
                    Ok(format!("{dict_text}.keys().cloned().collect::<Vec<_>>()"))
                }
            }
            smelt_hir::DictProjectionOp::Values => {
                if let Some(Type::Dict(key_ty, _)) = self.mir.types.get(self.operand_ty(dict)?)
                    && (self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty))
                {
                    Ok(format!("{dict_text}.values().collect::<Vec<_>>()"))
                } else {
                    Ok(format!("{dict_text}.values().cloned().collect::<Vec<_>>()"))
                }
            }
            smelt_hir::DictProjectionOp::Entries => {
                if let Some(Type::Dict(key_ty, _)) = self.mir.types.get(self.operand_ty(dict)?)
                    && (self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty))
                {
                    Ok(format!("{dict_text}.iter().collect::<Vec<_>>()"))
                } else {
                    Ok(format!(
                        "{dict_text}.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
                    ))
                }
            }
        }
    }

    /// Converts a JSON serialization operation to Rust text.
    ///
    /// The Serde JSON backend is intentionally confined to this helper and
    /// Cargo dependency injection, making it replaceable without changing HIR
    /// or frontend lowering.
    /// Converts a JSON serialization operation to Rust text.
    ///
    /// The Serde JSON backend is intentionally confined to this helper and
    /// Cargo dependency injection, making it replaceable without changing HIR
    /// or frontend lowering.
    pub(super) fn json_stringify_text(
        &self,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(self.mir.types.get(dest_ty), Some(Type::String)) {
            return Err(EmitError::new("JSON stringify destination must be string"));
        }
        if !self.is_json_serializable_type(self.operand_ty(value)?) {
            return Err(EmitError::new(
                "JSON stringify value must be JSON-serializable",
            ));
        }
        Ok(format!(
            "serde_json::to_string(&{}).expect(\"JSON serialization failed\")",
            self.operand_text(value)?
        ))
    }

    /// Converts a JSON parse operation to Rust text.
    ///
    /// Serde stays behind this helper so future backend changes do not affect
    /// the frontend lowering shape.
    /// Converts a JSON parse operation to Rust text.
    ///
    /// Serde stays behind this helper so future backend changes do not affect
    /// the frontend lowering shape.
    pub(super) fn json_parse_text(
        &self,
        text: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(text)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("JSON parse input must be a string"));
        }
        if !self.is_json_serializable_type(dest_ty) {
            return Err(EmitError::new(
                "JSON parse destination must be JSON-compatible",
            ));
        }
        Ok(format!(
            "serde_json::from_str::<{}>(&{}).expect(\"JSON parse failed\")",
            self.type_text(dest_ty)?,
            self.operand_text(text)?
        ))
    }

    // Returns whether a type is supported by the current JSON serializer path.
}
