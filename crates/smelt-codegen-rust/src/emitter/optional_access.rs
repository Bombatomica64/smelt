//! JavaScript optional-chaining and nullish-coalescing access: optional field, index, method, and coalesce text emission over Option-wrapped receivers.

use crate::rust::RustExpr;
use super::*;
use smelt_hir::CLASS_INDEX_STORE_FIELD;

impl FunctionEmitter<'_> {
    /// Emits Rust for an optional-chain field read coerced to a destination type.
    pub(super) fn optional_field_expr_for_dest(
        &self,
        receiver: &Operand,
        field: Symbol,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
            let value = self.field_access_expr(&RustExpr::path("_smelt_value"), inner_ty, field)?;
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    let mapped = self.optional_inner_map_expr(&value, *field_inner, *dest_inner)?;
                    return Ok(receiver_text.postfix(format!(
                        ".as_ref().and_then(|_smelt_value| {mapped})", mapped = mapped.delimited()
                    )));
                }
                if let Some(erased) = self.erased_field_optional_map_expr(&value, field_ty, *dest_inner)? {
                    return Ok(receiver_text.postfix(format!(
                        ".as_ref().and_then(|_smelt_value| {erased})", erased = erased.delimited()
                    )));
                }
                let mapped = self.rendered_at_type(&value, field_ty, *dest_inner, &self.render_scope())?;
                return Ok(receiver_text.postfix(format!(
                    ".as_ref().map(|_smelt_value| {mapped})", mapped = mapped.delimited()
                )));
            }
            let mapped = self.rendered_at_type(&value, field_ty, dest_ty, &self.render_scope())?;
            Ok(receiver_text.postfix(format!(
                ".as_ref().map_or({}, |_smelt_value| {mapped})",
                self.default_value(dest_ty)?.delimited(), mapped = mapped.delimited()
            )))
        } else {
            let value = self.field_access_expr(&receiver_text, inner_ty, field)?;
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    return self.optional_inner_map_expr(&value, *field_inner, *dest_inner);
                }
                if let Some(erased) = self.erased_field_optional_map_expr(&value, field_ty, *dest_inner)? {
                    return Ok(erased);
                }
                let mapped = self.rendered_at_type(&value, field_ty, *dest_inner, &self.render_scope())?;
                return Ok(RustExpr::chain(format!("Some({mapped})", mapped = mapped.delimited())));
            }
            self.rendered_at_type(&value, field_ty, dest_ty, &self.render_scope())
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
    /// `rendered_at_type`, which has no way to say "absent" and must invent a
    /// `T`: for a callback destination that is a synthesized default closure
    /// returning `false`, which then silently displaces the `??` fallback the
    /// source wrote (`options?.shouldRetry ?? DEFAULT_SHOULD_RETRY` bound a
    /// never-retry stub). This is the "unmodeled member silently becomes a value"
    /// shape; propagating `None` diagnoses it structurally instead.
    ///
    /// Returns `None` when the field type is not erased, leaving the statically
    /// typed paths — where the field provably exists — on their `map`/`Some`
    /// emission.
    fn erased_field_optional_map_expr(
        &self,
        value_text: &RustExpr,
        field_ty: TypeId,
        dest_inner: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        // "Erased" means the field's VALUE is carried as a tagged
        // `SmeltUnknown`: a `Type::Unknown` field, and equally a wide union
        // field (`number | null | (n) => number` stored in an erased record)
        // that renders to `SmeltUnknown`. Both can hold `Undefined` for an
        // absent key, which must become `None`, not a `Some` of a union
        // decoded from `undefined`.
        if !matches!(self.mir.types.get(field_ty), Some(Type::Unknown))
            && !self.renders_erased(field_ty)?
        {
            return Ok(None);
        }
        // An erased destination keeps the raw tagged value, `Undefined` included,
        // so there is nothing to narrow and `Some(..)` already round-trips.
        if matches!(self.mir.types.get(dest_inner), Some(Type::Unknown)) {
            return Ok(None);
        }
        let mapped = self.rendered_at_type(&RustExpr::path("_smelt_field"), field_ty, dest_inner, &self.render_scope())?;
        Ok(Some(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "match {value_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, _smelt_field => Some({mapped}) }}", value_text = value_text.delimited(), mapped = mapped.delimited()
        )))))
    }

    /// Emits Rust for a TypeScript optional-chain index read.
    pub(super) fn optional_index_expr(
        &self,
        receiver: &Operand,
        index: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let result_ty = if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
            *inner
        } else {
            self.type_id(Type::Unknown)?
        };
        if !is_optional
            && matches!(self.mir.types.get(dest_ty), Some(Type::Optional(_)))
            && let Some(text) = self.concrete_union_optional_index_expr(
                &receiver_text,
                inner_ty,
                index,
                result_ty,
            )?
        {
            return Ok(text);
        }
        if is_optional {
            let value =
                self.optional_index_access_expr(&RustExpr::path("_smelt_value"), inner_ty, index, result_ty)?;
            Ok(receiver_text.postfix(format!(
                ".as_ref().and_then(|_smelt_value| {value})", value = value.delimited()
            )))
        } else {
            self.optional_index_access_expr(&receiver_text, inner_ty, index, result_ty)
        }
    }

    /// Emits Rust for a TypeScript optional-chain method call.
    pub(super) fn optional_method_expr(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let method_name = self.method_rust_name(method)?;
        let args_text = args
            .iter()
            .map(|arg| self.operand_expr(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        if args.is_empty()
            && self.mir.types.get(inner_ty).is_some_and(Type::is_text)
            && matches!(
                self.symbol_name(method)?,
                "toUpperCase" | "to_upper_case" | "toLowerCase" | "to_lower_case"
            )
        {
            let js = self.mir.types.get(inner_ty) == Some(&Type::String);
            let rust_method_name = match self.symbol_name(method)? {
                "toUpperCase" | "to_upper_case" if js => "to_upper_case",
                "toLowerCase" | "to_lower_case" if js => "to_lower_case",
                "toUpperCase" | "to_upper_case" => "to_uppercase",
                "toLowerCase" | "to_lower_case" => "to_lowercase",
                _ => {
                    return Err(EmitError::new(
                        "unsupported optional string case-conversion method",
                    ));
                }
            };
            if is_optional {
                return Ok(receiver_text.postfix(format!(
                    ".as_ref().map(|_smelt_value| _smelt_value.{rust_method_name}())"
                )));
            }
            return Ok(RustExpr::chain(format!("Some({receiver_text}.{rust_method_name}())", receiver_text = receiver_text.tight())));
        }
        // A method that can throw returns a `Result` in the generated Rust, and
        // `?` cannot cross a closure boundary: `opt.map(|v| v.insert(k, x))`
        // answered `Option<Result<(), Box<dyn Error>>>` into an `Option<()>`
        // slot (2 of the router slice's errors). A `match` puts the call in the
        // enclosing function's own body, where `?` propagates as it does for
        // every other call to the same method.
        let throwing_suffix = self.optional_method_throwing_suffix(inner_ty, method)?;
        let flattens = self.optional_method_returns_optional(inner_ty, method, dest_ty)?;
        if !throwing_suffix.is_empty() {
            let call = format!("{method_name}({args_text}){throwing_suffix}");
            if !is_optional {
                let head = if flattens {
                    RustExpr::field_on(&receiver_text, format!("{receiver_text}.{call}", receiver_text = receiver_text.tight()))
                } else {
                    RustExpr::chain(format!("Some({receiver_text}.{call})", receiver_text = receiver_text.tight()))
                };
                return Ok(head);
            }
            let arm = if flattens {
                format!("_smelt_value.{call}")
            } else {
                format!("Some(_smelt_value.{call})")
            };
            return Ok(RustExpr::atom(format!(
                "match {receiver_text}.as_ref() {{ Some(_smelt_value) => {arm}, None => None }}", receiver_text = receiver_text.tight()
            )));
        }
        if is_optional {
            if flattens {
                return Ok(receiver_text.postfix(format!(
                    ".as_ref().and_then(|_smelt_value| _smelt_value.{method_name}({args_text}))"
                )));
            }
            Ok(receiver_text.postfix(format!(
                ".as_ref().map(|_smelt_value| _smelt_value.{method_name}({args_text}))"
            )))
        } else {
            Ok(RustExpr::chain(format!("Some({receiver_text}.{method_name}({args_text}))", receiver_text = receiver_text.tight())))
        }
    }

    /// The `?` suffix a throwing method call needs, or `""`.
    ///
    /// Resolved from the receiver's class method exactly as
    /// [`Self::method_return_type`] resolves the return type, so the optional
    /// chain and an ordinary call to the same method agree about whether it
    /// returns a `Result`.
    fn optional_method_throwing_suffix(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
    ) -> Result<&'static str, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Ok("");
        };
        let Some(class) = crate::classes::find_class(self.mir, *name) else {
            return Ok("");
        };
        for method_id in &class.methods {
            let function = self
                .mir
                .functions
                .get(id_index(method_id.0, "method index does not fit usize")?)
                .ok_or_else(|| EmitError::new("class method references an unknown function"))?;
            if function.name == method {
                return Ok(self.throwing_call_suffix(function));
            }
        }
        Ok("")
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
        if let Some(class) = crate::classes::find_class(self.mir, *name) {
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
    ) -> Result<(RustExpr, TypeId, bool), EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.operand_expr(receiver)?;
        if let Some(Type::Optional(inner)) = self.mir.types.get(receiver_ty) {
            Ok((receiver_text, *inner, true))
        } else {
            Ok((receiver_text, receiver_ty, false))
        }
    }

    /// Emits TypeScript nullish coalescing for optional operands.
    pub(super) fn optional_coalesce_expr(
        &self,
        optional: &Operand,
        fallback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let optional_ty = self.operand_ty(optional)?;
        if self.has_dynamic_shape(optional_ty)
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
            let coalesced = RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {optional_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => {fallback_text}, smelt_value => smelt_value }}", optional_text = optional_text.delimited(), fallback_text = fallback_text.delimited()
            )));
            if matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Optional(inner)) if self.has_dynamic_shape(*inner)
            ) {
                return Ok(RustExpr::chain(format!("Some({coalesced})", coalesced = coalesced.delimited())));
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
                return self.rendered_at_type(&coalesced, scrutinee_ty, dest_ty, &self.render_scope());
            }
            return Ok(coalesced);
        }
        // `rec[key] ?? fallback` / `arr[i] ?? fallback` over a NON-optional
        // declared element type: TypeScript types the read as the element, but
        // a missing key or index reads `undefined` and `??` takes the fallback.
        // The read's own `Option` (`get(..)`) is what `??` tests; rendering the
        // total read instead supplied the element type's default (`""`) and
        // never reached the fallback, so Hono's trie-router
        // `nodeParams[key] ?? params?.[key]` answered `""` for every
        // regexp-matched path param.
        if !matches!(
            self.mir.types.get(optional_ty),
            Some(Type::Optional(_) | Type::None)
        ) && let Some(read) = self.optional_element_read_expr(optional, optional_ty)?
        {
            let present_text =
                self.rendered_at_type(&RustExpr::path("smelt_value"), optional_ty, dest_ty, &self.render_scope())?;
            let fallback_text = self.value_at_type(fallback, dest_ty)?;
            return Ok(RustExpr::atom(format!(
                "match {read} {{ Some(smelt_value) => {present_text}, None => {fallback_text} }}", read = read.delimited(), present_text = present_text.delimited(), fallback_text = fallback_text.delimited()
            )));
        }
        match self.mir.types.get(optional_ty) {
            // `Some` of an erased value can still hold `undefined` or `null`
            // (`receiver?.missing` keeps the raw read so `=== null` stays exact),
            // and `??` tests the value, not the wrapper.
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Unknown))
                    && !matches!(self.mir.types.get(self.operand_ty(fallback)?), Some(Type::None)) =>
            {
                let optional_text = self.operand_expr(optional)?;
                let fallback_text = self.value_at_type(fallback, *inner)?;
                let coalesced = optional_text.postfix(format!(
                    ".filter(|smelt_value| !smelt_unknown_is_nullish(smelt_value)).unwrap_or_else(|| {fallback_text})",
                    fallback_text = fallback_text.delimited()
                ));
                if dest_ty == *inner {
                    return Ok(coalesced);
                }
                self.rendered_at_type(&coalesced, *inner, dest_ty, &self.render_scope())
            }
            Some(Type::Optional(inner)) => {
                if self.has_dynamic_shape(dest_ty)
                {
                    let optional_text = self.operand_expr(optional)?;
                    let present_text = self.rendered_at_type(&RustExpr::path("smelt_value"), *inner, dest_ty, &self.render_scope())?;
                    let fallback_text = self.value_at_type(fallback, dest_ty)?;
                    return Ok(optional_text.postfix(format!(
                        ".map_or_else(|| {fallback_text}, |smelt_value| {present_text})", fallback_text = fallback_text.delimited(), present_text = present_text.delimited()
                    )));
                }
                let fallback_ty = self.operand_ty(fallback)?;
                // `value ?? null` / `value ?? undefined`: a nullish fallback
                // replaces an absent value with an absent value, so the result
                // is the optional itself. Unwrapping it with the fallback
                // rendered at the INNER type invented a present default
                // (`Some(value.unwrap_or(0.0))`), or dropped the value entirely
                // for an inner type with no default (a function).
                if matches!(self.mir.types.get(fallback_ty), Some(Type::None)) {
                    return self.value_at_type(optional, dest_ty);
                }
                if self.has_dynamic_shape(fallback_ty)
                {
                    let optional_text = self.operand_expr(optional)?;
                    let fallback_text = self.value_at_type(fallback, fallback_ty)?;
                    let mapped_value = self.rendered_at_type(&RustExpr::path("smelt_value"), fallback_ty, *inner, &self.render_scope())?;
                    let fallback_option = self.carrier_format(self.carrier()?, format_args!(
                        "match {fallback_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, smelt_value => Some({mapped_value}) }}", fallback_text = fallback_text.delimited(), mapped_value = mapped_value.delimited()
                    ));
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                    {
                        return Ok(optional_text.postfix(format!(".or({fallback_option})")));
                    }
                    let coalesced = optional_text.postfix(format!(
                        ".unwrap_or_else(|| {fallback_option}.unwrap_or({}))",
                        self.default_value(*inner)?.delimited()
                    ));
                    if dest_ty == *inner {
                        return Ok(coalesced);
                    }
                    return self.rendered_at_type(&coalesced, *inner, dest_ty, &self.render_scope());
                }
                if let Some(Type::Optional(fallback_inner)) = self.mir.types.get(fallback_ty)
                    && fallback_inner == inner
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                {
                    return Ok({ let receiver = self.operand_borrow_expr(optional)?; receiver.postfix(format!(".clone().or({})", self.operand_expr(fallback)?.delimited())) });
                }
                // One clone of the optional: `operand_expr` of a place already
                // ends in `.clone()`, and the coalescing read only borrows.
                let coalesced = { let receiver = self.operand_borrow_expr(optional)?; receiver.postfix(format!(".clone().unwrap_or({})", self.value_at_type(fallback, *inner)?.delimited())) };
                if dest_ty == *inner {
                    Ok(coalesced)
                } else {
                    self.rendered_at_type(&coalesced, *inner, dest_ty, &self.render_scope())
                }
            }
            // A left operand that is statically nullish selects the fallback, and
            // one that is statically non-nullish selects itself. Either way the
            // surviving arm still has to arrive at the destination type: the join
            // of `string | undefined` with `3000` is `string | number`, so the
            // selected arm is lifted into that union instead of being emitted at
            // its own type under the union's declaration.
            Some(Type::None) => self.value_at_type(fallback, dest_ty),
            _ => self.value_at_type(optional, dest_ty),
        }
    }

    /// Emit a source `Option<S>` as a destination `Option<T>`.
    fn optional_inner_map_expr(
        &self,
        value_text: &RustExpr,
        source_inner: TypeId,
        dest_inner: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if source_inner == dest_inner {
            return Ok(value_text.to_owned());
        }
        self.optional_value_map_expr(value_text, source_inner, dest_inner, &self.render_scope())
    }

    /// Emits a JavaScript optional-chain index read against an in-scope receiver value.
    ///
    /// `value?.[index]` short-circuits only when the receiver is nullish, but a
    /// missing array/string element still produces `undefined`. Smelt models
    /// that as `None`, so this helper deliberately avoids the strict
    /// `expect("index out of bounds")` used by normal element access.
    fn optional_index_access_expr(
        &self,
        receiver_text: &RustExpr,
        receiver_ty: TypeId,
        index: &Operand,
        result_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // A numbered group read (`match[n]`) has an `Optional(String)` result, so
        // MIR routes it through `OptionalIndex`; `group_owned` already yields the
        // `Option<String>` this path expects.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_match_class_symbol(*name)?
        {
            return self.match_index_expr(receiver_text, index);
        }
        // A class with an index signature backs keyed reads with a real store
        // field (issue #84). A dynamic `bag[key]` read returns the store value
        // for the key or `None` (missing key -> `undefined`), giving the honest
        // `Option<T>` round-trip result rather than a stub.
        if let Some((key_ty, value_ty)) = self.class_index_store_types(receiver_ty) {
            let store_text = RustExpr::field_on(&receiver_text, format!("{receiver_text}.{CLASS_INDEX_STORE_FIELD}", receiver_text = receiver_text.tight()));
            return self.dict_index_optional_read_expr(&store_text, key_ty, value_ty, index);
        }
        match self.mir.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => {
                let index_text =
                    self.optional_normalized_index_expr(&receiver_text.postfix(".len()"), index)?;
                // Read receiver, not a write one: the `.len()` in the index
                // argument takes a second borrow of the same shared cell, and
                // only shared borrows may coexist. See `place.rs`'s list index
                // read arm for the full argument.
                let read_expr = list_read_expr(receiver_text);
                if let Some(Type::Optional(inner)) = self.mir.types.get(*item_ty)
                    && *inner == result_ty
                {
                    Ok(RustExpr::atom(format!(
                        "({index_text}).and_then(|smelt_index| {read_expr}.get(smelt_index).cloned().flatten())", index_text = index_text.delimited(), read_expr = read_expr.tight()
                    )))
                } else {
                    Ok(RustExpr::atom(format!(
                        "({index_text}).and_then(|smelt_index| {read_expr}.get(smelt_index).cloned())", index_text = index_text.delimited(), read_expr = read_expr.tight()
                    )))
                }
            }
            Some(Type::Dict(key_ty, value_ty)) => {
                self.dict_index_optional_read_expr(receiver_text, *key_ty, *value_ty, index)
            }
            Some(Type::String | Type::PyStr) => {
                let index_text = self.optional_normalized_index_expr(
                    &self.string_len_expr(receiver_text),
                    index,
                )?;
                Ok(RustExpr::atom(format!(
                    "({index_text}).and_then(|smelt_index| {})",
                    self.string_char_at_text(&receiver_text.parenthesized_if_needed(), "smelt_index"), index_text = index_text.delimited()
                )))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if self.has_dynamic_shape(receiver_ty) =>
            {
                let index_ty = self.operand_ty(index)?;
                let index_text = self.operand_expr(index)?;
                let key_text = self.property_key_to_string_expr(&index_text, index_ty)?;
                let numeric_index_text = match self.mir.types.get(index_ty) {
                    Some(Type::Int | Type::Float) => index_text,
                    Some(Type::Bool) => RustExpr::loose(format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}", index_text = index_text.delimited())),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    | Some(Type::Class { .. })
                        if self.is_erased_class_type(index_ty)
                            || matches!(
                                self.mir.types.get(index_ty),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            ) =>
                    {
                        // A concrete generated union is its own Rust enum, not a
                        // `SmeltUnknown`, so it reaches the runtime-narrowing
                        // `match` through its `IntoSmeltUnknown` boundary
                        // adapter — the same treatment the non-optional read
                        // gives this operand (`unknown_index_expr`).
                        let scrutinee = self
                            .erase_concrete_union_expr(&index_text.cloned_value(), index_ty)?;
                        RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                            "match {scrutinee} {{ SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),"), scrutinee = scrutinee.delimited()
                        )))
                    }
                    _ => RustExpr::atom("f64::NAN"),
                };
                // Each arm reads an erased element and the read's declared
                // result type says what the caller wants back. `String` keeps
                // its own spelling (it is the common case and the one every
                // existing golden shows) and an erased result needs no
                // conversion at all; any OTHER concrete result goes through the
                // same `rendered_at_type` boundary conversion the rest of the
                // emitter uses. Without it a `Record<string, [string, number]>`
                // read answered `Option<SmeltUnknown>` into an
                // `Option<(String, f64)>` slot (4 of the router slice's
                // errors) — the arms produced the erased value and nobody
                // converted it.
                let result_is_string = self.mir.types.get(result_ty).is_some_and(Type::is_text);
                let result_is_erased = matches!(self.mir.types.get(result_ty), Some(Type::Unknown));
                // The element this arm reads is a `SmeltUnknown` BY
                // CONSTRUCTION: it comes out of the runtime-narrowing `match`
                // above, not out of a MIR local. So the conversion is an
                // extraction, which is target-only, and asking for a
                // `Type::Unknown` TypeId to spell the source with is asking for
                // something a crate need not have: a crate that never writes
                // `unknown` in source interns no such type, and this seam then
                // aborted emission outright ("type table does not contain
                // literal operand type Unknown"). `coercion`'s module doc
                // states the same rule for the erasing direction; extraction is
                // its mirror and takes it too.
                let element_text = if result_is_erased {
                    "_smelt_element".to_owned()
                } else {
                    self.extract_rendered(&RustExpr::path("_smelt_element"), result_ty, &self.render_scope())?.into_string()
                };
                let char_at = self.string_char_at_text("smelt_value", "smelt_index");
                let string_some = if result_is_string {
                    char_at
                } else if result_is_erased {
                    format!("{char_at}.map(|smelt_ch| SmeltUnknown::String(smelt_ch.into()))")
                } else {
                    self.carrier_format(self.carrier()?, format_args!(
                        "{char_at}.map(|smelt_ch| {{ let _smelt_element = SmeltUnknown::String(smelt_ch.into()); {element_text} }})"
                    ))
                };
                let array_some = if result_is_string {
                    self.carrier_expr(self.carrier()?, &RustExpr::chain(format!(
                        "smelt_values.get(smelt_index).cloned().map(|smelt_value| match smelt_value {{ SmeltUnknown::String(smelt_value) => {}, other => {} }})",
                        self.text_from_unknown_string("smelt_value"),
                        self.text_from_string("other.to_string()"),
                    )))
                } else if result_is_erased {
                    RustExpr::chain("smelt_values.get(smelt_index).cloned()")
                } else {
                    RustExpr::chain(format!(
                        "smelt_values.get(smelt_index).cloned().map(|_smelt_element| {element_text})"
                    ))
                };
                // Read the OBJECT arm through `smelt_get_object_field`, the same
                // helper the erased static field read uses, so `o?.[k]` answers
                // what `o.k` answers: `o` may be a marker record whose property
                // is synthesized rather than stored (`err.name`, `x.constructor`,
                // a Map's `size`, the global object's builtin constructors), and
                // an own-field-only `values.get` was blind to all of them.
                // `Undefined` is the helper's "no such property" answer and maps
                // back to `None`, which is what the optional-read shape means.
                let object_field = format!("smelt_get_object_field(&smelt_values, &{key_text})", key_text = key_text.tight());
                let object_some = if result_is_string {
                    self.carrier_format(self.carrier()?, format_args!(
                        "match {object_field} {{ SmeltUnknown::Undefined => None, SmeltUnknown::String(smelt_value) => Some({}), other => Some({}) }}",
                        self.text_from_unknown_string("smelt_value"),
                        self.text_from_string("other.to_string()"),
                    ))
                } else if result_is_erased {
                    self.carrier_format(self.carrier()?, format_args!("match {object_field} {{ SmeltUnknown::Undefined => None, smelt_value => Some(smelt_value) }}"))
                } else {
                    self.carrier_format(self.carrier()?, format_args!(
                        "match {object_field} {{ SmeltUnknown::Undefined => None, _smelt_element => Some({element_text}) }}"
                    ))
                };
                let primitive_none = self.carrier_format(self.carrier()?, format_args!(
                    "{}SmeltUnknown::Bool(_) | SmeltUnknown::Number(_) | SmeltUnknown::Symbol(_) | SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => None",
                    self.bigint_unknown_arm("SmeltUnknown::BigInt(_) => None,")
                ));
                // Likewise for the receiver: `Optional(SmeltUnion156)` unwraps
                // to the union's own enum, and matching `SmeltUnknown` arms
                // against it read `expected SmeltUnion156, found SmeltUnknown`
                // — 40 of the router slice's 56 errors, all from this one
                // scrutinee. The non-optional index read already erases here
                // (see `place.rs`'s `Type::Union` arm).
                let scrutinee =
                    self.erase_concrete_union_expr(&receiver_text.cloned_value(), receiver_ty)?;
                // A string key is a property name on every arm: `arr["1"]` reads an
                // element, `arr["length"]` a property, `arr["x"]` nothing.
                if self.mir.types.get(self.operand_ty(index)?).is_some_and(Type::is_text) {
                    return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                        "match {scrutinee} {{ smelt_indexed @ (SmeltUnknown::String(_) | SmeltUnknown::Array(_)) => match smelt_get_unknown_field(&smelt_indexed, &{key_text}).clone() {{ SmeltUnknown::Undefined => None, _smelt_element => Some({element_text}) }}, SmeltUnknown::Object(smelt_values) => {object_some}, {primitive_none}, }}", scrutinee = scrutinee.delimited(), key_text = key_text.tight()
                    ))));
                }
                Ok(RustExpr::atom(format!(
                    r"match {scrutinee} {{
                        SmeltUnknown::String(smelt_value) => {{
                            let smelt_len = {string_len} as i64;
                            let smelt_index = {numeric_index_text} as i64;
                            let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }};
                            usize::try_from(smelt_normalized).ok().and_then(|smelt_index| {string_some})
                        }}
                        SmeltUnknown::Array(smelt_values) => {{
                            let smelt_len = smelt_values.len() as i64;
                            let smelt_index = {numeric_index_text} as i64;
                            let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }};
                            usize::try_from(smelt_normalized).ok().and_then(|smelt_index| {array_some})
                        }}
                        SmeltUnknown::Object(smelt_values) => {object_some},
                        {primitive_none},
                    }}",
                    string_len = self.string_len_text("smelt_value"), scrutinee = scrutinee.delimited(), numeric_index_text = numeric_index_text.as_cast_operand(), array_some = array_some.delimited(),
                )))
            }
            // A tuple has a static width, so a constant index names one field.
            // The read answers `Option<result>`: an element that is itself
            // optional maps option to option, and a present element is
            // wrapped. (`args?.[1]` on a tuple rest parameter; see the
            // frontend's `lowering::tuple_rest`.)
            Some(Type::Tuple(items)) => {
                let tuple_index = self.tuple_index(index, items.len())?;
                let element_ty = items
                    .get(tuple_index)
                    .copied()
                    .ok_or_else(|| EmitError::new("tuple index is out of range"))?;
                let field = receiver_text.postfix(format!(".{tuple_index}")).cloned_value();
                if let Some(Type::Optional(element_inner)) = self.mir.types.get(element_ty) {
                    if *element_inner == result_ty {
                        return Ok(field);
                    }
                    return self.optional_value_map_expr(
                        &field,
                        *element_inner,
                        result_ty,
                        &self.render_scope(),
                    );
                }
                if element_ty == result_ty {
                    return Ok(RustExpr::chain(format!("Some({field})", field = field.delimited())));
                }
                Ok(RustExpr::chain(format!(
                    "Some({})",
                    self.rendered_at_type(&field, element_ty, result_ty, &self.render_scope())?.delimited()
                )))
            }
            _ => Ok(RustExpr::path("None")),
        }
    }

    /// Normalize an optional JavaScript array/string read without panicking on misses.
    ///
    /// Indexed source reads whose value is already optional model JavaScript
    /// `undefined`; a negative or out-of-range normalized position therefore
    /// remains `None` instead of entering strict Python-style index behavior.
    fn optional_normalized_index_expr(
        &self,
        len_expr: &RustExpr,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_expr(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_len = {len_expr} as i64; let smelt_index = {index_text} as i64; let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }}; usize::try_from(smelt_normalized).ok() }}", len_expr = len_expr.as_cast_operand(), index_text = index_text.as_cast_operand()
        )))
    }
}
