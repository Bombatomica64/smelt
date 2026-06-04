//! Unknown emission helpers.

use super::*;
use crate::rust::RustIdent;
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
                Ok(format!("SmeltUnknown::Array({text}.into())"))
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
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record(({text}).clone()))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
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
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::new({text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))"
                ))
            }
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp" => {
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
            Some(Type::Set(_)) => Ok("SmeltUnknown::Array(Vec::new().into())".to_owned()),
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.unknown_wrap_value_text(&format!("{text}.{index}.clone()"), *item)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("SmeltUnknown::Array(vec![{values}].into())"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(_)) => {
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
                Ok(format!("SmeltUnknown::Array({value_text}.into())"))
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
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record(({value_text}).clone()))"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
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
                let value_wrap = self.unknown_wrap_value_text("value", *item)?;
                Ok(format!(
                    "SmeltUnknown::Object(SmeltObject::new({value_text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))"
                ))
            }
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp" => {
                Ok(format!("({value_text}).clone().into_smelt_unknown()"))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(ty) && self.symbol_name(*name)? == "Date" =>
            {
                Ok(self.date_unknown_identity_text(value_text))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(ty) => Ok(value_text.to_owned()),
            Some(Type::Class { .. }) => self.class_unknown_object_text(value_text, ty),
            Some(Type::Set(_)) => Ok("SmeltUnknown::Array(Vec::new().into())".to_owned()),
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
                Ok(format!("SmeltUnknown::Array(vec![{values}].into())"))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.unknown_wrap_value_text("value", *inner)?;
                Ok(format!(
                    "{value_text}.clone().map_or(SmeltUnknown::Null, |value| {value_wrap})"
                ))
            }
            Some(Type::Function(function)) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok(format!("{value_text}.clone().into_smelt_unknown()"));
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
                        self.unknown_wrap_value_text(&format!("{call_text}?"), function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                } else {
                    let value = self.unknown_wrap_value_text(&call_text, function.return_ty)?;
                    format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
                };
                Ok(format!(
                    "{{ let smelt_function_origin = {value_text}.clone(); let smelt_function_value = {value_text}; let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); SmeltUnknown::Function(smelt_erased_function) }}"
                ))
            }
            Some(Type::Union(_)) => Ok(value_text.to_owned()),
            Some(Type::Future(_)) => Ok("SmeltUnknown::Null".to_owned()),
        }
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
            .map(|field| {
                let source_name = self.symbol_source_name(field.name)?;
                let field_name = sanitize_ident(self.symbol_name(field.name)?);
                if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                    let field_value = self.unknown_wrap_value_text("value", *inner)?;
                    return Ok(format!(
                        "if let Some(value) = smelt_object_value.{field_name}.clone() {{ smelt_object_entries.insert({source_name:?}.to_owned(), {field_value}); }}"
                    ));
                }
                let field_value = if let Some(value) =
                    self.virtual_method_storage_field_text(target, target, field.name)?
                {
                    self.unknown_wrap_value_text(&value, field.ty)?
                } else {
                    self.unknown_wrap_value_text(
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

        Ok(format!(
            "{{ let smelt_object_value = {value_text}; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = ::std::collections::HashMap::new(); {entries} SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }}"
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
        if matches!(
            self.mir.types.get(self.operand_ty(value)?),
            Some(Type::Optional(_))
        ) {
            if kind == smelt_hir::UnknownKind::Null {
                return Ok(format!("{text}.is_none()"));
            }
            let check = self.unknown_is_text_raw("smelt_value", kind)?;
            return Ok(format!(
                "{text}.as_ref().is_some_and(|smelt_value| {check})"
            ));
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
            Some(Type::None) => Ok(format!(
                "if matches!({text}.clone(), SmeltUnknown::Null) {{ () }} else {{ panic!(\"unknown is not null\") }}"
            )),
            Some(Type::Bool) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            Some(Type::Float) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }}"
            )),
            Some(Type::Int) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value {{ 1_i64 }} else {{ 0_i64 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => 0_i64 }}"
            )),
            Some(Type::String) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned() }}"
            )),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown) => {
                Ok(format!(
                    "match {text}.clone() {{ SmeltUnknown::Array(value) => value.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), _ => panic!(\"unknown is not iterable\") }}"
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
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(value) => SmeltRecord::with_id_from_entries(value.id, value.into_iter()), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect(), SmeltUnknown::Function(value) => SmeltRecord::from([(\"__smelt_call\".to_owned(), SmeltUnknown::Function(value))]), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let item_text = self.unknown_cast_value_text("value", *item)?;
                Ok(format!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => SmeltRecord::with_id_from_entries(values.id, values.into_iter().map(|(key, value)| (key, {item_text}))), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), {item_text})).collect(), _ => SmeltRecord::new() }}"
                ))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_text =
                    self.rendered_value_as_type_text("key", self.type_id(Type::String)?, *key)?;
                let item_text = self.unknown_cast_value_text("value", *item)?;
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
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp" => Ok(format!(
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
                    self.unknown_wrap_value_text("value", *item_ty)?
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
                let item_text = self.unknown_wrap_value_text(&arg_text, *param_ty)?;
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
