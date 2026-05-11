//! Unknown emission helpers.

use super::*;

impl FunctionEmitter<'_> {

    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    pub(super) fn unknown_wrap_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(operand)?;
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Unknown) => Ok(text),
            Some(Type::None) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({text})")),
            Some(Type::Int | Type::Float) => Ok(format!("SmeltUnknown::Number({text} as f64)")),
            Some(Type::String) => Ok(format!("SmeltUnknown::String({text})")),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!("SmeltUnknown::Array({text})"))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!("SmeltUnknown::Object({text})"))
            }
            _ => Err(EmitError::new(
                "cannot wrap this type into TypeScript unknown yet",
            )),
        }
    }

    /// Emits a runtime tag check for `SmeltUnknown`.
    /// Emits a runtime tag check for `SmeltUnknown`.
    pub(super) fn unknown_is_text(
        &self,
        value: &Operand,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        let pattern = match kind {
            smelt_hir::UnknownKind::Null => "SmeltUnknown::Null",
            smelt_hir::UnknownKind::Bool => "SmeltUnknown::Bool(_)",
            smelt_hir::UnknownKind::Number => "SmeltUnknown::Number(_)",
            smelt_hir::UnknownKind::String => "SmeltUnknown::String(_)",
            smelt_hir::UnknownKind::Array => "SmeltUnknown::Array(_)",
            smelt_hir::UnknownKind::Object => "SmeltUnknown::Object(_)",
        };
        Ok(format!("matches!({text}, {pattern})"))
    }

    /// Emits checked extraction from `SmeltUnknown` into a concrete Rust type.
    /// Emits checked extraction from `SmeltUnknown` into a concrete Rust type.
    pub(super) fn unknown_cast_text(&self, value: &Operand, target: TypeId) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text),
            Some(Type::None) => Ok(format!(
                "if matches!({text}, SmeltUnknown::Null) {{ () }} else {{ panic!(\"unknown is not null\") }}"
            )),
            Some(Type::Bool) => Ok(format!(
                "if let SmeltUnknown::Bool(value) = {text} {{ value }} else {{ panic!(\"unknown is not boolean\") }}"
            )),
            Some(Type::Float) => Ok(format!(
                "if let SmeltUnknown::Number(value) = {text} {{ value }} else {{ panic!(\"unknown is not number\") }}"
            )),
            Some(Type::Int) => Ok(format!(
                "if let SmeltUnknown::Number(value) = {text} {{ value as i64 }} else {{ panic!(\"unknown is not number\") }}"
            )),
            Some(Type::String) => Ok(format!(
                "if let SmeltUnknown::String(value) = {text} {{ value }} else {{ panic!(\"unknown is not string\") }}"
            )),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!(
                    "if let SmeltUnknown::Array(value) = {text} {{ value }} else {{ panic!(\"unknown is not array\") }}"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "if let SmeltUnknown::Object(value) = {text} {{ value }} else {{ panic!(\"unknown is not object\") }}"
                ))
            }
            _ => Err(EmitError::new(
                "checked extraction from unknown to this type is not implemented yet",
            )),
        }
    }

    // Converts an awaited future operand without cloning it.

}
