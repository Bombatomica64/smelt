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
                        format!("SmeltUnknown::String({field_name:?}.to_owned())")
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
                    let base_text = self.local_value_text(*base)?;
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
                    let wrapped = self.unknown_wrap_value_text("value", *inner)?;
                    return Ok(format!(
                        "{}.clone().map_or(SmeltUnknown::Null, |value| {wrapped})",
                        self.local_value_text(*base)?
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.symbol_name(*name)? == "RegExp"
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
                    return Ok("SmeltUnknown::Null".to_owned());
                }
                if self.symbol_source_name(*field)? == "constructor" {
                    return Ok("SmeltUnknown::Null".to_owned());
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
                    return Ok("SmeltUnknown::Null".to_owned());
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
                            return Ok("SmeltUnknown::Null".to_owned());
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
                            self.operand_as_type_text(index, *key_ty)?
                        };
                        let base_text = self.local_value_text(*base)?;
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
                                "{base_text}.get(&{key_text}).expect(\"index out of bounds\")"
                            ))
                        } else if value_is_unknownish {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Null)"
                            ))
                        } else {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().expect(\"index out of bounds\")"
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
                    _ => Ok("SmeltUnknown::Null".to_owned()),
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
                            self.operand_as_type_text(index, *key_ty)?
                        };
                        Ok(format!(
                            "*{}.get_mut(&{key_text}).expect(\"index out of bounds\")",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists and dicts",
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
            self.operand_as_type_text(index, self.type_id(Type::Float)?)?
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
                    SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                    _ => panic!("unknown is not object"),
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
                    "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN }}"
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
