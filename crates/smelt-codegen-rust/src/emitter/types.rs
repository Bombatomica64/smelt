//! Types emission helpers.

use super::*;
use crate::rust::RustIdent;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Render one parameter type from a function-typed value.
    ///
    /// `FunctionType::mutable_params` is a Rust ABI marker used for structural
    /// method fields that must preserve JavaScript object mutation through a
    /// callback boundary.
    pub(super) fn function_type_param_text(
        &self,
        function: &FunctionType,
        index: usize,
        param: TypeId,
        scoped_type_params: &HashSet<Symbol>,
    ) -> Result<String, EmitError> {
        let param_text =
            self.type_text_with_scoped_type_params(param, false, scoped_type_params)?;
        if function.mutable_params.contains(&index) {
            Ok(format!("&mut {param_text}"))
        } else {
            Ok(param_text)
        }
    }

    /// Converts a primitive source-language cast operation to Rust text.
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
            | (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Float)
            | (smelt_hir::PrimitiveCastOp::ToString, Type::String, Type::String) => {
                Ok(operand_text)
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Int) => {
                Ok(format!("{operand_text} != 0"))
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Float) => Ok(format!(
                "{{ let smelt_number = {operand_text}; smelt_number != 0.0 && !smelt_number.is_nan() }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::String) => {
                Ok(format!("!{operand_text}.is_empty()"))
            }
            (
                smelt_hir::PrimitiveCastOp::ToBool,
                Type::Bool,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(format!(
                "match {operand_text} {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Class { .. }) => {
                Ok("true".to_owned())
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Optional(inner)) => {
                self.optional_truthy_text(&operand_text, *inner)
            }
            (smelt_hir::PrimitiveCastOp::ToBool, Type::Bool, Type::Function(_)) => {
                Ok("true".to_owned())
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1_i64 }} else {{ 0_i64 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::Float) => {
                Ok(format!("({operand_text} as f64).trunc() as i64"))
            }
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Int, Type::String) => Ok(format!(
                "{operand_text}.parse::<i64>().expect(\"int() parse failed\")"
            )),
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Float, Type::String) => Ok(format!(
                "{operand_text}.parse::<i64>().map(|value| value as f64).unwrap_or(f64::NAN)"
            )),
            (smelt_hir::PrimitiveCastOp::ToInt, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::String)) =>
            {
                Ok(format!(
                    "{operand_text}.unwrap_or_default().parse::<i64>().map(|value| value as f64).unwrap_or(f64::NAN)"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1.0 }} else {{ 0.0 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Int) => {
                Ok(format!("{operand_text} as f64"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::String) => Ok(format!(
                "{operand_text}.parse::<f64>().expect(\"float() parse failed\")"
            )),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Float)) =>
            {
                Ok(format!("{operand_text}.unwrap_or(f64::NAN)"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Int)) =>
            {
                Ok(format!(
                    "{operand_text}.map_or(f64::NAN, |value| value as f64)"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                ) || self.is_erased_class_type(*inner) =>
            {
                Ok(format!(
                    "{operand_text}.map_or(f64::NAN, |value| match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }})"
                ))
            }
            (
                smelt_hir::PrimitiveCastOp::ToFloat,
                Type::Float,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(format!(
                "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Class { .. })
                if self.is_erased_class_type(operand_ty) =>
            {
                Ok(format!(
                    "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }}"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1.0 }} else {{ 0.0 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Int) => {
                Ok(format!("{operand_text} as f64"))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::String) => {
                Ok(self.js_number_from_string_text(&operand_text))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Float)) =>
            {
                Ok(format!("{operand_text}.unwrap_or(f64::NAN)"))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Int)) =>
            {
                Ok(format!(
                    "{operand_text}.map_or(f64::NAN, |value| value as f64)"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::String)) =>
            {
                let conversion = self.js_number_from_string_text("value");
                Ok(format!(
                    "{operand_text}.map_or(f64::NAN, |value| {conversion})"
                ))
            }
            (
                smelt_hir::PrimitiveCastOp::ToJsNumber,
                Type::Float,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(self.js_number_from_unknown_text(&operand_text)),
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                ) || self.is_erased_class_type(*inner) =>
            {
                let conversion = self.js_number_from_unknown_text("value");
                Ok(format!(
                    "{operand_text}.map_or(f64::NAN, |value| {conversion})"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Class { .. })
                if self.is_erased_class_type(operand_ty) =>
            {
                Ok(self.js_number_from_unknown_text(&operand_text))
            }
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
            (smelt_hir::PrimitiveCastOp::ToString, Type::String, _) => {
                self.string_like_operand_text(operand, "String")
            }
            (_, Type::Bool, _) => Ok("false".to_owned()),
            (_, Type::Int, _) => Ok("0_i64".to_owned()),
            (_, Type::Float, _) => Ok("0.0".to_owned()),
            (_, Type::String, _) => Ok("String::new()".to_owned()),
            (_, Type::Unknown | Type::Union(_) | Type::Never, _) => self.unknown_wrap_text(operand),
            _ => self.default_value(dest_ty),
        }
    }

    /// Emit JavaScript numeric conversion for a Rust string expression.
    fn js_number_from_string_text(&self, operand_text: &str) -> String {
        format!(
            "{{ let smelt_value = {operand_text}; let smelt_text = smelt_value.trim(); if smelt_text.is_empty() {{ 0.0 }} else {{ smelt_text.parse::<f64>().unwrap_or(f64::NAN) }} }}"
        )
    }

    /// Emit JavaScript numeric conversion for an erased runtime value.
    fn js_number_from_unknown_text(&self, operand_text: &str) -> String {
        let string_conversion = self.js_number_from_string_text("value");
        format!(
            "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => {string_conversion}, SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null => 0.0, SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }}"
        )
    }

    /// Converts an optional value to JavaScript truthiness for its contained type.
    pub(super) fn optional_truthy_text(
        &self,
        operand_text: &str,
        inner_ty: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(inner_ty) {
            Some(Type::Bool) => Ok(format!("{operand_text}.clone().unwrap_or(false)")),
            Some(Type::Int) => Ok(format!(
                "{operand_text}.clone().is_some_and(|value| value != 0)"
            )),
            Some(Type::Float) => Ok(format!(
                "{operand_text}.clone().is_some_and(|value| value != 0.0 && !value.is_nan())"
            )),
            Some(Type::String) => Ok(format!(
                "{operand_text}.clone().is_some_and(|value| !value.is_empty())"
            )),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never) => {
                Ok(format!(
                    "match {operand_text}.clone() {{ None => false, Some(SmeltUnknown::Null) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_)) => true }}"
                ))
            }
            Some(Type::Optional(inner)) => {
                let nested = self.optional_truthy_text("value", *inner)?;
                Ok(format!(
                    "match {operand_text}.clone() {{ Some(value) => {nested}, None => false }}"
                ))
            }
            Some(Type::None) => Ok("false".to_owned()),
            Some(Type::Class { .. })
            | Some(Type::Function(_))
            | Some(Type::List(_))
            | Some(Type::Tuple(_))
            | Some(Type::Dict(_, _))
            | Some(Type::Set(_))
            | Some(Type::Future(_)) => Ok(format!("{operand_text}.is_some()")),
            None => Err(EmitError::new("optional truthiness inner type is unknown")),
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
                    Some(Type::Optional(inner)) => {
                        if let Some(Type::Dict(_, value)) = self.mir.types.get(*inner) {
                            return self.type_id(Type::Optional(*value));
                        }
                        let Some(fields) = self.structural_record_fields(*inner) else {
                            return self.type_id(Type::Unknown);
                        };
                        fields
                            .into_iter()
                            .find(|record_field| record_field.name == *field)
                            .map(|record_field| {
                                if matches!(
                                    self.mir.types.get(record_field.ty),
                                    Some(Type::Optional(_))
                                ) {
                                    record_field.ty
                                } else {
                                    self.type_id(Type::Optional(record_field.ty))
                                        .unwrap_or(record_field.ty)
                                }
                            })
                            .ok_or_else(|| EmitError::new("optional record field is unknown"))
                    }
                    Some(Type::Class { name, .. }) => {
                        let field_ty = if let Some(class) =
                            self.mir.classes.iter().find(|class| class.name == *name)
                        {
                            crate::classes::effective_class_fields(self.mir, class)
                                .into_iter()
                                .find(|class_field| class_field.name == *field)
                                .map(|class_field| class_field.ty)
                        } else if let Some(interface) = self
                            .mir
                            .interfaces
                            .iter()
                            .find(|interface| interface.name == *name)
                        {
                            crate::classes::effective_interface_fields(self.mir, interface)
                                .into_iter()
                                .find(|interface_field| interface_field.name == *field)
                                .map(|interface_field| interface_field.ty)
                        } else {
                            None
                        };
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
                    Some(Type::Optional(inner)) => {
                        if let Some(Type::List(item)) = self.mir.types.get(*inner) {
                            return if matches!(self.mir.types.get(*item), Some(Type::Optional(_))) {
                                Ok(*item)
                            } else {
                                self.type_id(Type::Optional(*item))
                            };
                        }
                        self.type_id(Type::Unknown)
                    }
                    Some(Type::Dict(_, value)) => Ok(*value),
                    Some(Type::String) => self.type_id(Type::String),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => Ok(base_ty),
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

    /// Return the innermost non-optional type for defensive Rust `Option<T>` emission.
    pub(super) fn flatten_optional_inner(&self, mut ty: TypeId) -> TypeId {
        while let Some(Type::Optional(inner)) = self.mir.types.get(ty) {
            ty = *inner;
        }
        ty
    }

    /// Convert a function parameter type to Rust.
    ///
    /// Callback parameters are borrowed as reentrant functions so callers can
    /// forward the same callback through multiple helper calls without
    /// consuming it. Generated closure state is stored in shared cells rather
    /// than requiring `FnMut` receiver access.
    pub(super) fn param_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        if let Some(Type::Function(function)) = self.mir.types.get(ty) {
            let params = function
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    self.function_type_param_text(function, index, *param, &HashSet::new())
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let return_ty = self.type_text_with_impl_trait(function.return_ty, false)?;
            return Ok(format!("&dyn Fn({params}) -> {return_ty}"));
        }
        self.type_text(ty)
    }

    /// Convert a concrete function parameter declaration to Rust.
    pub(super) fn parameter_decl_type_text(&self, local: LocalId) -> Result<String, EmitError> {
        let ty = self.local_decl(local)?.ty;
        if self.parameter_needs_mutable_reference(local) {
            return Ok(format!(
                "&mut {}",
                self.type_text_with_impl_trait(ty, false)?
            ));
        }
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
        self.type_text_with_scoped_type_params(
            ty,
            allow_impl_trait,
            &self.current_function_type_params(),
        )
    }

    /// Return type parameters declared by the current class function scope.
    ///
    /// MIR does not model free-function generics yet, but class constructors
    /// and methods are emitted inside `impl<T> Class<T>` blocks. Their
    /// signatures must therefore keep class type parameters instead of erasing
    /// them to `SmeltUnknown`.
    pub(super) fn current_function_has_type_param(&self, param: Symbol) -> bool {
        self.current_function_type_params().contains(&param)
    }

    /// Return type parameters declared by the current class function scope.
    fn current_function_type_params(&self) -> HashSet<Symbol> {
        let class_name = match self.function.origin {
            HirOrigin::ClassConstructor { class, .. } | HirOrigin::ClassMethod { class, .. } => {
                class
            }
            HirOrigin::Body(_) => return HashSet::new(),
        };
        self.mir
            .classes
            .iter()
            .find(|class| class.name == class_name)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default()
    }

    /// Convert a type ID to Rust while preserving type parameters declared by
    /// the current storage item.
    ///
    /// Function-level generics are not represented in MIR yet, so unscoped type
    /// parameters still lower to `SmeltUnknown`. Class and interface storage,
    /// however, already declares Rust generic parameters; those positions should
    /// keep the generic shape instead of erasing fields to the runtime unknown
    /// carrier.
    pub(super) fn type_text_with_scoped_type_params(
        &self,
        ty: TypeId,
        allow_impl_trait: bool,
        scoped_type_params: &HashSet<Symbol>,
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
            Type::TypeParam { name } if scoped_type_params.contains(name) => self
                .symbol_name(*name)
                .map(|param_name| RustIdent::new(param_name).into_string()),
            Type::TypeParam { .. } => Ok("SmeltUnknown".to_owned()),
            Type::Class { name, args } => {
                if self.symbol_name(*name)? == "RegExp" {
                    return Ok("SmeltRegExp".to_owned());
                }
                if !self.mir.classes.iter().any(|class| class.name == *name)
                    && !self
                        .mir
                        .interfaces
                        .iter()
                        .any(|interface| interface.name == *name)
                {
                    return Ok("SmeltUnknown".to_owned());
                }
                let type_name = sanitize_ident(self.symbol_name(*name)?);
                if args.is_empty() {
                    Ok(type_name)
                } else {
                    let arg_text = args
                        .iter()
                        .map(|arg| {
                            self.type_text_with_scoped_type_params(*arg, false, scoped_type_params)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    Ok(format!("{type_name}<{arg_text}>"))
                }
            }
            Type::None => Ok("()".to_owned()),
            Type::List(item) => Ok(format!(
                "Vec<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Set(item) if self.type_is_hash_set_key_safe(*item) => Ok(format!(
                "::std::collections::HashSet<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Set(item) => Ok(format!(
                "Vec<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Dict(key, value) if self.dict_uses_smelt_record(*key) => Ok(format!(
                "SmeltRecord<String, {}>",
                self.type_text_with_scoped_type_params(*value, false, scoped_type_params)?
            )),
            Type::Dict(key, value) if self.dict_uses_js_key_map(*key) => Ok(format!(
                "SmeltJsMap<{}, {}>",
                self.type_text_with_scoped_type_params(*key, false, scoped_type_params)?,
                self.type_text_with_scoped_type_params(*value, false, scoped_type_params)?
            )),
            Type::Dict(key, value) => Ok(format!(
                "::std::collections::HashMap<{}, {}>",
                self.type_text_with_scoped_type_params(*key, false, scoped_type_params)?,
                self.type_text_with_scoped_type_params(*value, false, scoped_type_params)?
            )),
            Type::Tuple(items) => {
                let items_text = items
                    .iter()
                    .map(|item| {
                        self.type_text_with_scoped_type_params(*item, false, scoped_type_params)
                    })
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
                self.type_text_with_scoped_type_params(
                    self.flatten_optional_inner(*item),
                    false,
                    scoped_type_params,
                )?
            )),
            Type::Union(_) => Ok("SmeltUnknown".to_owned()),
            Type::Function(function) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok("SmeltErasedFunction".to_owned());
                }
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        self.function_type_param_text(function, index, *param, scoped_type_params)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                let return_ty = if function.may_throw
                    && let Some(Type::Future(item)) = self.mir.types.get(function.return_ty)
                {
                    format!(
                        "::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<{}, Box<dyn std::error::Error>>>>>",
                        self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
                    )
                } else if function.may_throw {
                    let inner_return_ty = self.type_text_with_scoped_type_params(
                        function.return_ty,
                        false,
                        scoped_type_params,
                    )?;
                    format!("Result<{inner_return_ty}, Box<dyn std::error::Error>>")
                } else {
                    self.type_text_with_scoped_type_params(
                        function.return_ty,
                        false,
                        scoped_type_params,
                    )?
                };
                if allow_impl_trait {
                    Ok(format!("impl Fn({params}) -> {return_ty}"))
                } else {
                    Ok(format!("::std::rc::Rc<dyn Fn({params}) -> {return_ty}>"))
                }
            }
            Type::Future(item) => Ok(format!(
                "::std::pin::Pin<Box<dyn ::std::future::Future<Output = {}>>>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
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
            Type::Set(item) if self.type_is_hash_set_key_safe(*item) => {
                Ok("::std::collections::HashSet::new()".to_owned())
            }
            Type::Set(_) => Ok("Vec::new()".to_owned()),
            Type::Dict(key, _) if self.dict_uses_smelt_record(*key) => {
                Ok("SmeltRecord::new()".to_owned())
            }
            Type::Dict(key, _) if self.dict_uses_js_key_map(*key) => {
                Ok("SmeltJsMap::new()".to_owned())
            }
            Type::Dict(_, _) => Ok("::std::collections::HashMap::new()".to_owned()),
            Type::Optional(inner) => Ok(format!(
                "None::<{}>",
                self.type_text_with_impl_trait(self.flatten_optional_inner(*inner), false)?
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
            Type::TypeParam { name } if self.current_function_has_type_param(*name) => {
                Ok("Default::default()".to_owned())
            }
            Type::TypeParam { .. } | Type::Union(_) => Ok("SmeltUnknown::Null".to_owned()),
            Type::Class { name, .. } if self.symbol_name(*name)? == "RegExp" => {
                Ok("SmeltRegExp::new(String::new(), String::new())".to_owned())
            }
            Type::Class { .. } => Ok("Default::default()".to_owned()),
            Type::Function(function) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok("SmeltErasedFunction { callback: ::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| SmeltUnknown::Null), length: 0.0, object: None }".to_owned());
                }
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
                let return_ty = if function.may_throw
                    && let Some(Type::Future(item)) = self.mir.types.get(function.return_ty)
                {
                    format!(
                        "::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<{}, Box<dyn std::error::Error>>>>>",
                        self.type_text_with_impl_trait(*item, false)?
                    )
                } else if function.may_throw {
                    format!(
                        "Result<{}, Box<dyn std::error::Error>>",
                        self.type_text_with_impl_trait(function.return_ty, false)?
                    )
                } else {
                    self.type_text_with_impl_trait(function.return_ty, false)?
                };
                let return_value = self.default_value(function.return_ty)?;
                let return_text = if function.may_throw
                    && let Some(Type::Future(item)) = self.mir.types.get(function.return_ty)
                {
                    let item_value = self.default_value(*item)?;
                    format!(
                        "Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({item_value}) }}) as {return_ty}"
                    )
                } else if function.may_throw {
                    format!("Ok::<_, Box<dyn std::error::Error>>({return_value})")
                } else {
                    return_value
                };
                let function_type = self.type_text_with_impl_trait(ty, false)?;
                Ok(format!(
                    "{{ let smelt_default_callback: {function_type} = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ {return_text} }}); smelt_default_callback }}"
                ))
            }
            Type::Future(item) => Ok(format!(
                "Box::pin(async move {{ {} }}) as {}",
                self.default_value(*item)?,
                self.type_text_with_impl_trait(ty, false)?
            )),
        }
    }

    /// Gets a default value while preserving explicitly scoped type parameters.
    pub(super) fn default_value_with_scoped_type_params(
        &self,
        ty: TypeId,
        scoped_type_params: &HashSet<Symbol>,
    ) -> Result<String, EmitError> {
        match self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?
        {
            Type::TypeParam { name } if scoped_type_params.contains(name) => {
                Ok("Default::default()".to_owned())
            }
            Type::Optional(inner) => Ok(format!(
                "None::<{}>",
                self.type_text_with_scoped_type_params(
                    self.flatten_optional_inner(*inner),
                    false,
                    scoped_type_params,
                )?
            )),
            Type::Function(function) => {
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
                                scoped_type_params,
                            )?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let return_ty = if function.may_throw {
                    format!(
                        "Result<{}, Box<dyn std::error::Error>>",
                        self.type_text_with_scoped_type_params(
                            function.return_ty,
                            false,
                            scoped_type_params,
                        )?
                    )
                } else {
                    self.type_text_with_scoped_type_params(
                        function.return_ty,
                        false,
                        scoped_type_params,
                    )?
                };
                let return_value = self.default_value_with_scoped_type_params(
                    function.return_ty,
                    scoped_type_params,
                )?;
                let body = if function.may_throw {
                    format!("Ok::<_, Box<dyn std::error::Error>>({return_value})")
                } else {
                    return_value
                };
                Ok(format!(
                    "{{ let smelt_default_callback: ::std::rc::Rc<dyn Fn({}) -> {}> = ::std::rc::Rc::new(move |{}| -> {} {{ {} }}); smelt_default_callback }}",
                    function
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| {
                            self.function_type_param_text(
                                function,
                                index,
                                *param,
                                scoped_type_params,
                            )
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", "),
                    return_ty,
                    params,
                    return_ty,
                    body
                ))
            }
            _ => self.default_value(ty),
        }
    }

    // Gets the entry block of the function.
}
