//! Place emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a place to its Rust text representation.
    pub(super) fn place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_value_text(*local),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, value)) = self.mir.types.get(base_ty) {
                    let field_name = self.symbol_source_name(*field)?;
                    let key_text = if self.mir.types.get(*key) == Some(&Type::String) {
                        format!("{field_name:?}.to_owned()")
                    } else if matches!(
                        self.mir.types.get(*key),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(*key)
                    {
                        self.erase_value_text(
                            &format!("{field_name:?}.to_owned()"),
                            self.type_id(Type::String)?,
                        )?
                    } else {
                        self.default_value(*key)?
                    };
                    let base_text = self.local_value_text(*base)?;
                    if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                        if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                            return Ok(format!("{base_text}.get(&{key_text}).flatten()"));
                        }
                        return Ok(format!("{base_text}.get(&{key_text}).cloned().flatten()"));
                    }
                    if self.mir.types.get(*key) == Some(&Type::String)
                        && self.type_text_with_impl_trait(*value, false)? == "SmeltUnknown"
                    {
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).unwrap_or(SmeltUnknown::Null)"
                        ));
                    }
                    if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).expect(\"missing field\")"
                        ));
                    }
                    return Ok(format!(
                        "{base_text}.get(&{key_text}).cloned().expect(\"missing field\")"
                    ));
                }
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(base_ty)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let field_rule = smelt_stdlib::typescript_field_rule(field_name);
                    let base_text = self.local_value_text(*base)?;
                    if field_rule == Some(smelt_stdlib::FieldRule::TsLength) {
                        return Ok(format!(
                            "match {base_text}.clone() {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    if field_rule == Some(smelt_stdlib::FieldRule::TsSort) {
                        return Ok(format!(
                            "match {base_text}.clone() {{ SmeltUnknown::Array(value) => smelt_array_sort_method(value), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"sort\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    return Ok(format!(
                        "match {base_text}.clone() {{ SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Null }}"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && (matches!(
                        self.mir.types.get(*inner),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                    ) || self.is_erased_class_type(*inner))
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    if smelt_stdlib::typescript_field_rule(field_name)
                        == Some(smelt_stdlib::FieldRule::TsLength)
                    {
                        return Ok(format!(
                            "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    return Ok(format!(
                        "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Null }}"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(Type::Dict(key_ty, _)) = self.mir.types.get(*inner)
                    && self.mir.types.get(*key_ty) == Some(&Type::String)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    return Ok(format!(
                        "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.get({field_name:?}))"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && self.symbol_source_name(*field)? == "value"
                {
                    let wrapped = self.erase_value_text("value", *inner)?;
                    return Ok(format!(
                        "{}.clone().map_or(SmeltUnknown::Null, |value| {wrapped})",
                        self.local_value_text(*base)?
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_regexp_class_symbol(*name)?
                {
                    return self.regexp_field_text(&self.local_value_text(*base)?, *field);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(fields) = self.structural_record_fields(*inner)
                    && let Some(field_ty) = fields
                        .iter()
                        .find(|candidate| candidate.name == *field)
                        .map(|candidate| candidate.ty)
                {
                    let base_text = self.local_value_text(*base)?;
                    let field_name = sanitize_ident(self.symbol_name(*field)?);
                    return if matches!(self.mir.types.get(field_ty), Some(Type::Optional(_))) {
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    } else {
                        Ok(format!(
                            "{base_text}.as_ref().map(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    };
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
                    return Ok(self.null_value_text());
                }
                if smelt_stdlib::typescript_field_rule(self.symbol_source_name(*field)?)
                    == Some(smelt_stdlib::FieldRule::TsConstructor)
                {
                    return Ok(self.null_value_text());
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::String)) {
                    return self.string_field_text(&self.local_value_text(*base)?, *field);
                }
                if self.storage_field_is_function(base_ty, *field) {
                    return Ok(format!(
                        "{}.{}.clone()",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(method_text) = self.class_method_reference_text(
                        &self.local_value_text(*base)?,
                        *name,
                        *field,
                    )?
                {
                    return Ok(method_text);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                {
                    return Ok(self.null_value_text());
                }
                Ok(format!(
                    "{}.{}",
                    self.local_value_text(*base)?,
                    sanitize_ident(self.symbol_name(*field)?)
                ))
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(item_ty)) => {
                        let base_text = self.local_mut_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        let missing = self.default_value(*item_ty)?;
                        Ok(format!(
                            "{base_text}.get({index_text}).cloned().unwrap_or({missing})"
                        ))
                    }
                    Some(Type::Optional(inner_ty))
                        if matches!(self.mir.types.get(*inner_ty), Some(Type::List(_))) =>
                    {
                        let Some(Type::List(item_ty)) = self.mir.types.get(*inner_ty) else {
                            return Ok(self.null_value_text());
                        };
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_index_text(
                            &format!("{base_text}.as_ref().map_or(0, Vec::len)"),
                            index,
                        )?;
                        let access =
                            if matches!(self.mir.types.get(*item_ty), Some(Type::Optional(_))) {
                                "values.get({index_text}).cloned().flatten()"
                            } else {
                                "values.get({index_text}).cloned()"
                            };
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|values| {})",
                            access.replace("{index_text}", &index_text)
                        ))
                    }
                    Some(Type::Dict(key_ty, value_ty)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        let base_text = self.local_value_text(*base)?;
                        let default_value = self.default_value(*value_ty)?;
                        let value_is_unknownish = matches!(
                            self.mir.types.get(*value_ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        );
                        if (self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty))
                            && value_is_unknownish
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Null)"
                            ))
                        } else if self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty)
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).unwrap_or({default_value})"
                            ))
                        } else if value_is_unknownish {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Null)"
                            ))
                        } else {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or({default_value})"
                            ))
                        }
                    }
                    Some(Type::String) => {
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_index_text(
                            &format!("{base_text}.chars().count()"),
                            index,
                        )?;
                        Ok(format!(
                            "{base_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                        ))
                    }
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.unknown_index_text(&self.local_value_text(*base)?, index)
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!("{}.{tuple_index}", self.local_value_text(*base)?))
                    }
                    _ => Ok(self.null_value_text()),
                }
            }
        }
    }

    /// Converts a place to its Rust text representation for assignment.
    /// Converts a place to its Rust text representation for assignment.
    pub(super) fn assignment_place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_mut_value_text(*local),
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        let base_text = self.local_mut_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        Ok(format!("{base_text}[{index_text}]"))
                    }
                    Some(Type::Dict(key_ty, _)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        Ok(format!(
                            "*{}.get_mut(&{key_text}).expect(\"index out of bounds\")",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!(
                            "{}.{tuple_index}",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists, dicts, and tuples",
                    )),
                }
            }
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if self.structural_record_fields(base_ty).is_some() {
                    return Ok(format!(
                        "{}.{}",
                        self.local_mut_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                self.place_text(place)
            }
        }
    }

    /// Resolves a statically known tuple index for Rust field access.
    pub(super) fn tuple_index(&self, index: &Operand, len: usize) -> Result<usize, EmitError> {
        let value = match index {
            Operand::Const(Constant::Int(value)) => usize::try_from(*value).ok(),
            _ => None,
        }
        .ok_or_else(|| EmitError::new("tuple index must be a non-negative constant integer"))?;
        if value >= len {
            return Err(EmitError::new("tuple index is out of bounds"));
        }
        Ok(value)
    }

    /// Gets the type of a place.
    /// Converts a Python-style element index into a Rust `usize` expression.
    ///
    /// Negative indexes are offset from the collection length. Bounds are not
    /// clamped because Python element indexing raises when the normalized index
    /// is still outside the collection; the generated Rust keeps that behavior
    /// with `expect` on negative conversion and the eventual indexed lookup.
    pub(super) fn normalized_index_text(
        &self,
        len_expr: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(format!(
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}"
        ))
    }

    /// Emit a runtime index read for values whose concrete shape is erased.
    ///
    /// TypeScript generic and unknown receivers may still be strings, arrays,
    /// or objects at runtime. Returning `Null` here hides lowering bugs and
    /// breaks later casts, so the generated Rust dispatches on `SmeltUnknown`
    /// and panics only when the runtime value is not indexable.
    pub(super) fn unknown_index_text(
        &self,
        base_text: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = self.operand_text(index)?;
        let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
        if matches!(self.mir.types.get(index_ty), Some(Type::String)) {
            return Ok(format!(
                r#"match {base_text}.clone() {{
                    SmeltUnknown::String(value) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number(value.chars().count() as f64)
                        }} else {{
                            smelt_key.parse::<usize>().ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))).unwrap_or(SmeltUnknown::Null)
                        }}
                    }}
                    SmeltUnknown::Array(values) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number(values.len() as f64)
                        }} else {{
                            smelt_key.parse::<usize>().ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Null)
                        }}
                    }}
                    SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                    _ => SmeltUnknown::Null,
                }}"#
            ));
        }
        let numeric_index_text = match self.mir.types.get(index_ty) {
            Some(Type::Int | Type::Float) => index_text,
            Some(Type::Bool) => format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}"),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if self.is_erased_class_type(index_ty)
                    || matches!(
                        self.mir.types.get(index_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) =>
            {
                format!(
                    "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN }}"
                )
            }
            _ => "f64::NAN".to_owned(),
        };

        Ok(format!(
            r"match {base_text}.clone() {{
                    SmeltUnknown::String(value) => {{
                        let len = value.chars().count() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))).unwrap_or(SmeltUnknown::Null)
                    }}
                    SmeltUnknown::Array(values) => {{
                        let len = values.len() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Null)
                    }}
                SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                _ => SmeltUnknown::Null,
            }}"
        ))
    }

    // Unknown/runtime type helpers continue in `unknown.rs`.
}
