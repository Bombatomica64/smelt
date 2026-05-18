//! Unknown emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    pub(super) fn unknown_wrap_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(operand)?;
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Unknown | Type::TypeParam { .. }) => Ok(text),
            Some(Type::None) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({text})")),
            Some(Type::Int | Type::Float) => Ok(format!("SmeltUnknown::Number({text} as f64)")),
            Some(Type::String) => Ok(format!("SmeltUnknown::String({text})")),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!("SmeltUnknown::Array({text})"))
            }
            Some(Type::List(item)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Array({text}.into_iter().map(|value| {value_wrap}).collect())"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!("SmeltUnknown::Object({text})"))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object({text}.into_iter().map(|(key, value)| (key, {value_wrap})).collect())"
                ))
            }
            Some(Type::Dict(_, _)) | Some(Type::Class { .. }) => {
                Ok("SmeltUnknown::Object(::std::collections::HashMap::new())".to_owned())
            }
            Some(Type::Set(_)) | Some(Type::Tuple(_)) => {
                Ok("SmeltUnknown::Array(Vec::new())".to_owned())
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(_)) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Never | Type::Future(_) | Type::Union(_)) | None => {
                Ok("SmeltUnknown::Null".to_owned())
            }
        }
    }

    /// Wrap a rendered value expression with a known static type into `SmeltUnknown`.
    pub(super) fn unknown_wrap_value_text(
        &self,
        value_text: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(ty) {
            Some(Type::Unknown) => Ok(value_text.to_owned()),
            Some(Type::TypeParam { .. }) => Ok(format!(
                "IntoSmeltUnknown::into_smelt_unknown({value_text})"
            )),
            Some(Type::None | Type::Never) | None => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({value_text})")),
            Some(Type::Int | Type::Float) => {
                Ok(format!("SmeltUnknown::Number({value_text} as f64)"))
            }
            Some(Type::String) => Ok(format!("SmeltUnknown::String({value_text})")),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!("SmeltUnknown::Array({value_text})"))
            }
            Some(Type::List(item)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Array({value_text}.into_iter().map(|value| {value_wrap}).collect())"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!("SmeltUnknown::Object({value_text})"))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object({value_text}.into_iter().map(|(key, value)| (key, {value_wrap})).collect())"
                ))
            }
            Some(Type::Dict(_, _) | Type::Class { .. }) => {
                Ok("SmeltUnknown::Object(::std::collections::HashMap::new())".to_owned())
            }
            Some(Type::Set(_) | Type::Tuple(_)) => Ok("SmeltUnknown::Array(Vec::new())".to_owned()),
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{value_text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(_)) => Ok("SmeltUnknown::Null".to_owned()),
            Some(Type::Future(_) | Type::Union(_)) => Ok("SmeltUnknown::Null".to_owned()),
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
        if kind == smelt_hir::UnknownKind::Null
            && matches!(
                self.mir.types.get(self.operand_ty(value)?),
                Some(Type::Optional(_))
            )
        {
            return Ok(format!("{text}.is_none()"));
        }
        self.unknown_is_text_raw(&text, kind)
    }

    /// Emits a runtime tag check for already-rendered `SmeltUnknown` text.
    pub(super) fn unknown_is_text_raw(
        &self,
        text: &str,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let pattern = match kind {
            smelt_hir::UnknownKind::Null => "SmeltUnknown::Null",
            smelt_hir::UnknownKind::Bool => "SmeltUnknown::Bool(_)",
            smelt_hir::UnknownKind::Number => "SmeltUnknown::Number(_)",
            smelt_hir::UnknownKind::String => "SmeltUnknown::String(_)",
            smelt_hir::UnknownKind::Function => return Ok("false".to_owned()),
            smelt_hir::UnknownKind::Array => "SmeltUnknown::Array(_)",
            smelt_hir::UnknownKind::Object => "SmeltUnknown::Object(_)",
        };
        Ok(format!("matches!({text}, {pattern})"))
    }

    /// Emits checked extraction from `SmeltUnknown` into a concrete Rust type.
    /// Emits checked extraction from `SmeltUnknown` into a concrete Rust type.
    pub(super) fn unknown_cast_text(
        &self,
        value: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let text = self.operand_text(value)?;
        self.unknown_cast_value_text(&text, target)
    }

    /// Emits checked extraction from an already-rendered `SmeltUnknown` value.
    pub(super) fn unknown_cast_value_text(
        &self,
        text: &str,
        target: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text.to_owned()),
            Some(Type::None) => Ok(format!(
                "if matches!({text}.clone(), SmeltUnknown::Null) {{ () }} else {{ panic!(\"unknown is not null\") }}"
            )),
            Some(Type::Bool) => Ok(format!(
                "if let SmeltUnknown::Bool(value) = {text}.clone() {{ value }} else {{ panic!(\"unknown is not boolean\") }}"
            )),
            Some(Type::Float) => Ok(format!(
                "if let SmeltUnknown::Number(value) = {text}.clone() {{ value }} else {{ panic!(\"unknown is not number\") }}"
            )),
            Some(Type::Int) => Ok(format!(
                "if let SmeltUnknown::Number(value) = {text}.clone() {{ value as i64 }} else {{ panic!(\"unknown is not number\") }}"
            )),
            Some(Type::String) => Ok(format!(
                "if let SmeltUnknown::String(value) = {text}.clone() {{ value }} else {{ panic!(\"unknown is not string\") }}"
            )),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!(
                    "if let SmeltUnknown::Array(value) = {text}.clone() {{ value }} else {{ panic!(\"unknown is not array\") }}"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "if let SmeltUnknown::Object(value) = {text}.clone() {{ value }} else {{ panic!(\"unknown is not object\") }}"
                ))
            }
            Some(Type::TypeParam { .. }) => {
                Ok(format!("IntoSmeltUnknown::into_smelt_unknown({text})"))
            }
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(
                Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::Tuple(_)
                | Type::Optional(_)
                | Type::Class { .. },
            ) => Ok("Default::default()".to_owned()),
            Some(Type::Function(_)) => self.default_value(target),
            Some(Type::Future(_)) => Ok("Default::default()".to_owned()),
            _ => Err(EmitError::new(
                "checked extraction from unknown to this type is not implemented yet",
            )),
        }
    }

    // Converts an awaited future operand without cloning it.
}
