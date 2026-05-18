//! Place emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a place to its Rust text representation.
    pub(super) fn place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_name(*local).map(str::to_owned),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, _)) = self.mir.types.get(base_ty) {
                    let field_name = self.symbol_name(*field)?;
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
                    return Ok(format!(
                        "{}.get(&{key_text}).cloned().expect(\"missing field\")",
                        self.local_name(*base)?
                    ));
                }
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(base_ty)
                {
                    let field_name = self.symbol_name(*field)?;
                    let base_text = self.local_name(*base)?;
                    return Ok(format!(
                        "match {base_text}.clone() {{ SmeltUnknown::Object(map) => map.get({field_name:?}).cloned().unwrap_or(SmeltUnknown::Null), _ => SmeltUnknown::Null }}"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && (matches!(
                        self.mir.types.get(*inner),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                    ) || self.is_erased_class_type(*inner))
                {
                    let field_name = self.symbol_name(*field)?;
                    let base_text = self.local_name(*base)?;
                    return Ok(format!(
                        "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(map) => map.get({field_name:?}).cloned().unwrap_or(SmeltUnknown::Null), _ => SmeltUnknown::Null }}"
                    ));
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
                    return Ok("SmeltUnknown::Null".to_owned());
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::String)) {
                    return self.string_field_text(self.local_name(*base)?, *field);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                {
                    return Ok("SmeltUnknown::Null".to_owned());
                }
                Ok(format!(
                    "{}.{}",
                    self.local_name(*base)?,
                    sanitize_ident(self.symbol_name(*field)?)
                ))
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        let base_text = self.local_name(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        Ok(format!(
                            "{base_text}.get({index_text}).cloned().expect(\"index out of bounds\")"
                        ))
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
                            "{}.get(&{key_text}).cloned().expect(\"index out of bounds\")",
                            self.local_name(*base)?
                        ))
                    }
                    Some(Type::String) => {
                        let base_text = self.local_name(*base)?;
                        let index_text = self.normalized_index_text(
                            &format!("{base_text}.chars().count()"),
                            index,
                        )?;
                        Ok(format!(
                            "{base_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                        ))
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
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        let base_text = self.local_name(*base)?;
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
                            self.local_name(*base)?
                        ))
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists and dicts",
                    )),
                }
            }
            _ => self.place_text(place),
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

    // Unknown/runtime type helpers continue in `unknown.rs`.
}
