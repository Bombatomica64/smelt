//! Containment checks: whether an actual value holds an expected element,
//! substring, key or member (`toContain`, `toThrow("message")`, a rejected
//! promise's message), across typed strings, lists, sets, tuples, optionals and
//! erased values.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, PrimitiveCastOp, Type, UnknownKind};

impl ModuleBuilder<'_> {
    /// Create a containment expression for synthesized test assertions.
    pub(in crate::lowering) fn contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let expected_ty = Self::expr_ty(body, expected);
        // The expected value may be erased (`Unknown`) when it comes from a
        // cross-module helper, as in `expect(array).toContain(sample(array))`.
        // A concrete collection actual still supports the runtime containment
        // check against such a value, so an erased expected matches any item
        // type rather than forcing a static element-type equality.
        let expected_is_erased =
            matches!(self.ctx.krate.types.get(expected_ty), Some(Type::Unknown));
        // A `sample(...)`-style helper returns `T | undefined`, so the expected
        // needle is commonly an `Optional(T)` while the actual collection holds
        // `T` (`expect(array).toContain(sample(array))`). JavaScript containment
        // compares the needle against each element regardless of the needle's
        // nullability, so an optional expected whose inner type matches the
        // element type (or is itself erased) is accepted here; the emitter guards
        // the optional at runtime (a `None`/`undefined` needle is never contained
        // in a collection of non-optional elements).
        // Unwrap an optional expected to its inner type; a `None`/`undefined`
        // needle simply never matches, and the emitter guards it at runtime. The
        // containment is supported when the inner type is exactly the element
        // type, or when the inner type is erased (`unknown`/leaked type param):
        // an erased needle is compared against each element after the element is
        // itself erased to the runtime value (JS `includes`/`has` semantics),
        // exactly like a bare erased expected.
        let expected_inner = match self.ctx.krate.types.get(expected_ty) {
            Some(Type::Optional(inner, _)) => Some(*inner),
            _ => None,
        };
        let expected_inner_is_erased = expected_inner.is_some_and(|inner| {
            matches!(
                self.ctx.krate.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        });
        let item_matches = |item_ty: smelt_hir::TypeId| {
            expected_ty == item_ty
                || expected_is_erased
                || expected_inner == Some(item_ty)
                || expected_inner_is_erased
        };
        let kind = match self.ctx.krate.types.get(Self::expr_ty(body, actual)) {
            Some(Type::String)
                if self.ctx.krate.types.get(expected_ty) == Some(&Type::String)
                    || expected_is_erased =>
            {
                ExprKind::StringContains {
                    haystack: actual,
                    needle: expected,
                    from_index: None,
                }
            }
            Some(Type::List(item_ty)) if item_matches(*item_ty) => ExprKind::ListContains {
                list: actual,
                item: expected,
            },
            Some(Type::Set(item_ty)) if item_matches(*item_ty) => ExprKind::SetContains {
                set: actual,
                item: expected,
            },
            Some(Type::Tuple(items)) if items.iter().any(|item| item_matches(*item)) => {
                ExprKind::TupleContains {
                    tuple: actual,
                    item: expected,
                }
            }
            // `string | null` (`headers.get(name)`) and every other optional
            // actual: vitest rejects a nullish actual, so an absent value never
            // contains anything, and a present one is checked at its inner type.
            Some(Type::Optional(inner, _)) => {
                let inner_ty = *inner;
                return self.optional_contains_expr(actual, inner_ty, expected, span, body);
            }
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                return Ok(self.erased_contains_expr(actual, expected, span, body));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "expect(...).toContain(...) requires a string, array, set, or tuple actual value with a matching expected value",
                ));
            }
        };
        Ok(body.push_expr(Expr {
            kind,
            ty: bool_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Containment over an optional actual (`string | null`, `T[] | undefined`).
    ///
    /// vitest's `toContain`/`toMatch` reject a nullish actual, so the check is
    /// `actual != null && contains(actual!, expected)`: the presence test
    /// guards a `TypeAssert` to the inner type, and the inner containment is
    /// whatever [`Self::contains_expr`] picks for that type — the typed string,
    /// list, set or tuple path, or `erased_contains_expr` for an erased inner
    /// type. The `Conditional` keeps
    /// the unwrap from running when the value is absent.
    fn optional_contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        inner_ty: smelt_hir::TypeId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span: hir_span,
        });
        let present = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: actual,
                rhs: none,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: actual },
            ty: inner_ty,
            span: hir_span,
        });
        let contains = self.contains_expr(value, expected, span, body)?;
        let absent = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: hir_span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: present,
                then_expr: contains,
                else_expr: absent,
            },
            ty: bool_ty,
            span: hir_span,
        }))
    }

    /// Containment over an erased actual (`unknown`, `any`, a leaked type param).
    ///
    /// DYNAMIC BOUNDARY: the actual has no static shape here — it is source
    /// `unknown`/`any`, or a cross-module helper's result that does not resolve
    /// in this lowering unit (`expect(keysIn(buffer)).toContain(k)`,
    /// `expect(JSON.parse(text)).toContain('x')`) — and vitest's `toContain`
    /// picks its semantics from the LIVE value: a string actual is a substring
    /// search, an array actual an element search with `===`, and anything else
    /// fails the assertion. No concrete type, union or scoped generic can make
    /// that choice at compile time, because the source never names one; the
    /// only thing that knows is the value's runtime tag. So the check reads it
    /// (`UnknownIs`) and dispatches:
    ///
    /// * string → `StringContains` over the value viewed as a string, with the
    ///   needle converted by JavaScript `ToString` (what `String.prototype.
    ///   includes` does to a non-string argument);
    /// * array → `ListContains` over the value viewed as an erased list, with
    ///   the needle erased to the same runtime representation;
    /// * otherwise → `false`, so the assertion fails as vitest's does.
    ///
    /// A statically typed actual never reaches this: string/list/set/tuple and
    /// optional actuals keep their typed paths in [`Self::contains_expr`]; an erased
    /// inner type of an optional actual takes this path.
    fn erased_contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // not a new erasure: only an already-erased actual reaches this path
        let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let expected_ty = Self::expr_ty(body, expected);
        let value = Self::erase_to_unknown(actual, unknown_ty, hir_span, body);

        let is_string = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::String,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let haystack = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: string_ty,
            span: hir_span,
        });
        let needle = if expected_ty == string_ty {
            expected
        } else {
            body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand: expected,
                },
                ty: string_ty,
                span: hir_span,
            })
        };
        let string_contains = body.push_expr(Expr {
            kind: ExprKind::StringContains {
                haystack,
                needle,
                from_index: None,
            },
            ty: bool_ty,
            span: hir_span,
        });

        let is_array = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::Array,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let list = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: list_ty,
            span: hir_span,
        });
        let item = if expected_ty == unknown_ty {
            expected
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: expected },
                ty: unknown_ty,
                span: hir_span,
            })
        };
        let list_contains = body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty: bool_ty,
            span: hir_span,
        });
        let neither = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: hir_span,
        });
        let array_or_nothing = body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_array,
                then_expr: list_contains,
                else_expr: neither,
            },
            ty: bool_ty,
            span: hir_span,
        });
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_string,
                then_expr: string_contains,
                else_expr: array_or_nothing,
            },
            ty: bool_ty,
            span: hir_span,
        })
    }
}
