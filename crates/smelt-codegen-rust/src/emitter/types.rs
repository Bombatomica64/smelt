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
        // A concrete union stores a tagged `SmeltUnion…` enum, but the erased JS
        // coercion arms below (`Type::Union(_)`) match over `SmeltUnknown`
        // discriminants. Project a concrete-union operand back to its erased value
        // so those arms see the `SmeltUnknown` shape they expect. Non-concrete
        // unions are already stored erased, so this is a no-op for them.
        let operand_text = self.erase_concrete_union_text(&self.operand_text(operand)?, operand_ty);
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
                "match {operand_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}"
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
                Ok(format!("({operand_text} as f64)"))
            }
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::String) => Ok(format!(
                "{operand_text}.parse::<f64>().expect(\"float() parse failed\")"
            )),
            (smelt_hir::PrimitiveCastOp::ParseFloat, Type::Float, Type::String) => Ok(format!(
                "{operand_text}.parse::<f64>().unwrap_or(f64::NAN)"
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
                    "{operand_text}.map_or(f64::NAN, |value| match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }})"
                ))
            }
            (
                smelt_hir::PrimitiveCastOp::ToFloat,
                Type::Float,
                Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never,
            ) => Ok(format!(
                "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
            )),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Float, Type::Class { .. })
                if self.is_erased_class_type(operand_ty) =>
            {
                Ok(format!(
                    "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
                ))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Bool) => {
                Ok(format!("if {operand_text} {{ 1.0 }} else {{ 0.0 }}"))
            }
            (smelt_hir::PrimitiveCastOp::ToJsNumber, Type::Float, Type::Int) => {
                Ok(format!("({operand_text} as f64)"))
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
            (_, Type::Unknown | Type::Union(_) | Type::Never, _) => self.erase(operand),
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
            "match {operand_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => {string_conversion}, SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null => 0.0, SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
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
            Some(Type::Union(_)) => {
                // A concrete generated union (`SmeltUnionNNNN`) is not a
                // `SmeltUnknown`, so its inner value cannot be matched against
                // `SmeltUnknown::` arms directly. Erase it through the union's
                // `IntoSmeltUnknown` boundary adapter first — truthiness is a
                // genuine runtime-narrowing inspection of the tagged value, and
                // only the resulting `bool` escapes, so no static shape is lost.
                Ok(format!(
                    "match {operand_text}.clone().map(|value| value.into_smelt_unknown()) {{ None => false, Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_)) => true }}"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never) => {
                Ok(format!(
                    "match {operand_text}.clone() {{ None => false, Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_)) => true }}"
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
            Some(Type::Generator { .. }) => Ok(format!("{operand_text}.is_some()")),
            Some(Type::GeneratorResult { .. }) => Ok(format!("{operand_text}.is_some()")),
            None => Err(EmitError::new("optional truthiness inner type is unknown")),
        }
    }

    /// Convert an operand to a Rust boolean using source-language truthiness.
    pub(super) fn truthy_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let bool_ty = self.type_id(Type::Bool)?;
        self.primitive_cast_text(smelt_hir::PrimitiveCastOp::ToBool, operand, bool_ty)
    }

    /// Convert an already-rendered value expression to a Rust boolean using
    /// JavaScript/Python truthiness rules.
    ///
    /// This is the text-based counterpart to the `ToBool` arm of
    /// [`Self::primitive_cast_text`]: callers that hold a rendered expression
    /// (rather than an [`Operand`]) — such as an array predicate callback result —
    /// coerce it here. `value_text` is evaluated exactly once. Objects, arrays,
    /// functions, class instances, and other reference values are always truthy;
    /// primitives follow their per-type emptiness/zero rules; optionals defer to
    /// [`Self::optional_truthy_text`].
    pub(super) fn value_truthy_text(
        &self,
        value_text: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(ty) {
            Some(Type::Bool) => Ok(value_text.to_owned()),
            Some(Type::Int) => Ok(format!("({value_text}) != 0")),
            Some(Type::Float) => Ok(format!(
                "{{ let smelt_number = ({value_text}); smelt_number != 0.0 && !smelt_number.is_nan() }}"
            )),
            Some(Type::String) => Ok(format!("!({value_text}).is_empty()")),
            Some(Type::Union(_)) => {
                // A concrete generated union is not a `SmeltUnknown`; erase it
                // through its `IntoSmeltUnknown` boundary adapter before the
                // runtime-narrowing truthiness match (see `optional_truthy_text`).
                Ok(format!(
                    "match ({value_text}).into_smelt_unknown() {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never) => {
                Ok(format!(
                    "match ({value_text}) {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}"
                ))
            }
            Some(Type::Optional(inner)) => self.optional_truthy_text(value_text, *inner),
            Some(Type::None) => Ok(format!("{{ let _ = ({value_text}); false }}")),
            Some(
                Type::Class { .. }
                | Type::Function(_)
                | Type::List(_)
                | Type::Tuple(_)
                | Type::Dict(_, _)
                | Type::Set(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            ) => Ok(format!("{{ let _ = ({value_text}); true }}")),
            None => Err(EmitError::new("predicate result type is unknown")),
        }
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
                // A `.length` read on a CONCRETE collection (typed list or set)
                // renders as `({base}.len() as f64)` (see the matching arm in
                // `place::field_read_text`), so its value type is `Float`, not the
                // erased `Unknown` the struct-field fallback would otherwise
                // report. Keeping the resolved type aligned with the rendered
                // expression lets callers coerce from the concrete `f64` instead
                // of treating an already-concrete value as erased.
                if matches!(self.mir.types.get(base_ty), Some(Type::List(_) | Type::Set(_)))
                    && smelt_stdlib::typescript_field_rule(self.symbol_source_name(*field)?)
                        == Some(smelt_stdlib::FieldRule::TsLength)
                {
                    return self.type_id(Type::Float);
                }
                if let Some((_, descriptor)) = self.descriptor_for_field(base_ty, *field) {
                    return Ok(descriptor.read_ty);
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(kind) = self.match_class_kind(*name)?
                {
                    return self.match_field_ty(kind, *field);
                }
                // A concrete builtin `RegExp` receiver exposes data properties
                // with statically known concrete field types backed by the
                // generated `SmeltRegExp` runtime shape. Resolve them here so
                // the field operand's type agrees with `regexp_field_text` and
                // `field_access_type`; without this the generic class arm below
                // (which only knows user classes/interfaces) would erase the
                // read to `Unknown` and make callers re-coerce an already
                // concrete value.
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_regexp_class_symbol(*name)?
                {
                    return match self.symbol_name(*field)? {
                        "source" | "flags" => self.type_id(Type::String),
                        "global" | "ignoreCase" | "ignore_case" | "multiline" | "sticky"
                        | "unicode" | "dotAll" | "dot_all" => self.type_id(Type::Bool),
                        "lastIndex" | "last_index" => self.type_id(Type::Float),
                        _ => self.type_id(Type::Unknown),
                    };
                }
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
                            // An undeclared member on an index-signature class is
                            // a keyed store read; its type is the store's value
                            // type `T` (issue #84), not an erased `Unknown`.
                            None => match self.class_index_store_types(base_ty) {
                                Some((_key_ty, value_ty)) => Ok(value_ty),
                                None => self.type_id(Type::Unknown),
                            },
                        }
                    }
                    _ => self.type_id(Type::Unknown),
                }
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_match_class_symbol(*name)?
                {
                    // A numbered group read is an optional string.
                    let string_ty = self.type_id(Type::String)?;
                    return self.type_id(Type::Optional(string_ty));
                }
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
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        items
                            .get(tuple_index)
                            .copied()
                            .ok_or_else(|| EmitError::new("tuple index is out of bounds"))
                    }
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
    #[track_caller]
    pub(super) fn type_id(&self, needle: Type) -> Result<TypeId, EmitError> {
        let caller = ::std::panic::Location::caller();
        self.find_type_id(&needle).ok_or_else(|| {
            EmitError::new(format!(
                "type table does not contain literal operand type {needle:?} at {}:{}",
                caller.file(),
                caller.line()
            ))
        })
    }

    /// Find the interned `__SmeltMatchGroups` class type, if the program uses it.
    ///
    /// The frontend interns this synthetic class whenever it types a `.groups`
    /// read, so it is present in the type table by the time codegen runs.
    fn match_groups_class_ty(&self) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|ty| {
                matches!(
                    ty,
                    Type::Class { name, .. }
                        if self.match_class_kind(*name)
                            == Ok(Some(smelt_stdlib::StdlibClass::MatchGroups))
                )
            })
            .and_then(|index| compact_index(index, "type index does not fit u32").ok())
            .map(TypeId)
    }

    /// Resolve the result type of a field read on a synthetic match-result class.
    ///
    /// Mirrors the frontend `builtin_class_field_type` typing so `place_ty` and
    /// the emitted accessor agree: a `.groups` read yields the named-group
    /// accessor class, `.index`/`.length` are floats, `.input` is a string, and
    /// every named-group read on the accessor class is an optional string. The
    /// concrete types are already interned by the frontend that typed the read,
    /// so `find_type_id` resolves them; an unmodeled field on the match value
    /// itself falls back to the erased boundary only if the program interned it.
    pub(super) fn match_field_ty(
        &self,
        kind: smelt_stdlib::StdlibClass,
        field: Symbol,
    ) -> Result<TypeId, EmitError> {
        let string_ty = self.type_id(Type::String)?;
        match kind {
            smelt_stdlib::StdlibClass::MatchGroups => self.type_id(Type::Optional(string_ty)),
            _ => match self.symbol_source_name(field)? {
                "index" | "length" => self.type_id(Type::Float),
                "input" => Ok(string_ty),
                "groups" => self.match_groups_class_ty().ok_or_else(|| {
                    EmitError::new("match groups accessor class type is not interned")
                }),
                _ => self.type_id(Type::Unknown),
            },
        }
    }

    /// Finds the type ID for a type when that type is present in the MIR table.
    pub(super) fn find_type_id(&self, needle: &Type) -> Option<TypeId> {
        let type_index = self.mir.types.all().iter().position(|ty| ty == needle)?;
        let compact = compact_index(type_index, "type index does not fit u32").ok()?;
        Some(TypeId(compact))
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

    /// Return whether `param` is a type parameter in scope for the current
    /// emitted function.
    ///
    /// Class constructors and methods are emitted inside `impl<T> Class<T>`
    /// blocks, so class type parameters are in scope for them. A generic free
    /// function (`fn identity<T>(x: T) -> T`) declares its own type parameters;
    /// those are in scope only within that function. Either way, an in-scope
    /// parameter keeps its generic shape instead of erasing to `SmeltUnknown`.
    pub(super) fn current_function_has_type_param(&self, param: Symbol) -> bool {
        self.current_function_type_params().contains(&param)
    }

    /// Return the type parameters in scope for the current emitted function.
    ///
    /// For a class member this is the owning class's generic parameters (the
    /// member is emitted inside the class `impl<T>` block). For a module-level
    /// free function it is the function's own declared type parameters, which
    /// lets a generic free function emit real Rust generics rather than routing
    /// its `T`-typed parameters and return through the runtime unknown carrier.
    pub(super) fn current_function_type_params(&self) -> HashSet<Symbol> {
        // A closure (or other nested) sub-emitter inherits the type parameters
        // that are in scope in the enclosing Rust output. That set was captured
        // from the enclosing emitter at construction time, so it is already
        // gated by the enclosing function's suppress/erasure decision: it is
        // empty when the enclosing signature erased its generics, and holds the
        // in-scope `T`s otherwise. Honoring it keeps closure bodies rendering
        // `T` exactly when the enclosing signature declares it.
        if !self.enclosing_type_params.is_empty() {
            return self.enclosing_type_params.clone();
        }
        let class_name = match self.function.origin {
            HirOrigin::ClassConstructor { class, .. }
            | HirOrigin::ClassMethod { class, .. }
            | HirOrigin::ClassStaticMethod { class, .. } => class,
            HirOrigin::Body(_) => {
                // A free function emits real Rust generics only when its
                // signature is generic-safe (see
                // `classes::function_emits_rust_generics`) AND the body-cleanliness
                // trial has not forced a fall back to erasure via
                // `suppress_type_params`.
                if *self.suppress_type_params.borrow()
                    || !crate::classes::function_emits_rust_generics(self.mir, self.function)
                {
                    return HashSet::new();
                }
                return self
                    .function
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect();
            }
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
                if self.is_regexp_class_symbol(*name)? {
                    return Ok("SmeltRegExp".to_owned());
                }
                // Both synthetic match-result classes are backed by the same
                // concrete `SmeltMatch` Rust type.
                if self.is_match_class_symbol(*name)? {
                    return Ok("SmeltMatch".to_owned());
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
                    // A generic class referenced without resolved type
                    // arguments (e.g. a local temp typed as bare
                    // `ImmutableCache` for a `new ImmutableCache<T>()` result)
                    // still needs its generic slots spelled so the reference is
                    // well-formed. Emit inference placeholders `<_, _>` per
                    // declared type parameter and let Rust unify them from the
                    // initializer (was E0107 in the generated `ImmutableCache`).
                    let declared_params = self
                        .mir
                        .classes
                        .iter()
                        .find(|class| class.name == *name)
                        .map_or(0, |class| class.type_params.len());
                    if declared_params == 0 {
                        Ok(type_name)
                    } else {
                        let placeholders = vec!["_"; declared_params].join(", ");
                        Ok(format!("{type_name}<{placeholders}>"))
                    }
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
                "SmeltList<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Set(item) if self.type_is_hash_set_key_safe(*item) => Ok(format!(
                "::std::collections::HashSet<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Set(item) => Ok(format!(
                "SmeltJsSet<{}>",
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
            Type::Union(_) if self.concrete_union_members(ty).is_some() => {
                self.union_type_text(ty)
            }
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
                let return_ty = if let Some(Type::Future(item)) =
                    self.mir.types.get(function.return_ty)
                {
                    // An async function's return type is the promise value
                    // itself (`SmeltFuture<T>`); a synchronous throw from an
                    // async function surfaces as a rejected future, not an outer
                    // `Result`, so `may_throw` does not add a wrapper here.
                    format!(
                        "SmeltFuture<{}>",
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
            // A source `Promise<T>` / `Type::Future(T)` lowers to the generic
            // promise-value ABI `SmeltFuture<T>` in every position, so the same
            // MIR future type renders one Rust type everywhere.
            Type::Future(item) => Ok(format!(
                "SmeltFuture<{}>",
                self.type_text_with_scoped_type_params(*item, false, scoped_type_params)?
            )),
            Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                ..
            } => Ok(format!(
                "Smelt{}Generator<{}, {}>",
                if *is_async { "Async" } else { "" },
                self.type_text_with_scoped_type_params(*yield_ty, false, scoped_type_params)?,
                self.type_text_with_scoped_type_params(*return_ty, false, scoped_type_params)?
            )),
            Type::GeneratorResult {
                yield_ty,
                return_ty,
            } => Ok(format!(
                "SmeltGeneratorResult<{}, {}>",
                self.type_text_with_scoped_type_params(*yield_ty, false, scoped_type_params)?,
                self.type_text_with_scoped_type_params(*return_ty, false, scoped_type_params)?
            )),
        }
    }

    /// Converts a function return type to Rust, including uncaught exception wrapping.
    /// Converts a function return type to Rust, including uncaught exception wrapping.
    pub(super) fn return_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        let inner = self.type_text_with_impl_trait(ty, false)?;
        if (self.function.is_async || self.function.can_throw)
            && !self.function.is_generator
        {
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
            Type::Unknown => Ok(self.null_value_text()),
            Type::Never => Ok(self.null_value_text()),
            Type::None => Ok("()".to_owned()),
            // Annotate the element type: a bare `Vec::new()` is uninferable when
            // the default is used as a `.into_iter()`/`.map()` receiver whose
            // element type is not otherwise constrained (E0282).
            Type::List(item) => Ok(format!(
                "SmeltList::new(Vec::<{}>::new())",
                self.type_text_with_impl_trait(*item, false)?
            )),
            Type::Set(item) if self.type_is_hash_set_key_safe(*item) => {
                Ok("::std::collections::HashSet::new()".to_owned())
            }
            Type::Set(_) => Ok("SmeltJsSet::new()".to_owned()),
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
            Type::Union(items) if self.concrete_union_members(ty).is_some() => {
                let first = *items
                    .first()
                    .ok_or_else(|| EmitError::new("concrete union has no members"))?;
                Ok(format!(
                    "{}::M0({})",
                    union::union_name(ty),
                    self.default_value(first)?
                ))
            }
            Type::TypeParam { .. } | Type::Union(_) => Ok(self.null_value_text()),
            Type::Class { name, .. } if self.is_regexp_class_symbol(*name)? => {
                Ok("SmeltRegExp::new(String::new(), String::new())".to_owned())
            }
            Type::Class { name, .. } if self.is_match_class_symbol(*name)? => {
                Ok("SmeltMatch::default()".to_owned())
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
                let return_ty = if let Some(Type::Future(item)) =
                    self.mir.types.get(function.return_ty)
                {
                    format!("SmeltFuture<{}>", self.type_text_with_impl_trait(*item, false)?)
                } else if function.may_throw {
                    format!(
                        "Result<{}, Box<dyn std::error::Error>>",
                        self.type_text_with_impl_trait(function.return_ty, false)?
                    )
                } else {
                    self.type_text_with_impl_trait(function.return_ty, false)?
                };
                let return_value = self.default_value(function.return_ty)?;
                let return_text = if let Some(Type::Future(item)) =
                    self.mir.types.get(function.return_ty)
                {
                    let item_value = self.default_value(*item)?;
                    format!("SmeltFuture::resolved({item_value})")
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
                "SmeltFuture::resolved({})",
                self.default_value(*item)?
            )),
            Type::Generator { .. } => Err(EmitError::new(
                "a generator value has no eager default",
            )),
            Type::GeneratorResult { return_ty, .. } => Ok(format!(
                "SmeltGeneratorResult::Complete({})",
                self.default_value(*return_ty)?
            )),
        }
    }

    /// Return the value type produced by `return` inside the current body.
    pub(super) fn body_return_ty(&self) -> TypeId {
        match (self.function.is_generator, self.mir.types.get(self.function.return_ty)) {
            (true, Some(Type::Generator { return_ty, .. })) => *return_ty,
            _ => self.function.return_ty,
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
                // An erased-unknown-rest function type renders as the concrete
                // `SmeltErasedFunction` struct (see `type_text_with_scoped_type_params`),
                // not a bare `Rc<dyn Fn(..)>`. Its default must therefore be a
                // `SmeltErasedFunction` wrapping a no-op callback so a struct
                // field default agrees with the field's declared type (a
                // callable-interface `__smelt_call` slot). Without this guard the
                // default below emits an `Rc<dyn Fn(..)>` closure whose type
                // mismatches the `SmeltErasedFunction` field (E0308).
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

    /// Returns the generated fields for a concrete class/interface storage type.
    pub(super) fn structural_record_fields(&self, ty: TypeId) -> Option<Vec<MirField>> {
        let Some(Type::Class { name, args }) = self.mir.types.get(ty) else {
            return None;
        };
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            let fields = crate::classes::effective_interface_fields(self.mir, interface);
            return Some(self.substitute_record_field_type_params(
                &interface.type_params,
                args,
                fields,
            ));
        }
        self.mir
            .classes
            .iter()
            .find(|class| class.name == *name)
            .map(|class| {
                self.substitute_record_field_type_params(
                    &class.type_params,
                    args,
                    crate::classes::effective_class_fields(self.mir, class),
                )
            })
    }

    /// Substitute concrete class/interface arguments into structural fields.
    ///
    /// Generated storage structs keep their Rust generic parameters, but
    /// adapter emission usually works with an instantiated type such as
    /// `MatchFnResult<f64>`. Field reads must therefore use the instantiated
    /// payload type instead of the declaration-time type parameter.
    fn substitute_record_field_type_params(
        &self,
        type_params: &[smelt_hir::TypeParamDef],
        args: &[TypeId],
        fields: Vec<MirField>,
    ) -> Vec<MirField> {
        let substitutions = type_params
            .iter()
            .zip(args.iter().copied())
            .map(|(param, arg)| (param.name, arg))
            .collect::<HashMap<_, _>>();
        if substitutions.is_empty() {
            return fields;
        }
        fields
            .into_iter()
            .map(|mut field| {
                field.ty = self.substitute_type_params_in_type(field.ty, &substitutions);
                field
            })
            .collect()
    }

    /// Substitute type parameters in a type, reusing already-interned MIR types.
    pub(super) fn substitute_type_params_in_type(
        &self,
        ty: TypeId,
        substitutions: &HashMap<Symbol, TypeId>,
    ) -> TypeId {
        let Some(ty_kind) = self.mir.types.get(ty) else {
            return ty;
        };
        match ty_kind {
            Type::TypeParam { name } => substitutions.get(name).copied().unwrap_or(ty),
            Type::Optional(inner) => self
                .existing_type_id(Type::Optional(
                    self.substitute_type_params_in_type(*inner, substitutions),
                ))
                .unwrap_or(ty),
            Type::List(item) => self
                .existing_type_id(Type::List(
                    self.substitute_type_params_in_type(*item, substitutions),
                ))
                .unwrap_or(ty),
            Type::Set(item) => self
                .existing_type_id(Type::Set(
                    self.substitute_type_params_in_type(*item, substitutions),
                ))
                .unwrap_or(ty),
            Type::Future(item) => self
                .existing_type_id(Type::Future(
                    self.substitute_type_params_in_type(*item, substitutions),
                ))
                .unwrap_or(ty),
            Type::Dict(key, value) => self
                .existing_type_id(Type::Dict(
                    self.substitute_type_params_in_type(*key, substitutions),
                    self.substitute_type_params_in_type(*value, substitutions),
                ))
                .unwrap_or(ty),
            Type::Tuple(items) => self
                .existing_type_id(Type::Tuple(
                    items
                        .iter()
                        .map(|item| self.substitute_type_params_in_type(*item, substitutions))
                        .collect(),
                ))
                .unwrap_or(ty),
            Type::Union(items) => self
                .existing_type_id(Type::Union(
                    items
                        .iter()
                        .map(|item| self.substitute_type_params_in_type(*item, substitutions))
                        .collect(),
                ))
                .unwrap_or(ty),
            Type::Class { name, args } => self
                .existing_type_id(Type::Class {
                    name: *name,
                    args: args
                        .iter()
                        .map(|arg| self.substitute_type_params_in_type(*arg, substitutions))
                        .collect(),
                })
                .unwrap_or(ty),
            Type::Function(function) => self
                .existing_type_id(Type::Function(FunctionType {
                    params: function
                        .params
                        .iter()
                        .map(|param| self.substitute_type_params_in_type(*param, substitutions))
                        .collect(),
                    rest: function.rest,
                    required_params: function.required_params,
                    mutable_params: function.mutable_params.clone(),
                    return_ty: self
                        .substitute_type_params_in_type(function.return_ty, substitutions),
                    is_async: function.is_async,
                    may_throw: function.may_throw,
                }))
                .unwrap_or(ty),
            _ => ty,
        }
    }

    /// Find the ID of an already interned type.
    pub(super) fn existing_type_id(&self, ty: Type) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|candidate| *candidate == ty)
            .and_then(|index| u32::try_from(index).ok())
            .map(TypeId)
    }

    /// Returns whether a generated storage type is an interface-shaped record.
    pub(super) fn is_interface_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. })
                if self
                    .mir
                    .interfaces
                    .iter()
                    .any(|interface| interface.name == *name)
        )
    }

    /// Returns true when `source` can be field-wise adapted to `target`.
    ///
    /// TypeScript option bags are structurally compatible, but generated Rust
    /// structs are nominal. This predicate intentionally only enables the
    /// adapter when the destination is an emitted interface record; ordinary
    /// class-to-class conversion still keeps Rust's nominal identity.
    pub(super) fn structural_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        self.structural_record_adapter_fields(source, target)
            .is_some()
    }
}
