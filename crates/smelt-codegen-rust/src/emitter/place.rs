//! Place emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a place to its Rust text representation.
    pub(super) fn place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_name(*local).map(str::to_owned),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, _)) = self.mir.types.get(base_ty)
                    && self.mir.types.get(*key) == Some(&Type::String)
                {
                    let field_name = self.symbol_name(*field)?;
                    return Ok(format!(
                        "{}.get({field_name:?}).cloned().expect(\"missing field\")",
                        self.local_name(*base)?
                    ));
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
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
                        let key_text = self.operand_as_type_text(index, *key_ty)?;
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
                        let key_text = self.operand_as_type_text(index, *key_ty)?;
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
        let index_text = self.operand_text(index)?;
        Ok(format!(
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}"
        ))
    }

    // Unknown/runtime type helpers continue in `unknown.rs`.
}
