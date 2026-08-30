//! JavaScript optional-chaining and nullish-coalescing access: optional field, index, method, and coalesce text emission over Option-wrapped receivers.

use super::*;
use smelt_hir::CLASS_INDEX_STORE_FIELD;

impl FunctionEmitter<'_> {
    /// Emits Rust for an optional-chain field read coerced to a destination type.
    pub(super) fn optional_field_text_for_dest(
        &self,
        receiver: &Operand,
        field: Symbol,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        // An optional field read on a statically-absent receiver (`None`/unit, as
        // for a host global the non-DOM profile does not model, e.g.
        // `window?.document`) always short-circuits to `undefined`. The receiver
        // carries no value to read, so fold to the destination's absent default
        // instead of attempting a field access on a unit type.
        if matches!(self.mir.types.get(inner_ty), Some(Type::None)) {
            return self.default_value(dest_ty);
        }
        let field_ty = self.field_access_type(inner_ty, field)?;
        if is_optional {
            let value = self.field_access_text("_smelt_value", inner_ty, field)?;
            if matches!(self.mir.types.get(inner_ty), Some(Type::Unknown))
                && self.symbol_name(field)? == "groups"
            {
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| {value})"
                ));
            }
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    let mapped = self.optional_inner_map_text(&value, *field_inner, *dest_inner)?;
                    return Ok(format!(
                        "{receiver_text}.as_ref().and_then(|_smelt_value| {mapped})"
                    ));
                }
                if let Some(erased) = self.erased_field_optional_map_text(&value, field_ty, *dest_inner)? {
                    return Ok(format!(
                        "{receiver_text}.as_ref().and_then(|_smelt_value| {erased})"
                    ));
                }
                let mapped = self.value_at_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| {mapped})"
                ));
            }
            let mapped = self.value_at_type_text(&value, field_ty, dest_ty)?;
            Ok(format!(
                "{receiver_text}.as_ref().map_or({}, |_smelt_value| {mapped})",
                self.default_value(dest_ty)?
            ))
        } else {
            let value = self.field_access_text(&receiver_text, inner_ty, field)?;
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    return self.optional_inner_map_text(&value, *field_inner, *dest_inner);
                }
                if let Some(erased) = self.erased_field_optional_map_text(&value, field_ty, *dest_inner)? {
                    return Ok(erased);
                }
                let mapped = self.value_at_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!("Some({mapped})"));
            }
            self.value_at_type_text(&value, field_ty, dest_ty)
        }
    }

    /// Emits an `Option<T>` for a *dynamically* read field whose static type is
    /// erased to `SmeltUnknown`.
    ///
    /// A field read on a union, an erased class, or an `unknown` receiver has no
    /// static field type, so `field_access_type` reports `Type::Unknown` and the
    /// emitted text is a `SmeltUnknown`-producing dynamic lookup that yields
    /// `SmeltUnknown::Undefined`/`Null` when the property is simply absent. The
    /// destination here is an `Option<T>`, i.e. the source expression's type is
    /// `T | undefined`, so an absent property must land in that `None` — the same
    /// answer JavaScript gives.
    ///
    /// Coercing the raw lookup straight to `T` instead would hand `None`'s job to
    /// `value_at_type_text`, which has no way to say "absent" and must invent a
    /// `T`: for a callback destination that is a synthesized default closure
    /// returning `false`, which then silently displaces the `??` fallback the
    /// source wrote (`options?.shouldRetry ?? DEFAULT_SHOULD_RETRY` bound a
    /// never-retry stub). This is the "unmodeled member silently becomes a value"
    /// shape; propagating `None` diagnoses it structurally instead.
    ///
    /// Returns `None` when the field type is not erased, leaving the statically
    /// typed paths — where the field provably exists — on their `map`/`Some`
    /// emission.
    fn erased_field_optional_map_text(
        &self,
        value_text: &str,
        field_ty: TypeId,
        dest_inner: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(self.mir.types.get(field_ty), Some(Type::Unknown)) {
            return Ok(None);
        }
        // An erased destination keeps the raw tagged value, `Undefined` included,
        // so there is nothing to narrow and `Some(..)` already round-trips.
        if matches!(self.mir.types.get(dest_inner), Some(Type::Unknown)) {
            return Ok(None);
        }
        let mapped = self.value_at_type_text("_smelt_field", field_ty, dest_inner)?;
        Ok(Some(format!(
            "match {value_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, _smelt_field => Some({mapped}) }}"
        )))
    }

    /// Emits Rust for a TypeScript optional-chain index read.
    pub(super) fn optional_index_text(
        &self,
        receiver: &Operand,
        index: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let result_ty = if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
            *inner
        } else {
            self.type_id(Type::Unknown)?
        };
        if is_optional {
            let value =
                self.optional_index_access_text("_smelt_value", inner_ty, index, result_ty)?;
            Ok(format!(
                "{receiver_text}.as_ref().and_then(|_smelt_value| {value})"
            ))
        } else {
            self.optional_index_access_text(&receiver_text, inner_ty, index, result_ty)
        }
    }

    /// Emits Rust for a TypeScript optional-chain method call.
    pub(super) fn optional_method_text(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        if args.is_empty()
            && self.mir.types.get(inner_ty) == Some(&Type::String)
            && matches!(
                self.symbol_name(method)?,
                "toUpperCase" | "to_upper_case" | "toLowerCase" | "to_lower_case"
            )
        {
            let rust_method_name = match self.symbol_name(method)? {
                "toUpperCase" | "to_upper_case" => "to_uppercase",
                "toLowerCase" | "to_lower_case" => "to_lowercase",
                _ => {
                    return Err(EmitError::new(
                        "unsupported optional string case-conversion method",
                    ));
                }
            };
            if is_optional {
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| _smelt_value.{rust_method_name}())"
                ));
            }
            return Ok(format!("Some({receiver_text}.{rust_method_name}())"));
        }
        if is_optional {
            if self.optional_method_returns_optional(inner_ty, method, dest_ty)? {
                return Ok(format!(
                    "{receiver_text}.as_ref().and_then(|_smelt_value| _smelt_value.{method_name}({args_text}))"
                ));
            }
            Ok(format!(
                "{receiver_text}.as_ref().map(|_smelt_value| _smelt_value.{method_name}({args_text}))"
            ))
        } else {
            Ok(format!("Some({receiver_text}.{method_name}({args_text}))"))
        }
    }

    /// Return whether optional method chaining should flatten the method result.
    fn optional_method_returns_optional(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
        dest_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) else {
            return Ok(false);
        };
        let Some(return_ty) = self.method_return_type(receiver_ty, method)? else {
            return Ok(false);
        };
        Ok(matches!(
            self.mir.types.get(return_ty),
            Some(Type::Optional(return_inner)) if return_inner == dest_inner
        ))
    }

    /// Resolve the static return type for a known class or interface method.
    fn method_return_type(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
    ) -> Result<Option<TypeId>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Ok(None);
        };
        if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
            for method_id in &class.methods {
                let function = self
                    .mir
                    .functions
                    .get(id_index(method_id.0, "method index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("class method references an unknown function"))?;
                if function.name == method {
                    return Ok(Some(function.return_ty));
                }
            }
            return Ok(None);
        }
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return Ok(interface
                .methods
                .iter()
                .find(|signature| signature.name == method)
                .map(|signature| signature.return_ty));
        }
        Ok(None)
    }

    /// Returns receiver source, the non-optional receiver type, and whether it was optional.
    fn optional_receiver_parts(
        &self,
        receiver: &Operand,
    ) -> Result<(String, TypeId, bool), EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.operand_text(receiver)?;
        if let Some(Type::Optional(inner)) = self.mir.types.get(receiver_ty) {
            Ok((receiver_text, *inner, true))
        } else {
            Ok((receiver_text, receiver_ty, false))
        }
    }

    /// Emits TypeScript nullish coalescing for optional operands.
    pub(super) fn optional_coalesce_text(
        &self,
        optional: &Operand,
        fallback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let optional_ty = self.operand_ty(optional)?;
        if matches!(
            self.mir.types.get(optional_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(optional_ty)
        {
            // The nullish `match` operates on the erased `SmeltUnknown` form. A
            // concrete-union operand stores a tagged enum, so both the scrutinee
            // and the fallback are rendered erased here and the tagged union is
            // reconstructed by the destination coercion below.
            let scrutinee_ty = if self.concrete_union_members(optional_ty).is_some() {
                self.type_id(Type::Unknown)?
            } else {
                optional_ty
            };
            let optional_text = self.value_at_type(optional, scrutinee_ty)?;
            let fallback_text = self.value_at_type(fallback, scrutinee_ty)?;
            let coalesced = format!(
                "match {optional_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => {fallback_text}, value => value }}"
            );
            if matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Optional(inner)) if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*inner)
            ) {
                return Ok(format!("Some({coalesced})"));
            }
            // A concrete-union destination is not an erased boundary: coerce the
            // erased coalesced value into the tagged union (`from_smelt_unknown`)
            // rather than leaving it as `SmeltUnknown`.
            let dest_is_erased = (matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Union(_))
            ) && self.concrete_union_members(dest_ty).is_none())
                || self.is_erased_class_type(dest_ty);
            if !dest_is_erased {
                return self.value_at_type_text(&coalesced, scrutinee_ty, dest_ty);
            }
            return Ok(coalesced);
        }
        match self.mir.types.get(optional_ty) {
            Some(Type::Optional(inner)) => {
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let optional_text = self.operand_text(optional)?;
                    let present_text = self.value_at_type_text("value", *inner, dest_ty)?;
                    let fallback_text = self.value_at_type(fallback, dest_ty)?;
                    return Ok(format!(
                        "{optional_text}.map_or_else(|| {fallback_text}, |value| {present_text})"
                    ));
                }
                let fallback_ty = self.operand_ty(fallback)?;
                if matches!(
                    self.mir.types.get(fallback_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(fallback_ty)
                {
                    let optional_text = self.operand_text(optional)?;
                    let fallback_text = self.value_at_type(fallback, fallback_ty)?;
                    let mapped_value = self.value_at_type_text("value", fallback_ty, *inner)?;
                    let fallback_option = format!(
                        "match {fallback_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some({mapped_value}) }}"
                    );
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                    {
                        return Ok(format!("{optional_text}.or({fallback_option})"));
                    }
                    let coalesced = format!(
                        "{optional_text}.unwrap_or_else(|| {fallback_option}.unwrap_or({}))",
                        self.default_value(*inner)?
                    );
                    if dest_ty == *inner {
                        return Ok(coalesced);
                    }
                    return self.value_at_type_text(&coalesced, *inner, dest_ty);
                }
                if let Some(Type::Optional(fallback_inner)) = self.mir.types.get(fallback_ty)
                    && fallback_inner == inner
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                {
                    return Ok(format!(
                        "{}.clone().or({})",
                        self.operand_text(optional)?,
                        self.operand_text(fallback)?
                    ));
                }
                let coalesced = format!(
                    "{}.clone().unwrap_or({})",
                    self.operand_text(optional)?,
                    self.value_at_type(fallback, *inner)?
                );
                if dest_ty == *inner {
                    Ok(coalesced)
                } else {
                    self.value_at_type_text(&coalesced, *inner, dest_ty)
                }
            }
            Some(Type::None) => self.operand_text(fallback),
            _ => self.operand_text(optional),
        }
    }

    /// Emit a source `Option<S>` as a destination `Option<T>`.
    fn optional_inner_map_text(
        &self,
        value_text: &str,
        source_inner: TypeId,
        dest_inner: TypeId,
    ) -> Result<String, EmitError> {
        if source_inner == dest_inner {
            return Ok(value_text.to_owned());
        }
        let mapped = self.value_at_type_text("_smelt_inner", source_inner, dest_inner)?;
        Ok(format!("{value_text}.map(|_smelt_inner| {mapped})"))
    }

    /// Emits a JavaScript optional-chain index read against an in-scope receiver value.
    ///
    /// `value?.[index]` short-circuits only when the receiver is nullish, but a
    /// missing array/string element still produces `undefined`. Smelt models
    /// that as `None`, so this helper deliberately avoids the strict
    /// `expect("index out of bounds")` used by normal element access.
    fn optional_index_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        index: &Operand,
        result_ty: TypeId,
    ) -> Result<String, EmitError> {
        // A numbered group read (`match[n]`) has an `Optional(String)` result, so
        // MIR routes it through `OptionalIndex`; `group_owned` already yields the
        // `Option<String>` this path expects.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_match_class_symbol(*name)?
        {
            return self.match_index_text(receiver_text, index);
        }
        // A class with an index signature backs keyed reads with a real store
        // field (issue #84). A dynamic `bag[key]` read returns the store value
        // for the key or `None` (missing key -> `undefined`), giving the honest
        // `Option<T>` round-trip result rather than a stub.
        if let Some((key_ty, _value_ty)) = self.class_index_store_types(receiver_ty) {
            let store_text = format!("{receiver_text}.{CLASS_INDEX_STORE_FIELD}");
            return self.dict_index_optional_read_text(&store_text, key_ty, index);
        }
        match self.mir.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => {
                let index_text =
                    self.optional_normalized_index_text(&format!("{receiver_text}.len()"), index)?;
                // Read receiver, not a write one: the `.len()` in the index
                // argument takes a second borrow of the same shared cell, and
                // only shared borrows may coexist. See `place.rs`'s list index
                // read arm for the full argument.
                let read_text = list_read_text(receiver_text);
                if let Some(Type::Optional(inner)) = self.mir.types.get(*item_ty)
                    && *inner == result_ty
                {
                    Ok(format!(
                        "({index_text}).and_then(|index| {read_text}.get(index).cloned().flatten())"
                    ))
                } else {
                    Ok(format!(
                        "({index_text}).and_then(|index| {read_text}.get(index).cloned())"
                    ))
                }
            }
            Some(Type::Dict(key_ty, _)) => {
                self.dict_index_optional_read_text(receiver_text, *key_ty, index)
            }
            Some(Type::String) => {
                let index_text = self.optional_normalized_index_text(
                    &format!("{receiver_text}.chars().count()"),
                    index,
                )?;
                Ok(format!(
                    "({index_text}).and_then(|index| {receiver_text}.chars().nth(index).map(|ch| ch.to_string()))"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if matches!(
                    self.mir.types.get(receiver_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(receiver_ty) =>
            {
                let index_ty = self.operand_ty(index)?;
                let index_text = self.operand_text(index)?;
                let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
                let numeric_index_text = match self.mir.types.get(index_ty) {
                    Some(Type::Int | Type::Float) => index_text,
                    Some(Type::Bool) => format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}"),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    | Some(Type::Class { .. })
                        if self.is_erased_class_type(index_ty)
                            || matches!(
                                self.mir.types.get(index_ty),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            ) =>
                    {
                        format!(
                            "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
                        )
                    }
                    _ => "f64::NAN".to_owned(),
                };
                let string_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    "value.chars().nth(index).map(|ch| ch.to_string())".to_owned()
                } else {
                    "value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string().into()))"
                        .to_owned()
                };
                let array_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    "values.get(index).cloned().map(|value| match value { SmeltUnknown::String(value) => value.to_string(), other => other.to_string() })".to_owned()
                } else {
                    "values.get(index).cloned()".to_owned()
                };
                // Read the OBJECT arm through `smelt_get_object_field`, the same
                // helper the erased static field read uses, so `o?.[k]` answers
                // what `o.k` answers: `o` may be a marker record whose property
                // is synthesized rather than stored (`err.name`, `x.constructor`,
                // a Map's `size`, the global object's builtin constructors), and
                // an own-field-only `values.get` was blind to all of them.
                // `Undefined` is the helper's "no such property" answer and maps
                // back to `None`, which is what the optional-read shape means.
                let object_field = format!("smelt_get_object_field(&values, &{key_text})");
                let object_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    format!(
                        "match {object_field} {{ SmeltUnknown::Undefined => None, SmeltUnknown::String(value) => Some(value.to_string()), other => Some(other.to_string()) }}"
                    )
                } else {
                    format!("match {object_field} {{ SmeltUnknown::Undefined => None, value => Some(value) }}")
                };
                let primitive_none = "SmeltUnknown::Bool(_) | SmeltUnknown::Number(_) | SmeltUnknown::Symbol(_) | SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => None";
                Ok(format!(
                    r"match {receiver_text}.clone() {{
                        SmeltUnknown::String(value) => {{
                            let len = value.chars().count() as i64;
                            let index = {numeric_index_text} as i64;
                            let normalized = if index < 0 {{ len + index }} else {{ index }};
                            usize::try_from(normalized).ok().and_then(|index| {string_some})
                        }}
                        SmeltUnknown::Array(values) => {{
                            let len = values.len() as i64;
                            let index = {numeric_index_text} as i64;
                            let normalized = if index < 0 {{ len + index }} else {{ index }};
                            usize::try_from(normalized).ok().and_then(|index| {array_some})
                        }}
                        SmeltUnknown::Object(values) => {object_some},
                        {primitive_none},
                    }}"
                ))
            }
            _ => Ok("None".to_owned()),
        }
    }

    /// Normalize an optional JavaScript array/string read without panicking on misses.
    ///
    /// Indexed source reads whose value is already optional model JavaScript
    /// `undefined`; a negative or out-of-range normalized position therefore
    /// remains `None` instead of entering strict Python-style index behavior.
    fn optional_normalized_index_text(
        &self,
        len_expr: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(format!(
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).ok() }}"
        ))
    }
}
