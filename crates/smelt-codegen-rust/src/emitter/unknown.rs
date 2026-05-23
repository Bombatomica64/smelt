//! Unknown emission helpers.

use super::*;
use smelt_hir::FunctionType;

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
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_text("key", *key)?;
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object({text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect())"
                ))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(self.operand_ty(operand)?) =>
            {
                Ok(text)
            }
            Some(Type::Class { name, .. }) => self.class_unknown_object_text(&text, *name),
            Some(Type::Set(_)) => Ok("SmeltUnknown::Array(Vec::new())".to_owned()),
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.unknown_wrap_value_text(&format!("{text}.{index}.clone()"), *item)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("SmeltUnknown::Array(vec![{values}])"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(_)) => {
                let adapter = self
                    .rest_vector_unknown_adapter_text(operand)?
                    .unwrap_or_else(|| "::std::rc::Rc::new(::std::cell::RefCell::new(move |_smelt_args: Vec<SmeltUnknown>| SmeltUnknown::Null))".to_owned());
                Ok(format!("SmeltUnknown::Function({adapter})"))
            }
            Some(Type::Union(_)) => Ok(text),
            Some(Type::Never | Type::Future(_)) | None => Ok("SmeltUnknown::Null".to_owned()),
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
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_text("key", *key)?;
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object({value_text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect())"
                ))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(ty) => Ok(value_text.to_owned()),
            Some(Type::Class { name, .. }) => self.class_unknown_object_text(value_text, *name),
            Some(Type::Set(_)) => Ok("SmeltUnknown::Array(Vec::new())".to_owned()),
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.unknown_wrap_value_text(
                            &format!("{value_text}.{index}.clone()"),
                            *item,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("SmeltUnknown::Array(vec![{values}])"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{value_text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(function)) => {
                let args = self.function_args_from_smelt_args_text(function)?;
                let call_text = format!("(&mut *smelt_function_value.borrow_mut())({args})");
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
                        self.unknown_wrap_value_text(&format!("{call_text}?"), function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                } else {
                    let value = self.unknown_wrap_value_text(&call_text, function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                };
                Ok(format!(
                    "{{ let smelt_function_value = {value_text}; SmeltUnknown::Function(::std::rc::Rc::new(::std::cell::RefCell::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}))) }}"
                ))
            }
            Some(Type::Union(_)) => Ok(value_text.to_owned()),
            Some(Type::Future(_)) => Ok("SmeltUnknown::Null".to_owned()),
        }
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
        name: Symbol,
    ) -> Result<String, EmitError> {
        let fields = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
            .or_else(|| {
                self.mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == name)
                    .map(|interface| {
                        crate::classes::effective_interface_fields(self.mir, interface)
                    })
            })
            .unwrap_or_default();

        let entries = fields
            .iter()
            .map(|field| {
                let source_name = self.symbol_source_name(field.name)?;
                let field_name = sanitize_ident(self.symbol_name(field.name)?);
                let field_value = self.unknown_wrap_value_text(
                    &format!("smelt_object_value.{field_name}"),
                    field.ty,
                )?;
                Ok(format!("({source_name:?}.to_owned(), {field_value})"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");

        Ok(format!(
            "{{ let smelt_object_value = {value_text}; SmeltUnknown::Object(::std::collections::HashMap::from([{entries}])) }}"
        ))
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
        if text == "Default::default()" {
            return match self.mir.types.get(target) {
                Some(Type::None) => Ok("()".to_owned()),
                Some(Type::Bool) => Ok("false".to_owned()),
                Some(Type::Float) => Ok("0.0".to_owned()),
                Some(Type::Int) => Ok("0_i64".to_owned()),
                Some(Type::String) => Ok("String::new()".to_owned()),
                Some(Type::List(_)) => Ok("Vec::new()".to_owned()),
                Some(Type::Dict(_, _)) => Ok("::std::collections::HashMap::new()".to_owned()),
                Some(Type::Optional(_)) => Ok("None".to_owned()),
                _ => self.default_value(target),
            };
        }
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text.to_owned()),
            Some(Type::List(_)) if text.contains(".concat(") => Ok(text.to_owned()),
            Some(Type::None) => Ok(format!(
                "if matches!({text}.clone(), SmeltUnknown::Null) {{ () }} else {{ panic!(\"unknown is not null\") }}"
            )),
            Some(Type::Bool) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            Some(Type::Float) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN }}"
            )),
            Some(Type::Int) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value as i64, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value {{ 1_i64 }} else {{ 0_i64 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => 0_i64 }}"
            )),
            Some(Type::String) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned() }}"
            )),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(value) => value, SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), _ => panic!(\"unknown is not iterable\") }}"
                ))
            }
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::String) => {
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| if let SmeltUnknown::String(value) = value {{ value }} else {{ value.to_string() }}).collect::<Vec<_>>(), SmeltUnknown::String(value) => value.chars().map(|ch| ch.to_string()).collect::<Vec<_>>(), _ => panic!(\"unknown is not iterable\") }}"
                ))
            }
            Some(Type::List(item)) => {
                let item_text = self.unknown_cast_value_text("value", *item)?;
                Ok(format!(
                    "if let SmeltUnknown::Array(values) = {text}.clone() {{ values.into_iter().map(|value| {item_text}).collect::<Vec<_>>() }} else {{ panic!(\"unknown is not array\") }}"
                ))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(value) => value, SmeltUnknown::Function(value) => ::std::collections::HashMap::from([(\"__smelt_call\".to_owned(), SmeltUnknown::Function(value))]), _ => ::std::collections::HashMap::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) =>
            {
                let item_text = self.unknown_cast_value_text("value", *item)?;
                Ok(format!(
                    "if let SmeltUnknown::Object(values) = ({text}).into_smelt_unknown() {{ values.into_iter().map(|(key, value)| (key, {item_text})).collect::<::std::collections::HashMap<_, _>>() }} else {{ ::std::collections::HashMap::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_text =
                    self.rendered_value_as_type_text("key", self.type_id(Type::String)?, *key)?;
                let item_text = self.unknown_cast_value_text("value", *item)?;
                Ok(format!(
                    "if let SmeltUnknown::Object(values) = {text}.clone() {{ values.into_iter().map(|(key, value)| ({key_text}, {item_text})).collect::<::std::collections::HashMap<_, _>>() }} else {{ ::std::collections::HashMap::new() }}"
                ))
            }
            Some(Type::TypeParam { .. }) => Ok(format!("({text}).into_smelt_unknown()")),
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(Type::Optional(inner)) => {
                let inner_text = self.unknown_cast_value_text(text, *inner)?;
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
                        self.unknown_cast_value_text(&value, *item)
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
            Some(Type::Set(_) | Type::Dict(_, _) | Type::Class { .. }) => {
                Ok("Default::default()".to_owned())
            }
            Some(Type::Function(function)) => {
                let target_text = self.type_text_with_impl_trait(target, false)?;
                let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        Ok(format!(
                            "arg{index}: {}",
                            self.type_text_with_impl_trait(*param, false)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let args = self.unknown_function_call_args_text(function)?;
                let call_text = if function.may_throw {
                    format!("(&mut *smelt_function.borrow_mut())({args})?")
                } else {
                    format!(
                        "(&mut *smelt_function.borrow_mut())({args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                    )
                };
                let converted_return_text = if return_ty == "SmeltUnknown" {
                    "smelt_result".to_owned()
                } else {
                    self.unknown_cast_value_text("smelt_result", function.return_ty)?
                };
                let return_text = if function.may_throw {
                    format!("Ok::<_, Box<dyn std::error::Error>>({converted_return_text})")
                } else {
                    converted_return_text
                };
                let default_callback = self.default_value(target)?;
                Ok(format!(
                    "{{ let smelt_function = match {text}.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(mut smelt_object) => match smelt_object.remove(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ let smelt_callback: {target_text} = ::std::rc::Rc::new(::std::cell::RefCell::new(move |{params}| -> {return_ty} {{ let smelt_result = {call_text}; {return_text} }})); smelt_callback }} else {{ {default_callback} }} }}"
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
                    self.unknown_wrap_value_text("value", *item_ty)?
                };
                statements.push(format!(
                    "smelt_call_args.extend(arg{index}.clone().into_iter().map(|value| {item_text}));"
                ));
            } else {
                let item_text = self.unknown_wrap_value_text(&format!("arg{index}"), *param_ty)?;
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
