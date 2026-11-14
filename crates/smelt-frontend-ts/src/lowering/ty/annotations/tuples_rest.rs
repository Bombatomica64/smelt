//! Tuple and rest-parameter annotations: tuple elements, rest tails, minimum
//! arity, and the mutable-tuple parameter ABI.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::{TSTupleElement, TSType, TSTypeName};
use oxc::span::GetSpan;
use smelt_hir::{ErasureReason, Type};

impl ModuleBuilder<'_> {
    /// Convert a function-type rest parameter annotation into its list type,
    /// or into its tuple type when it is a fixed-width tuple (which the caller
    /// expands into fixed parameters; see `lowering::tuple_rest`).
    pub(in crate::lowering) fn function_type_rest_param_to_hir(
        &mut self,
        ty: &TSType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if matches!(ty, TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_)) {
            let item_ty = if matches!(ty, TSType::TSAnyKeyword(_)) {
                self.erase(ErasureReason::ExplicitAny)
            } else {
                self.erase(ErasureReason::SourceUnknown)
            };
            return Ok(self.ctx.krate.types.intern(Type::List(item_ty)));
        }
        if matches!(ty, TSType::TSNeverKeyword(_)) {
            let item_ty = self.ctx.krate.types.intern(Type::Never);
            return Ok(self.ctx.krate.types.intern(Type::List(item_ty)));
        }
        let ty = self.ts_type_to_hir(ty)?;
        if self.fixed_tuple_rest_elements(ty).is_some() {
            return Ok(ty);
        }
        let (list_ty, _) = self.rest_param_array_type(ty)?;
        Ok(list_ty)
    }

    /// Resolve a rest-parameter annotation to the runtime array binding type and item type.
    pub(in crate::lowering) fn rest_param_array_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::TypeId), SmeltError> {
        let ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::List(item_ty)) => Ok((ty, item_ty)),
            Some(Type::Unknown) => {
                // not a new erasure: the rest annotation is already erased
                let item_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            Some(Type::TypeParam { .. }) => {
                let item_ty = self.erase(ErasureReason::UnfilledGeneric);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            Some(Type::Tuple(items)) => {
                let item_ty = self.union_of_types_or_unknown(items);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            Some(Type::Union(items)) => {
                let mut item_tys = Vec::new();
                for item in items {
                    if matches!(self.ctx.krate.types.get(item), Some(Type::Never)) {
                        continue;
                    }
                    let (_, item_ty) = self.rest_param_array_type(item)?;
                    if !item_tys.contains(&item_ty) {
                        item_tys.push(item_ty);
                    }
                }
                let item_ty = self.union_of_types_or_unknown(item_tys);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "rest parameter type must resolve to an array type",
            )),
        }
    }

    /// Lower `[T, ...T[]]`-style tuple aliases to homogeneous list metadata.
    ///
    /// TypeScript uses this spelling to model non-empty arrays. HIR does not
    /// currently track non-empty length refinements, so the useful runtime
    /// shape is the list element type.
    pub(in crate::lowering) fn homogeneous_tuple_rest_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(TSTupleElement::TSRestType(rest)) = tuple.element_types.last() else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        if tuple.element_types.len() <= 1 {
            return Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))));
        }
        let Some(fixed_items) = tuple.element_types.get(..tuple.element_types.len() - 1) else {
            return Ok(None);
        };
        for item in fixed_items {
            if matches!(item, TSTupleElement::TSRestType(_)) {
                return Ok(None);
            }
            if self.tuple_element_type_to_hir(item)? != element_ty {
                return Ok(None);
            }
        }
        Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))))
    }

    /// Return how many leading arguments a rest parameter's source type requires.
    ///
    /// HIR lowers both `...rest: T[]` and `...rest: [T, ...T[]]` to the same
    /// `Type::List(T)`, erasing the required-prefix arity. Overload selection
    /// still needs that arity so a required prefix does not match an empty rest
    /// tail vacuously, so this inspects the *source* annotation:
    ///
    /// * a required-prefix tuple `[A, B, ...T[]]` returns the number of required
    ///   leading elements (`>= 1`).
    /// * Plain `T[]`, `Array<T>`, `ReadonlyArray<T>`, and all-rest tuples
    ///   `[...T[]]` accept an empty tail and return `0`.
    ///
    /// `Readonly`/`Required`/parenthesized wrappers are transparent, matching how
    /// `ts_type_to_hir` already unwraps them.
    pub(in crate::lowering) fn rest_parameter_min_arity(ty: &TSType<'_>) -> usize {
        match ty {
            TSType::TSParenthesizedType(parenthesized) => {
                Self::rest_parameter_min_arity(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                Self::rest_parameter_min_arity(&operator.type_annotation)
            }
            TSType::TSTypeReference(reference) => match &reference.type_name {
                // `Readonly<…>`/`Required<…>` are transparent wrappers.
                TSTypeName::IdentifierReference(name)
                    if matches!(name.name.as_str(), "Readonly" | "Required") =>
                {
                    reference
                        .type_arguments
                        .as_ref()
                        .and_then(|args| args.params.first())
                        .map_or(0, Self::rest_parameter_min_arity)
                }
                _ => 0,
            },
            // A tuple type's required-prefix length, e.g. `[A, B, ...T[]]` => 2.
            // All-rest tuples (`[...T[]]`) and fixed tuples without a rest tail
            // are not required-prefix rests for this purpose, so they yield 0.
            TSType::TSTupleType(tuple) => {
                let has_rest_tail = matches!(
                    tuple.element_types.last(),
                    Some(TSTupleElement::TSRestType(_))
                );
                if !has_rest_tail {
                    return 0;
                }
                tuple
                    .element_types
                    .iter()
                    .take_while(|item| !matches!(item, TSTupleElement::TSRestType(_)))
                    .count()
            }
            _ => 0,
        }
    }

    /// Read the length an ordinary (non-rest) parameter's annotation demands.
    ///
    /// Companion to [`Self::rest_parameter_min_arity`], which answers the same
    /// question for a `...rest` parameter as a plain count. A *fixed* parameter
    /// position needs the distinction between "exactly n" and "at least n":
    ///
    /// * a rest-less tuple `[A, B]` => `Exact(2)` (and `[]` => `Exact(0)`),
    /// * a required-prefix tuple `[T, ...T[]]` => `AtLeast(1)`,
    /// * everything else (`T[]`, `Array<T>`, non-array types) => `None`.
    ///
    /// `Readonly`/`Required`/parenthesized wrappers and the `readonly` operator
    /// are transparent, matching `ts_type_to_hir`.
    pub(in crate::lowering) fn parameter_length_requirement(
        ty: &TSType<'_>,
    ) -> Option<crate::context::ParamLength> {
        use crate::context::ParamLength;
        match ty {
            TSType::TSParenthesizedType(parenthesized) => {
                Self::parameter_length_requirement(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                Self::parameter_length_requirement(&operator.type_annotation)
            }
            TSType::TSTypeReference(reference) => match &reference.type_name {
                TSTypeName::IdentifierReference(name)
                    if matches!(name.name.as_str(), "Readonly" | "Required") =>
                {
                    reference
                        .type_arguments
                        .as_ref()
                        .and_then(|args| args.params.first())
                        .and_then(Self::parameter_length_requirement)
                }
                _ => None,
            },
            TSType::TSTupleType(tuple) => {
                // Every non-rest element is one element the argument must
                // supply, wherever the rest sits: `[T, ...T[]]` and
                // `[...T[], U]` both demand at least one. A rest element or an
                // optional element makes the length a minimum; without either,
                // the tuple pins it exactly.
                let mut fixed = 0_usize;
                let mut optional = 0_usize;
                let mut has_rest = false;
                for element in &tuple.element_types {
                    match element {
                        TSTupleElement::TSRestType(_) => has_rest = true,
                        TSTupleElement::TSOptionalType(_) => {
                            fixed += 1;
                            optional += 1;
                        }
                        _ => fixed += 1,
                    }
                }
                if has_rest || optional > 0 {
                    Some(ParamLength::AtLeast(fixed.saturating_sub(optional)))
                } else {
                    Some(ParamLength::Exact(fixed))
                }
            }
            _ => None,
        }
    }

    /// Lower tuple aliases with a non-`never` rest tail to list metadata.
    pub(in crate::lowering) fn tuple_rest_list_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if !tuple.element_types.is_empty()
            && tuple
                .element_types
                .iter()
                .all(|item| matches!(item, TSTupleElement::TSRestType(_)))
        {
            let mut list_item_ty = None;
            for item in &tuple.element_types {
                let TSTupleElement::TSRestType(rest) = item else {
                    continue;
                };
                let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
                let rest_ty = self.type_param_constraint_or_self(rest_ty);
                let item_ty = match self.ctx.krate.types.get(rest_ty).cloned() {
                    Some(Type::List(item_ty)) => item_ty,
                    Some(Type::Never) => return Ok(None),
                    // not a new erasure: the rest type is already erased
                    Some(Type::Unknown) => self.unknown_type(NotNewErasure::ErasedUpstream),
                    _ => self.erase(ErasureReason::TypeLevel),
                };
                list_item_ty = Some(match list_item_ty {
                    Some(previous) if previous == item_ty => previous,
                    Some(_) => self.erase(ErasureReason::TypeLevel),
                    None => item_ty,
                });
            }
            if let Some(item_ty) = list_item_ty {
                return Ok(Some(self.ctx.krate.types.intern(Type::List(item_ty))));
            }
        }

        let Some(TSTupleElement::TSRestType(rest)) = tuple.element_types.last() else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))))
    }

    /// Lower a tuple whose single rest element is NOT last (e.g. `[...T[], U]`)
    /// to a homogeneous list.
    ///
    /// TypeScript uses leading/middle-rest tuples to model arrays whose leading
    /// portion is variadic with a fixed tail. HIR has no leading-rest tuple
    /// shape, and modelling it as a fixed tuple would silently drop the rest
    /// element (and truncate any literal) because `tuple_rest_type_erases_to_empty`
    /// discards every `TSRestType`. The useful runtime shape is a list of the
    /// union of the rest element type and the fixed (non-rest) element types, so
    /// indexing any position stays well-typed. Trailing-rest and all-rest tuples
    /// are handled earlier by `homogeneous_tuple_rest_type` / `tuple_rest_list_type`,
    /// so this only fires for the otherwise-truncated leading/middle-rest shape.
    pub(in crate::lowering) fn leading_rest_tuple_list_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let rest_positions: Vec<usize> = tuple
            .element_types
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, TSTupleElement::TSRestType(_)))
            .map(|(idx, _)| idx)
            .collect();
        if rest_positions.len() != 1 {
            return Ok(None);
        }
        let Some(rest_index) = rest_positions.first().copied() else {
            return Ok(None);
        };
        if rest_index == tuple.element_types.len() - 1 {
            return Ok(None); // trailing rest: handled by tuple_rest_list_type
        }
        let Some(TSTupleElement::TSRestType(rest)) = tuple.element_types.get(rest_index) else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        let mut members = vec![element_ty];
        for (idx, item) in tuple.element_types.iter().enumerate() {
            if idx == rest_index {
                continue;
            }
            let item_ty = self.tuple_element_type_to_hir(item)?;
            if !members.contains(&item_ty) {
                members.push(item_ty);
            }
        }
        let element = if let Some(member) = members.first().copied().filter(|_| members.len() == 1)
        {
            member
        } else {
            self.ctx.krate.types.intern(Type::Union(members))
        };
        Ok(Some(self.ctx.krate.types.intern(Type::List(element))))
    }

    /// Convert tuple element type to HIR type.
    pub(in crate::lowering) fn tuple_element_type_to_hir(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match item {
            TSTupleElement::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSTupleElement::TSBigIntKeyword(_) => Ok(self.ctx.krate.types.intern(Type::BigInt)),
            TSTupleElement::TSStringKeyword(_) | TSTupleElement::TSTemplateLiteralType(_) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSTupleElement::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSTupleElement::TSAnyKeyword(_) => Ok(self.erase(ErasureReason::ExplicitAny)),
            TSTupleElement::TSUnknownKeyword(_) => Ok(self.erase(ErasureReason::SourceUnknown)),
            TSTupleElement::TSNeverKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Never)),
            TSTupleElement::TSNullKeyword(_)
            | TSTupleElement::TSUndefinedKeyword(_)
            | TSTupleElement::TSVoidKeyword(_) => Ok(self.ctx.krate.types.intern(Type::None)),
            TSTupleElement::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSTupleElement::TSTupleType(tuple) => {
                if let Some(ty) = self.homogeneous_tuple_rest_type(tuple)? {
                    return Ok(ty);
                }
                if let Some(ty) = self.tuple_rest_list_type(tuple)? {
                    return Ok(ty);
                }
                if let Some(ty) = self.leading_rest_tuple_list_type(tuple)? {
                    return Ok(ty);
                }
                let mut items = Vec::new();
                for tuple_item in &tuple.element_types {
                    if self.tuple_rest_type_erases_to_empty(tuple_item)? {
                        continue;
                    }
                    items.push(self.tuple_element_type_to_hir(tuple_item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSTupleElement::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSTupleElement::TSTypeLiteral(literal) => self.type_literal_to_hir(literal),
            TSTupleElement::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BigIntLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::BigInt))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Ok(self.erase(ErasureReason::UnmodeledType)),
            },
            TSTupleElement::TSIndexedAccessType(_) => Ok(self.erase(ErasureReason::TypeLevel)),
            TSTupleElement::TSConditionalType(conditional) => {
                let true_ty = self.ts_type_to_hir(&conditional.true_type)?;
                let false_ty = self.ts_type_to_hir(&conditional.false_type)?;
                if true_ty == false_ty {
                    Ok(true_ty)
                } else {
                    Ok(self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Union(vec![true_ty, false_ty])))
                }
            }
            TSTupleElement::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSTupleElement::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSTupleElement::TSParenthesizedType(parenthesized) => {
                self.ts_type_to_hir(&parenthesized.type_annotation)
            }
            TSTupleElement::TSFunctionType(function) => self.function_type_to_hir(function),
            TSTupleElement::TSUnionType(union) => {
                let mut lowered = Vec::new();
                let mut nullish = Vec::new();
                let mut saw_never = false;
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::Never)) {
                        saw_never = true;
                    } else if matches!(self.ctx.krate.types.get(member_ty), Some(Type::None)) {
                        nullish.push(member_ty);
                    } else if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if let Some(list_ty) = self.list_union_type(&lowered) {
                    if nullish.is_empty() {
                        return Ok(list_ty);
                    }
                    lowered = vec![list_ty];
                }
                if let Some(dict_ty) = self.dict_union_type(&lowered) {
                    if nullish.is_empty() {
                        return Ok(dict_ty);
                    }
                    lowered = vec![dict_ty];
                }
                if lowered.len() == 1 && !nullish.is_empty() {
                    let single = lowered.remove(0);
                    Ok(self.ctx.krate.types.intern(Type::Optional(single)))
                } else if lowered.len() == 1 {
                    let single = lowered.remove(0);
                    Ok(single)
                } else if lowered.is_empty() && nullish.is_empty() && saw_never {
                    Ok(self.ctx.krate.types.intern(Type::Never))
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSTupleElement::TSOptionalType(optional) => {
                let inner = self.ts_type_to_hir(&optional.type_annotation)?;
                Ok(self.ctx.krate.types.intern(Type::Optional(inner)))
            }
            TSTupleElement::TSRestType(rest) => Err(SmeltError::unsupported(
                self.span(rest.span.start, rest.span.end),
                "tuple rest types are only lowered for never tails",
            )),
            // A named member keeps its optional mark (`[key?: string]`) as the
            // unnamed `[string?]` form does: an absent element is `undefined`,
            // which only an `Optional` can hold apart from `""`.
            TSTupleElement::TSNamedTupleMember(named) => {
                let inner = self.tuple_element_type_to_hir(&named.element_type)?;
                Ok(if named.optional {
                    self.ctx.krate.types.intern(Type::Optional(inner))
                } else {
                    inner
                })
            }
            // A tuple element that is not one of the tuple-only forms above
            // (`TSOptionalType`, `TSRestType`, `TSNamedTupleMember`) IS an
            // ordinary type: `TSTupleElement` inherits every `TSType` variant.
            // The arms above are shortcuts, not the whole grammar, so anything
            // they do not name is delegated to `ts_type_to_hir` rather than
            // rejected. Before this, a tuple element written as an intersection
            // (`[H<E2, P, I> & M1, H<E3, P, I2, R>]`) was refused even though
            // `ts_type_to_hir` has lowered intersections for a long time — and
            // so was every other `TSType` shape nobody had happened to add here.
            element => match element.as_ts_type() {
                Some(ts_type) => self.ts_type_to_hir(ts_type),
                None => Err(SmeltError::unsupported(
                    self.span(item.span().start, item.span().end),
                    format!("tuple element type is not lowered yet: {item:?}"),
                )),
            },
        }
    }

    /// Infer function parameters that represent returned structural state.
    ///
    /// Some TypeScript APIs model mutation by returning either a value or a
    /// tuple containing the updated state object. When a function field has
    /// that shape, generated Rust can pass the state parameter by `&mut`
    /// instead of cloning it through an adapter and losing identity.
    pub(in crate::lowering) fn mutable_params_from_returned_tuple_state(
        &self,
        params: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
    ) -> Vec<usize> {
        params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                (self.mutable_abi_param_type(*param)
                    && self.return_type_tuple_contains(return_ty, *param))
                .then_some(index)
            })
            .collect()
    }

    /// Return whether a parameter type can use Rust mutable-reference ABI.
    pub(in crate::lowering) fn mutable_abi_param_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { .. } | Type::List(_) | Type::Set(_) | Type::Dict(_, _)) => true,
            Some(Type::Optional(inner)) => self.mutable_abi_param_type(*inner),
            _ => false,
        }
    }

    /// Return whether a return type has a tuple branch containing `target`.
    pub(in crate::lowering) fn return_type_tuple_contains(
        &self,
        ty: smelt_hir::TypeId,
        target: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Tuple(items)) => items.contains(&target),
            Some(Type::Union(items)) => items
                .iter()
                .any(|item| self.return_type_tuple_contains(*item, target)),
            Some(Type::Optional(inner) | Type::Future(inner)) => {
                self.return_type_tuple_contains(*inner, target)
            }
            _ => false,
        }
    }

    /// Return whether a tuple rest element can be erased from HIR tuple metadata.
    ///
    /// TypeScript libraries use `[...never[]]` and occasionally `...never` as
    /// type-level variadic filters. Those tails cannot contribute runtime tuple
    /// elements, so Smelt erases them to an empty metadata tail. Generic tuple
    /// accumulators constrained to arrays are also erased because HIR has no
    /// open tuple-tail type and frontend type checking has already validated
    /// the source-level spread shape.
    pub(in crate::lowering) fn tuple_rest_type_erases_to_empty(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<bool, SmeltError> {
        let TSTupleElement::TSRestType(rest) = item else {
            return Ok(false);
        };
        let _ = self.ts_type_to_hir(&rest.type_annotation)?;
        Ok(true)
    }
}
