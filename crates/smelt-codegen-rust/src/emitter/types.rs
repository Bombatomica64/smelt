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
            _ => Err(EmitError::new(
                "primitive cast must convert bool, int, float, or string to the matching destination type",
            )),
        }
    }

    /// Converts a string trim operation to Rust text.
    /// Returns whether a type is supported by the current JSON serializer path.
    pub(super) fn is_json_serializable_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
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
                    Some(Type::Class { name, .. }) => self
                        .mir
                        .classes
                        .iter()
                        .find(|class| class.name == *name)
                        .and_then(|class| {
                            class
                                .fields
                                .iter()
                                .find(|class_field| class_field.name == *field)
                        })
                        .map(|class_field| class_field.ty)
                        .ok_or_else(|| EmitError::new("class field type lookup failed")),
                    _ => Err(EmitError::new(
                        "field type lookup is only implemented for dicts",
                    )),
                }
            }
            Place::Index { base, .. } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(item)) => Ok(*item),
                    Some(Type::Dict(_, value)) => Ok(*value),
                    Some(Type::String) => self.type_id(Type::String),
                    _ => Err(EmitError::new(
                        "index type lookup is only implemented for lists, strings, and dicts",
                    )),
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
            Type::Never => Err(EmitError::new(
                "never type has no Rust runtime representation",
            )),
            Type::TypeParam { name } | Type::Class { name, .. } => {
                Ok(sanitize_ident(self.symbol_name(*name)?))
            }
            Type::None => Ok("()".to_owned()),
            Type::List(item) => Ok(format!("Vec<{}>", self.type_text(*item)?)),
            Type::Set(item) => Ok(format!(
                "::std::collections::HashSet<{}>",
                self.type_text(*item)?
            )),
            Type::Dict(key, value) => Ok(format!(
                "::std::collections::HashMap<{}, {}>",
                self.type_text(*key)?,
                self.type_text(*value)?
            )),
            Type::Tuple(items) => {
                let items_text = items
                    .iter()
                    .map(|item| self.type_text(*item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if items.len() == 1 {
                    Ok(format!("({items_text},)"))
                } else {
                    Ok(format!("({items_text})"))
                }
            }
            Type::Optional(item) => Ok(format!("Option<{}>", self.type_text(*item)?)),
            Type::Union(_) => Err(EmitError::new("union type codegen is not implemented yet")),
            Type::Function(function) => {
                let params = function
                    .params
                    .iter()
                    .map(|param| self.type_text(*param))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!(
                    "impl FnMut({params}) -> {}",
                    self.type_text(function.return_ty)?
                ))
            }
            Type::Future(item) => Ok(format!(
                "::std::pin::Pin<Box<dyn ::std::future::Future<Output = {}>>>",
                self.type_text(*item)?
            )),
        }
    }

    /// Converts a function return type to Rust, including uncaught exception wrapping.
    /// Converts a function return type to Rust, including uncaught exception wrapping.
    pub(super) fn return_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        let inner = self.type_text(ty)?;
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
            Type::Never => Err(EmitError::new("never type has no default value")),
            Type::None => Ok("()".to_owned()),
            Type::List(_) => Ok("Vec::new()".to_owned()),
            Type::Set(_) => Ok("::std::collections::HashSet::new()".to_owned()),
            Type::Dict(_, _) => Ok("::std::collections::HashMap::new()".to_owned()),
            _ => Err(EmitError::new(
                "default value codegen is not implemented for this field type",
            )),
        }
    }

    // Gets the entry block of the function.
}
