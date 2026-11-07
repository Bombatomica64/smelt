//! JavaScript truthiness of a value, per type.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Converts an optional value to JavaScript truthiness for its contained type.
    pub(super) fn optional_truthy_expr(
        &self,
        operand_expr: &RustExpr,
        inner_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match self.mir.types.get(inner_ty) {
            Some(Type::Bool) => Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.clone().unwrap_or(false)"))),
            Some(Type::Int | Type::FixedInt(_)) => Ok(RustExpr::postfix_on(&operand_expr, format!(
                "{operand_expr}.clone().is_some_and(|value| value != 0)"
            ))),
            Some(Type::BigInt) => Ok(RustExpr::postfix_on(&operand_expr, format!(
                "{operand_expr}.as_ref().is_some_and(|value| !::num_traits::Zero::is_zero(value))"
            ))),
            Some(Type::Float) => Ok(RustExpr::postfix_on(&operand_expr, format!(
                "{operand_expr}.clone().is_some_and(|value| value != 0.0 && !value.is_nan())"
            ))),
            Some(Type::String | Type::PyStr) => Ok(RustExpr::postfix_on(&operand_expr, format!(
                "{operand_expr}.clone().is_some_and(|value| !value.is_empty())"
            ))),
            Some(Type::Union(_)) => {
                // A concrete generated union (`SmeltUnionNNNN`) is not a
                // `SmeltUnknown`, so its inner value cannot be matched against
                // `SmeltUnknown::` arms directly. Erase it through the union's
                // `IntoSmeltUnknown` boundary adapter first — truthiness is a
                // genuine runtime-narrowing inspection of the tagged value, and
                // only the resulting `bool` escapes, so no static shape is lost.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {operand_expr}.clone().map(|value| value.into_smelt_unknown()) {{ None => false, Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), {smelt_bigint_arm}Some(SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_)) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("Some(SmeltUnknown::BigInt(value)) => !::num_traits::Zero::is_zero(&value),")
                ))))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never) => {
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {operand_expr}.clone() {{ None => false, Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), {smelt_bigint_arm}Some(SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_)) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("Some(SmeltUnknown::BigInt(value)) => !::num_traits::Zero::is_zero(&value),")
                ))))
            }
            Some(Type::Optional(inner)) => {
                let nested = self.optional_truthy_expr(&RustExpr::path("value"), *inner)?;
                Ok(RustExpr::atom(format!(
                    "match {operand_expr}.clone() {{ Some(value) => {nested}, None => false }}"
                )))
            }
            Some(Type::None) => Ok(RustExpr::path("false")),
            Some(Type::Class { .. })
            | Some(Type::Function(_))
            | Some(Type::List(_))
            | Some(Type::Tuple(_))
            | Some(Type::Dict(_, _))
            | Some(Type::JsMap(_, _))
            | Some(Type::Set(_))
            | Some(Type::Future(_)) => Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.is_some()"))),
            Some(Type::Generator { .. }) => Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.is_some()"))),
            Some(Type::GeneratorResult { .. }) => Ok(RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.is_some()"))),
            None => Err(EmitError::new("optional truthiness inner type is unknown")),
        }
    }

    /// Return whether a union value's truthiness is `true` for every value its
    /// Rust representation can hold.
    ///
    /// Only a CONCRETE generated union (`SmeltUnion…`) qualifies: its variants
    /// are exactly the declared arms, so when every arm is an object surface
    /// (class, function, list, tuple, dict, map, set, future — never falsy in
    /// JavaScript) no value is falsy. A union stored erased as `SmeltUnknown`
    /// can also hold `undefined` — a keyed read of a missing entry produces it
    /// whatever the static type says — so it keeps the runtime test. The
    /// frontend leaves every such union guard as a `ToBool` for this decision.
    pub(super) fn union_is_constantly_truthy(&self, ty: TypeId) -> bool {
        let Some(members) = self.concrete_union_members(ty) else {
            return false;
        };
        members.iter().all(|member| {
            matches!(
                self.mir.types.get(*member),
                Some(
                    Type::Class { .. }
                        | Type::Function(_)
                        | Type::List(_)
                        | Type::Tuple(_)
                        | Type::Dict(_, _)
                        | Type::JsMap(_, _)
                        | Type::Set(_)
                        | Type::Future(_)
                )
            )
        })
    }

    /// Convert an operand to a Rust boolean using source-language truthiness.
    pub(super) fn truthy_operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        let bool_ty = self.type_id(Type::Bool)?;
        self.primitive_cast_expr(smelt_hir::PrimitiveCastOp::ToBool, operand, bool_ty)
    }

    /// Convert an already-rendered value expression to a Rust boolean using
    /// JavaScript/Python truthiness rules.
    ///
    /// This is the text-based counterpart to the `ToBool` arm of
    /// [`Self::primitive_cast_expr`]: callers that hold a rendered expression
    /// (rather than an [`Operand`]) — such as an array predicate callback result —
    /// coerce it here. `value_text` is evaluated exactly once. Objects, arrays,
    /// functions, class instances, and other reference values are always truthy;
    /// primitives follow their per-type emptiness/zero rules; optionals defer to
    /// [`Self::optional_truthy_expr`].
    pub(super) fn value_truthy_expr(
        &self,
        value_text: &RustExpr,
        ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match self.mir.types.get(ty) {
            Some(Type::Bool) => Ok(value_text.to_owned()),
            Some(Type::Int | Type::FixedInt(_)) => Ok(RustExpr::loose(format!("({value_text}) != 0"))),
            Some(Type::BigInt) => Ok(RustExpr::loose(format!("!::num_traits::Zero::is_zero(&({value_text}))"))),
            Some(Type::Float) => Ok(RustExpr::atom(format!(
                "{{ let smelt_number = ({value_text}); smelt_number != 0.0 && !smelt_number.is_nan() }}"
            ))),
            Some(Type::String | Type::PyStr) => Ok(RustExpr::loose(format!("!({value_text}).is_empty()"))),
            Some(Type::Union(_)) => {
                // A concrete generated union is not a `SmeltUnknown`; erase it
                // through its `IntoSmeltUnknown` boundary adapter before the
                // runtime-narrowing truthiness match (see `optional_truthy_expr`).
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match ({value_text}).into_smelt_unknown() {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), {smelt_bigint_arm}SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value),")
                ))))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never) => {
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match ({value_text}) {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), {smelt_bigint_arm}SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value),")
                ))))
            }
            Some(Type::Optional(inner)) => self.optional_truthy_expr(value_text, *inner),
            Some(Type::None) => Ok(RustExpr::atom(format!("{{ let _ = ({value_text}); false }}"))),
            Some(
                Type::Class { .. }
                | Type::Function(_)
                | Type::List(_)
                | Type::Tuple(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Set(_)
                | Type::Future(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            ) => Ok(RustExpr::atom(format!("{{ let _ = ({value_text}); true }}"))),
            None => Err(EmitError::new("predicate result type is unknown")),
        }
    }
}
