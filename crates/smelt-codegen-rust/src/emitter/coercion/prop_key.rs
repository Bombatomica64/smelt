//! `ToPropertyKey` at the coercion seam: any value flowing into a
//! [`Type::PropKey`] slot (the key of a property-key record) is converted.
//!
//! A property-key record's Rust key type is the runtime `SmeltPropKey`, whose
//! only constructor is the `SmeltToPropKey` conversion, so a key that skipped
//! this rule would not type-check. Every key position reaches it through
//! [`FunctionEmitter::rendered_at_type`] / [`FunctionEmitter::value_at_type`]:
//! reads, writes, `in`, `delete`, an entry push and literal entries alike.

use super::*;

impl FunctionEmitter<'_> {
    /// The conversion of `value_text` (of type `source`) to a property key, or
    /// `None` when `target` is not [`Type::PropKey`] or no conversion is needed.
    ///
    /// A value whose runtime type implements `SmeltToPropKey` (a string,
    /// number, boolean, symbol, key, an optional of one, or the erased
    /// carrier) converts directly. Anything else -- a generated union such as
    /// `PropertyKey` itself, a type parameter, an object -- is erased first
    /// through the seam's own `erase_rendered`, and the carrier's conversion
    /// applies `ToPropertyKey` to the tag it holds.
    pub(super) fn prop_key_conversion(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        if source == target || self.mir.types.get(target) != Some(&Type::PropKey) {
            return Ok(None);
        }
        let direct = self.converts_to_prop_key_directly(source);
        let value = if direct {
            value_text.to_owned()
        } else {
            self.erase_rendered(value_text, source)?
        };
        Ok(Some(RustExpr::chain(format!(
            "SmeltToPropKey::smelt_to_prop_key(&({value}))",
            value = value.delimited()
        ))))
    }

    /// Whether `ty`'s Rust rendering implements `SmeltToPropKey` itself.
    fn converts_to_prop_key_directly(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(
                Type::String
                | Type::Float
                | Type::Int
                | Type::Bool
                | Type::Symbol
                | Type::PropKey
                | Type::Unknown,
            ) => true,
            // `i32`, `i64` and `usize` all carry the conversion.
            Some(Type::FixedInt(_)) => true,
            Some(Type::Optional(inner)) => self.converts_to_prop_key_directly(*inner),
            _ => false,
        }
    }

    /// A `string | number | symbol` value at a `string` slot: each arm as the
    /// string key it names.
    ///
    /// Smelt lowers TypeScript's `keyof T` to `string`, while the keys it
    /// denotes may be numbers and symbols too (`[...Object.keys(o),
    /// ...getSymbols(o)] as Array<keyof T>`). A string-keyed record stores a
    /// number key as its canonical string and a symbol key under its reserved
    /// storage spelling, so that is what each arm becomes here: the value
    /// still indexes the same property it named. A union with any other arm
    /// is not a key union and keeps the ordinary rules (`None`).
    pub(super) fn key_union_at_string(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        if self.mir.types.get(target) != Some(&Type::String) {
            return Ok(None);
        }
        let Some(members) = self.concrete_union_members(source) else {
            return Ok(None);
        };
        let key_arm = |member: &TypeId| {
            matches!(
                self.mir.types.get(*member),
                Some(Type::String | Type::Float | Type::Int | Type::FixedInt(_) | Type::Symbol)
            )
        };
        if !members.iter().all(key_arm)
            || !members
                .iter()
                .any(|member| self.mir.types.get(*member) == Some(&Type::Symbol))
        {
            return Ok(None);
        }
        let union_name = union::union_name(source);
        let arms = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                let key =
                    self.property_key_to_string_expr(&RustExpr::path("smelt_value"), *member)?;
                Ok(format!(
                    "{union_name}::M{index}(smelt_value) => {key}",
                    key = key.delimited()
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        Ok(Some(RustExpr::atom(format!(
            "match {value_text} {{ {} }}",
            arms.join(", "),
            value_text = value_text.delimited()
        ))))
    }
}
