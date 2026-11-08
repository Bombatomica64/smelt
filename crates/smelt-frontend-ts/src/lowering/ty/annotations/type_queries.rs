//! Queries over lowered HIR types: string compatibility, binary result types,
//! unknown and nullish membership, `never` values, indexed element types, static
//! tuple indices and promise inner types.

use crate::lowering::erasure::NotNewErasure;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use smelt_hir::{BinOp, Body, ErasureReason, ExprKind, Literal, Type, UnaryOp};

impl ModuleBuilder<'_> {
    /// Return whether a concrete value of this type would contain a `never` value.
    pub(in crate::lowering) fn concrete_type_requires_never_value(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Never) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.concrete_type_requires_never_value(*item)
            }
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.concrete_type_requires_never_value(*key)
                    || self.concrete_type_requires_never_value(*value)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .any(|item| self.concrete_type_requires_never_value(*item)),
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.concrete_type_requires_never_value(*item)),
            Some(Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            }) => {
                self.concrete_type_requires_never_value(*yield_ty)
                    || self.concrete_type_requires_never_value(*return_ty)
                    || self.concrete_type_requires_never_value(*next_ty)
            }
            Some(Type::GeneratorResult {
                yield_ty,
                return_ty,
            }) => {
                self.concrete_type_requires_never_value(*yield_ty)
                    || self.concrete_type_requires_never_value(*return_ty)
            }
            Some(
                Type::Bool
                | Type::Int
                | Type::FixedInt(_)
                | Type::BigInt
                | Type::Float
                | Type::String
                | Type::PyStr
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::Function(_)
                | Type::None,
            )
            | None => false,
        }
    }

    /// Return whether an array or tuple literal would need to materialize `never`.
    pub(in crate::lowering) fn array_literal_needs_never_value(&self, ty: smelt_hir::TypeId, item_count: usize) -> bool {
        if item_count == 0 {
            return false;
        }
        match self.ctx.krate.types.get(ty) {
            Some(Type::List(item) | Type::Set(item)) => {
                self.concrete_type_requires_never_value(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .take(item_count)
                .any(|item| self.concrete_type_requires_never_value(*item)),
            _ => false,
        }
    }

    /// Get the element type of an indexable type.
    pub(in crate::lowering) fn index_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let receiver_ty = self.type_param_constraint_or_self(receiver_ty);
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::String) => Ok(self.ctx.krate.types.intern(Type::String)),
            Some(Type::Dict(_, value)) => Ok(*value),
            // not a new erasure: the receiver is already erased
            Some(Type::Unknown) => Ok(self.unknown_type(NotNewErasure::ErasedUpstream)),
            Some(Type::None) => Ok(self.erase(ErasureReason::DynamicOperator)),
            // Dynamically indexing a JavaScript boolean primitive with a key is a
            // property lookup on the autoboxed `Boolean` wrapper. For an
            // arbitrary key that has no own indexed element this yields
            // `undefined`, so the element type is the dynamic `Unknown` boundary
            // rather than a lowering failure. This is the shape produced by
            // `(a && b)[key]` chains whose `&&` short-circuit value is a boolean.
            // Numeric primitives are intentionally excluded: iterating or
            // indexing a bare number is almost always a real source error (e.g.
            // `for (x of 1)`) that should stay an honest blocker.
            Some(Type::Bool) => Ok(self.erase(ErasureReason::DynamicOperator)),
            // A numbered index into a RegExp match result (`m[0]`, `m[2]`) reads a
            // capture group: a string when the group matched and `undefined`
            // otherwise, so the element type is `Optional(String)` rather than the
            // erased dynamic boundary.
            // An index into a `BigInt64Array`/`BigUint64Array` reads a bigint.
            // (The other views' element reads keep the erased answer they have
            // always had.)
            Some(Type::Class { .. }) if self.is_bigint_view_type(receiver_ty) => {
                Ok(self.ctx.krate.types.intern(Type::BigInt))
            }
            Some(Type::Class { name, .. }) if self.match_stdlib_class(*name).is_some() => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(self.ctx.krate.types.intern(Type::Optional(string_ty)))
            }
            // A class with a declared string/number index signature keeps a keyed
            // store whose value type `T` is statically known. A dynamic keyed read
            // `instance[key]` may name a missing key, so — following JavaScript's
            // missing-key-is-`undefined` semantics — the honest element type is
            // `Optional<T>` rather than the erased `Unknown` boundary. Classes with
            // no index signature still fall through to `Unknown` below.
            Some(Type::Class { name, .. })
                if self.classes.has_index_value(*name) =>
            {
                let value_ty = self.classes.index_value(*name)
                    .unwrap_or_else(|| self.erase(ErasureReason::DynamicOperator));
                Ok(self.optional_chain_result_type(value_ty))
            }
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                Ok(self.erase(ErasureReason::DynamicOperator))
            }
            Some(Type::Tuple(items)) => {
                let mut union_items = Vec::new();
                for item in items.clone() {
                    if !union_items.contains(&item) {
                        union_items.push(item);
                    }
                }
                match union_items.as_slice() {
                    [single] => Ok(*single),
                    [] => Ok(self.erase(ErasureReason::DynamicOperator)),
                    _ => Ok(self.ctx.krate.types.intern(Type::Union(union_items))),
                }
            }
            Some(Type::Union(items)) => {
                let mut union_items = Vec::new();
                for item in items.clone() {
                    if let Ok(item_ty) = self.index_type(item)
                        && !union_items.contains(&item_ty)
                    {
                        union_items.push(item_ty);
                    }
                }
                match union_items.as_slice() {
                    [single] => Ok(*single),
                    [] => Ok(self.erase(ErasureReason::DynamicOperator)),
                    _ => Ok(self.ctx.krate.types.intern(Type::Union(union_items))),
                }
            }
            None => Ok(self.erase(ErasureReason::Unclassified)),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "index access is only lowered for arrays, strings, and records for now (receiver: {:?})",
                    self.ctx.krate.types.get(receiver_ty)
                ),
            )),
        }
    }

    /// Return whether a type can be represented as a string at runtime.
    pub(in crate::lowering) fn is_string_compatible_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::String | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            Some(Type::Optional(item)) => self.is_string_compatible_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.is_string_compatible_type(item)),
            _ => false,
        }
    }

    /// Infer the HIR result type for a lowered JavaScript binary operation.
    pub(in crate::lowering) fn binary_result_type(
        &mut self,
        op: BinOp,
        lhs_ty: smelt_hir::TypeId,
        rhs_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if matches!(
            op,
            BinOp::Eq
                | BinOp::NotEq
                | BinOp::StrictEq
                | BinOp::StrictNotEq
                | BinOp::JsStrictEq
                | BinOp::JsStrictNotEq
                | BinOp::Lt
                | BinOp::Lte
                | BinOp::Gt
                | BinOp::Gte
        ) {
            return self.ctx.krate.types.intern(Type::Bool);
        }
        if op == BinOp::Add && (self.has_static_string_type(lhs_ty) || self.has_static_string_type(rhs_ty)) {
            return self.ctx.krate.types.intern(Type::String);
        }
        // A bigint operand makes every arithmetic, bitwise and shift result a
        // bigint: TypeScript rejects mixing it with a `number`, and an erased
        // partner either holds a bigint at run time or throws `TypeError`.
        // Typing the result `number` (the erased-numeric rule below) would
        // round it through `f64`.
        if self.is_bigint_type(lhs_ty) || self.is_bigint_type(rhs_ty) {
            return self.ctx.krate.types.intern(Type::BigInt);
        }
        // An operand that may hold EITHER a bigint or a number (a
        // `number | bigint` union) gives a result that may be either: two
        // bigints add to a bigint and two numbers to a number, decided at run
        // time. `number` (below) would drop the bigint arm.
        if !matches!(op, BinOp::And | BinOp::Or | BinOp::IMul)
            && (self.type_may_hold_bigint(lhs_ty) || self.type_may_hold_bigint(rhs_ty))
        {
            let float = self.ctx.krate.types.intern(Type::Float);
            let bigint = self.ctx.krate.types.intern(Type::BigInt);
            return self.ctx.krate.types.intern(Type::Union(vec![float, bigint]));
        }
        if matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Rem
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
        ) && (self.is_erased_numeric_operand(lhs_ty)
            || self.is_erased_numeric_operand(rhs_ty)
            || self.is_optional_numeric_operand(lhs_ty)
            || self.is_optional_numeric_operand(rhs_ty))
        {
            return self.ctx.krate.types.intern(Type::Float);
        }
        lhs_ty
    }

    /// Return whether arithmetic should coerce an optional numeric operand to a number.
    pub(in crate::lowering) fn is_optional_numeric_operand(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Optional(inner)) => self.is_numeric_like_type(*inner),
            _ => false,
        }
    }

    /// Return whether an operand should use erased numeric arithmetic.
    pub(in crate::lowering) fn is_erased_numeric_operand(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        )
    }

    /// Return whether a type is known to contain a string operand for `+`.
    pub(in crate::lowering) fn has_static_string_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::String) => true,
            Some(Type::Optional(item)) => self.has_static_string_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.has_static_string_type(item)),
            _ => false,
        }
    }

    /// Return whether a type contains `unknown` after unwrapping simple containers.
    pub(in crate::lowering) fn type_contains_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) => true,
            Some(Type::Optional(item)) => self.type_contains_unknown(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_contains_unknown(item)),
            _ => false,
        }
    }

    /// Return whether a type can hold a TypeScript nullish value at runtime.
    pub(in crate::lowering) fn is_nullishable_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None) => true,
            Some(Type::Union(items)) => items.iter().copied().any(|item| {
                self.ctx.krate.types.get(item) == Some(&Type::None)
                    || self.is_nullishable_type(item)
            }),
            _ => false,
        }
    }

    /// Return whether a union contains at least one string-compatible branch.
    pub(in crate::lowering) fn union_has_string_compatible_member(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::String) => true,
            Some(Type::Optional(item)) => self.union_has_string_compatible_member(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.union_has_string_compatible_member(item)),
            _ => false,
        }
    }

    /// Resolve the static numeric index required for TypeScript tuple indexing.
    pub(in crate::lowering) fn static_tuple_index(
        &self,
        index_expr_id: smelt_hir::ExprId,
        body: &Body,
        len: usize,
        span: oxc::span::Span,
    ) -> Result<usize, SmeltError> {
        let Some(index_expr) = body.expr_at(index_expr_id)
        else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple index expression is invalid",
            ));
        };
        let ExprKind::Literal(Literal::Float(value)) = &index_expr.kind else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires a static non-negative integer index",
            ));
        };
        let index_value = *value;
        if index_value < 0.0_f64 {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "negative tuple bracket indexes are JavaScript property lookups; use .at(...) when supported for negative element indexing",
            ));
        }
        if index_value.fract() != 0.0_f64 {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires an integer index",
            ));
        }
        let resolved_index = index_value.to_string().parse::<usize>().map_err(|_err| {
            SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires a representable usize index",
            )
        })?;
        if resolved_index >= len {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple index is out of bounds",
            ));
        }
        Ok(resolved_index)
    }

    /// Returns whether a lowered expression is a negative numeric literal.
    ///
    /// Used to recognize the JavaScript property-lookup class of bracket access
    /// (`arr[-1]`, `arr[-(2)]`) so it can be lowered with `undefined`/no-op
    /// semantics rather than element indexing. See the `negative_index` lowering
    /// module for how reads and writes are lowered.
    pub(in crate::lowering) fn is_negative_numeric_expr(body: &Body, expr_id: smelt_hir::ExprId) -> bool {
        let Some(candidate) = body.expr_at(expr_id) else {
            return false;
        };
        match &candidate.kind {
            ExprKind::Literal(Literal::Float(value)) => *value < 0.0,
            ExprKind::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => body.expr_at(*operand)
                .is_some_and(|operand_expr| {
                    matches!(operand_expr.kind, ExprKind::Literal(Literal::Float(_)))
                }),
            _ => false,
        }
    }

    /// Return the inner item type for a `Promise<T>` / `Future<T>` value.
    pub(in crate::lowering) fn future_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Return the resolved item type for any awaitable actual value.
    ///
    /// A statically-typed `Promise<T>` / `Future<T>` yields its inner `T`. An
    /// erased actual (`Type::Unknown`) is a `SmeltUnknown::Promise` thunk — for
    /// example an `async` call across an import boundary whose generic return
    /// type was erased — and resolves to an erased `Unknown` value once awaited.
    /// Anything else is not awaitable and returns `None`. This keeps the Vitest
    /// `resolves`/`rejects` matchers general over both concrete and erased
    /// promise actuals instead of demanding a statically-spelled `Promise<T>`.
    pub(in crate::lowering) fn awaitable_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            // An erased actual is itself the `Unknown` type id; awaiting it
            // yields another erased value, so reuse the same id rather than
            // re-interning.
            Some(Type::Unknown) => Some(ty),
            _ => None,
        }
    }
}
