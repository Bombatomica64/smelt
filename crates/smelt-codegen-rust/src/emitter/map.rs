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
            if self.mir.types.get(dict_ty) == Some(&Type::Unknown) {
                let dict_text = self.operand_text(dict)?;
                let key_text = self.operand_text(key)?;
                let key_value = match self.mir.types.get(self.operand_ty(key)?) {
                    Some(Type::String) => key_text,
                    Some(Type::Int | Type::Float | Type::Bool) => format!("{key_text}.to_string()"),
                    Some(Type::Unknown) => format!("{key_text}.to_string()"),
                    _ => return Ok("false".to_owned()),
                };
                return Ok(format!(
                    "{{ let smelt_key = {key_value}; match {dict_text}.clone() {{ SmeltUnknown::Object(values) => values.contains_key(&smelt_key), SmeltUnknown::Array(values) => smelt_key == \"length\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < values.len()), SmeltUnknown::String(value) => smelt_key == \"length\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < value.chars().count()), _ => false }} }}"
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
        if self.operand_ty(key)? != *key_ty
            && !matches!(
                self.mir.types.get(self.operand_ty(key)?),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        {
            return Ok("Default::default()".to_owned());
        }
        let key_text = self.operand_as_type_text(key, *key_ty)?;
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
            return Ok(format!(
                "{}.get(&{}).cloned().unwrap_or({})",
                self.operand_text(dict)?,
                key_text,
                self.operand_text(default_operand)?
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Optional(inner)) if *inner == *value_ty)
        {
            return Err(EmitError::new(
                "dict get destination without default must be optional value",
            ));
        }
        Ok(format!(
            "{}.get(&{}).cloned()",
            self.operand_text(dict)?,
            key_text
        ))
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
        let dict_text = self.local_name(*local)?;
        let key_text = self.operand_text(key)?;
        let default_text = self.operand_text(default)?;
        Ok(format!(
            "{{ {dict_text}.entry({key_text}).or_insert({default_text}).clone() }}"
        ))
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
        if self.operand_ty(key)? != *key_ty {
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
        let dict_text = self.local_name(*local)?;
        let key_text = self.operand_as_type_text(key, *key_ty)?;
        let value_text = self.operand_as_type_text(value, *value_ty)?;
        Ok(format!(
            "{{ {dict_text}.insert({key_text}, {value_text}); {dict_text}.clone() }}"
        ))
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
                        self.local_name(*local)?
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
            self.local_name(*local)?,
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
        let dict_text = self.local_name(*local)?;
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
        let dict_text = self.local_name(*local)?;
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
            if self.operand_ty(source)? != target_ty {
                continue;
            }
            let source_text = self.operand_text(source)?;
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
                smelt_hir::DictProjectionOp::Keys => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys().cloned().collect::<Vec<_>>(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Values => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.values().cloned().collect::<Vec<_>>(), _ => Vec::new() }}"
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
            smelt_hir::DictProjectionOp::Keys => {
                Ok(format!("{dict_text}.keys().cloned().collect::<Vec<_>>()"))
            }
            smelt_hir::DictProjectionOp::Values => {
                Ok(format!("{dict_text}.values().cloned().collect::<Vec<_>>()"))
            }
            smelt_hir::DictProjectionOp::Entries => Ok(format!(
                "{dict_text}.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
            )),
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
