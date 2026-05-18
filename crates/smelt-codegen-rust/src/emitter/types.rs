//! Types emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a primitive Python-style cast operation to Rust text.
    pub(super) fn primitive_cast_text(
        &self,
        op: smelt_hir::PrimitiveCastOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        let operand_type = self
            .mir
            .types
            .get(operand_ty)
            .ok_or_else(|| EmitError::new("primitive cast operand has unknown type"))?;
        let dest_type = self
            .mir
            .types
            .get(dest_ty)
            .ok_or_else(|| EmitError::new("primitive cast destination has unknown type"))?;
        let operand_text = self.operand_text(operand)?;
        match (op, dest_type, operand_type) {
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Bool)
            | (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Int)
            | (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Float)
            | (smelt_hir::PrimitiveCastOp::ToString, Type::String, Type::String) => {
                Ok(operand_text)
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Int) => {
                Ok(format!("{operand_text} != 0"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Float) => {
                Ok(format!("{operand_text} != 0.0"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::String) => {
                Ok(format!("!{operand_text}.is_empty()"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Unknown) => Ok(format!(
                "match {operand_text} {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => true }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Optional(_)) => {
                Ok(format!("{operand_text}.is_some()"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Function(_)) => {
                Ok("true".to_owned())
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1_i64 }} else {{ 0_i64 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Float) => {
                Ok(format!("{operand_text}.trunc() as i64"))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::String) => Ok(format!(
                "{operand_text}.parse::<i64>().expect(\"int() parse failed\")"
            )),
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Float, Type::String) => Ok(format!(
                "({operand_text}.parse::<i64>().expect(\"int() parse failed\") as f64)"
            )),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1.0 }} else {{ 0.0 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Int) => {
                Ok(format!("{operand_text} as f64"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::String) => Ok(format!(
                "{operand_text}.parse::<f64>().expect(\"float() parse failed\")"
            )),
            (smelt_hir::PrimitiveCastOp::ToString, Type::String, Type::Bool) => Ok(format!(
                "if {operand_text} {{ \"True\".to_owned() }} else {{ \"False\".to_owned() }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToString, Type::String, Type::Int | Type::Float) => {
                Ok(format!("{operand_text}.to_string()"))
            }
            (smelt_hir::PrimitiveCastOp::ToString, Type::String, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Bool | Type::Int | Type::Float | Type::String)
                ) =>
            {
                Ok(format!("{operand_text}.unwrap_or_default().to_string()"))
            }
            (_, Type::Bool, _) => Ok("false".to_owned()),
            (_, Type::Int, _) => Ok("0_i64".to_owned()),
            (_, Type::Float, _) => Ok("0.0".to_owned()),
            (_, Type::String, _) => Ok("String::new()".to_owned()),
            (_, Type::Unknown | Type::Union(_) | Type::Never, _) => self.unknown_wrap_text(operand),
            _ => self.default_value(dest_ty),
        }
    }

    /// Convert an operand to a Rust boolean using source-language truthiness.
    pub(super) fn truthy_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let bool_ty = self.type_id(Type::Bool)?;
        self.primitive_cast_text(smelt_hir::PrimitiveCastOp::ToBool, operand, bool_ty)
    }

    /// Converts a string trim operation to Rust text.
    /// Returns whether a type is supported by the current JSON serializer path.
    pub(super) fn is_json_serializable_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
                self.is_json_serializable_type(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.is_json_serializable_type(*item)),
            Some(Type::Dict(key, value)) => {
                matches!(self.mir.types.get(*key), Some(Type::String))
                    && self.is_json_serializable_type(*value)
            }
            Some(Type::Class { name, .. }) => {
                if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
                    crate::classes::effective_class_fields(self.mir, class)
                        .iter()
                        .all(|field| self.is_json_serializable_type(field.ty))
                } else {
                    self.mir
                        .interfaces
                        .iter()
                        .find(|interface| interface.name == *name)
                        .is_some_and(|interface| {
                            interface
                                .fields
                                .iter()
                                .all(|field| self.is_json_serializable_type(field.ty))
                        })
                }
            }
            _ => false,
        }
    }

    /// Converts a blocking HTTP GET operation to Rust text.
    /// Gets the type of a place.
    pub(super) fn place_ty(&self, place: &Place) -> Result<TypeId, EmitError> {
        match place {
            Place::Local(local) => Ok(self.local_decl(*local)?.ty),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::Dict(_, value)) => Ok(*value),
                    Some(Type::Class { name, .. }) => {
                        let Some(class) = self.mir.classes.iter().find(|class| class.name == *name)
                        else {
                            return self.type_id(Type::Unknown);
                        };
                        let field_ty = crate::classes::effective_class_fields(self.mir, class)
                            .into_iter()
                            .find(|class_field| class_field.name == *field)
                            .map(|class_field| class_field.ty);
                        match field_ty {
                            Some(ty) => Ok(ty),
                            None => self.type_id(Type::Unknown),
                        }
                    }
                    _ => self.type_id(Type::Unknown),
                }
            }
            Place::Index { base, .. } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(item)) => Ok(*item),
                    Some(Type::Dict(_, value)) => Ok(*value),
                    Some(Type::String) => self.type_id(Type::String),
                    _ => self.type_id(Type::Unknown),
                }
            }
        }
    }

    /// Converts a Python-style element index into a Rust `usize` expression.
    ///
    /// Negative indexes are offset from the collection length. Bounds are not
    /// clamped because Python element indexing raises when the normalized index
    /// is still outside the collection; the generated Rust keeps that behavior
    /// with `expect` on negative conversion and the eventual indexed lookup.
    /// Finds the type ID for a given type.
    pub(super) fn type_id(&self, needle: Type) -> Result<TypeId, EmitError> {
        let index = self
            .mir
            .types
            .all()
            .iter()
            .position(|ty| *ty == needle)
            .ok_or_else(|| EmitError::new("type table does not contain literal operand type"))?;
        Ok(TypeId(compact_index(index, "type index does not fit u32")?))
    }

    /// Converts a type ID to its Rust text representation.
    /// Converts a type ID to its Rust text representation.
    pub(super) fn type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        self.type_text_with_impl_trait(ty, true)
    }

    /// Convert a function parameter type to Rust.
    ///
    /// Callback parameters are borrowed mutably so callers can forward the same
    /// callback through multiple helper calls without consuming it. Returned
    /// functions and nested function values still use owned boxes because
    /// references would not be valid value representations there.
    pub(super) fn param_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        if let Some(Type::Function(function)) = self.mir.types.get(ty) {
            let params = function
                .params
                .iter()
                .map(|param| self.type_text_with_impl_trait(*param, false))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
            return Ok(format!("&mut dyn FnMut({params}) -> {return_ty}"));
        }
        self.type_text(ty)
    }

    /// Convert a concrete function parameter declaration to Rust.
    pub(super) fn parameter_decl_type_text(&self, local: LocalId) -> Result<String, EmitError> {
        let ty = self.local_decl(local)?.ty;
        if matches!(self.mir.types.get(ty), Some(Type::Function(_))) {
            if !self.function_parameter_requires_owned(local)? {
                return self.param_type_text(ty);
            }
            return self.type_text_with_impl_trait(ty, false);
        }
        self.param_type_text(ty)
    }

    /// Convert a type ID to Rust, controlling whether root `impl Trait` is legal.
    pub(super) fn type_text_with_impl_trait(
        &self,
        ty: TypeId,
        allow_impl_trait: bool,
    ) -> Result<String, EmitError> {
        let resolved_ty = self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?;
        match resolved_ty {
            Type::Bool => Ok("bool".to_owned()),
            Type::Int => Ok("i64".to_owned()),
            Type::Float => Ok("f64".to_owned()),
            Type::String => Ok("String".to_owned()),
            Type::Unknown => Ok("SmeltUnknown".to_owned()),
            Type::Never => Ok("SmeltUnknown".to_owned()),
            Type::TypeParam { .. } => Ok("SmeltUnknown".to_owned()),
            Type::Class { name, args } => {
                if !self.mir.classes.iter().any(|class| class.name == *name)
                    && !self
                        .mir
                        .interfaces
                        .iter()
                        .any(|interface| interface.name == *name)
                {
                    return Ok("SmeltUnknown".to_owned());
                }
                let name = sanitize_ident(self.symbol_name(*name)?);
                if args.is_empty() {
                    Ok(name)
                } else {
                    let args = args
                        .iter()
                        .map(|arg| self.type_text_with_impl_trait(*arg, false))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    Ok(format!("{name}<{args}>"))
                }
            }
            Type::None => Ok("()".to_owned()),
            Type::List(item) => Ok(format!(
                "Vec<{}>",
                self.type_text_with_impl_trait(*item, false)?
            )),
            Type::Set(item) => Ok(format!(
                "::std::collections::HashSet<{}>",
                self.type_text_with_impl_trait(*item, false)?
            )),
            Type::Dict(key, value) => Ok(format!(
                "::std::collections::HashMap<{}, {}>",
                self.type_text_with_impl_trait(*key, false)?,
                self.type_text_with_impl_trait(*value, false)?
            )),
            Type::Tuple(items) => {
                let items_text = items
                    .iter()
                    .map(|item| self.type_text_with_impl_trait(*item, false))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if items.len() == 1 {
                    Ok(format!("({items_text},)"))
                } else {
                    Ok(format!("({items_text})"))
                }
            }
            Type::Optional(item) => Ok(format!(
                "Option<{}>",
                self.type_text_with_impl_trait(*item, false)?
            )),
            Type::Union(_) => Ok("SmeltUnknown".to_owned()),
            Type::Function(function) => {
                let params = function
                    .params
                    .iter()
                    .map(|param| self.type_text_with_impl_trait(*param, false))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
                if allow_impl_trait {
                    Ok(format!("impl FnMut({params}) -> {return_ty}"))
                } else {
                    Ok(format!(
                        "::std::rc::Rc<::std::cell::RefCell<dyn FnMut({params}) -> {return_ty}>>"
                    ))
                }
            }
            Type::Future(item) => Ok(format!(
                "::std::pin::Pin<Box<dyn ::std::future::Future<Output = {}>>>",
                self.type_text_with_impl_trait(*item, false)?
            )),
        }
    }

    /// Converts a function return type to Rust, including uncaught exception wrapping.
    /// Converts a function return type to Rust, including uncaught exception wrapping.
    pub(super) fn return_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        let inner = self.type_text_with_impl_trait(ty, false)?;
        if self.function.can_throw {
            Ok(format!("Result<{inner}, Box<dyn std::error::Error>>"))
        } else {
            Ok(inner)
        }
    }

    /// Gets the default value for a given type.
    /// Gets the default value for a given type.
    pub(super) fn default_value(&self, ty: TypeId) -> Result<String, EmitError> {
        match self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?
        {
            Type::Bool => Ok("false".to_owned()),
            Type::Int => Ok("0".to_owned()),
            Type::Float => Ok("0.0".to_owned()),
            Type::String => Ok("String::new()".to_owned()),
            Type::Unknown => Ok("SmeltUnknown::Null".to_owned()),
            Type::Never => Ok("SmeltUnknown::Null".to_owned()),
            Type::None => Ok("()".to_owned()),
            Type::List(_) => Ok("Vec::new()".to_owned()),
            Type::Set(_) => Ok("::std::collections::HashSet::new()".to_owned()),
            Type::Dict(_, _) => Ok("::std::collections::HashMap::new()".to_owned()),
            Type::Optional(inner) => Ok(format!(
                "None::<{}>",
                self.type_text_with_impl_trait(*inner, false)?
            )),
            Type::Tuple(items) => {
                let items_text = items
                    .iter()
                    .map(|item| self.default_value(*item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if items.len() == 1 {
                    Ok(format!("({items_text},)"))
                } else {
                    Ok(format!("({items_text})"))
                }
            }
            Type::Class { .. } | Type::TypeParam { .. } | Type::Union(_) => {
                Ok("Default::default()".to_owned())
            }
            Type::Function(function) => {
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
                let return_text = self.default_value(function.return_ty)?;
                let function_type = self.type_text_with_impl_trait(ty, false)?;
                Ok(format!(
                    "{{ let smelt_default_callback: {function_type} = ::std::rc::Rc::new(::std::cell::RefCell::new(move |{params}| {return_text})); smelt_default_callback }}"
                ))
            }
            Type::Future(_) => Ok("Default::default()".to_owned()),
        }
    }

    // Gets the entry block of the function.
}
