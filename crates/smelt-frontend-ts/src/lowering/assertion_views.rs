//! Assertion views: `const view = erased as T` keeps the erased value.
//!
//! A type assertion does nothing at run time, so after
//! `const pattern = x as Matcher` the binding holds whatever `x` holds. When
//! `x` is erased (`unknown`/`any`), materializing a `Matcher` struct at the
//! declaration is a checked conversion that `x` need not pass, and code that
//! asserts and then *inspects* is exactly code that does not know yet:
//! ts-pattern's type guard
//!
//! ```ts
//! const isMatcher = (x: unknown): x is Matcher => {
//!   const pattern = x as Matcher;
//!   return pattern && !!pattern[symbols.matcher];
//! };
//! ```
//!
//! is called with every pattern a user writes: arrays, strings, numbers, `null`,
//! plain objects. Materializing panicked on each non-object ("expected Matcher,
//! got array"), and on a plain object it produced a struct whose method is
//! always present, so `!!pattern[symbols.matcher]` folded to `true` and every
//! object counted as a matcher.
//!
//! # The rule
//!
//! A `const` binding with no annotation whose initializer is `operand as T` (or
//! `<T>operand`), where `operand` is a plain read that lowers to an erased
//! value and `T` is a structural object type (an interface or object type
//! literal: a generated struct, or a record) rather than a class, is
//! an *assertion view*: the local holds the erased value itself, and the
//! scope remembers `T` ([`crate::lowering::state::local_scope::LocalScope::mark_assertion_view`]).
//!
//! * A read of the view as a value stands for `T`: it is the same checked
//!   `UnknownCast` the declaration used to perform, moved to the use
//!   (`expr::references`). A value that does not fit fails where it is used as
//!   a `T`, as the Rust port would.
//! * A read that only probes the view reads the erased value: a bracket read
//!   (`view[key]`) is the property lookup on the value
//!   ([`ModuleBuilder::assertion_view_read`]), and a truthiness test (`if
//!   (view)`, `!view`, `view && guard`) tests the value.
//! * A dot read or a method call (`view.member`, `view.method()`) uses the view
//!   as a `T`, so it reads the materialized `T`: the struct and its typed
//!   slots, as without a view. A `T` has a member it declares; a bracket key
//!   (a symbol, a computed name) is how a guard probes for one it may lack.
//!
//! # Why the local is erased
//!
//! The binding holds a value whose shape the program has not established: it
//! is the input of a runtime check. That is the dynamic boundary
//! `SmeltUnknown` exists for (a value inspected through runtime narrowing),
//! and no concrete type can hold it: an array is not a `Matcher`. The
//! regression test `assertion_view_probes` passes arrays, strings, numbers,
//! `null` and plain objects through such a guard.
//!
//! The view is a copy of the operand, so a later write to the operand does not
//! reach it, as with any `const`.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::peel::{Wrappers, peel};
use oxc::ast::ast::{
    BindingPattern, Expression, TSType, TSTypeName, VariableDeclarationKind, VariableDeclarator,
};
use smelt_hir::{Body, Expr, ExprKind, LocalDecl, Pattern, Stmt, Type};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower `const view = erased as T` as an assertion view; see the module
    /// docs. Returns `false`, having lowered nothing, for any other declarator.
    pub(in crate::lowering) fn assertion_view_declarator(
        &mut self,
        kind: VariableDeclarationKind,
        declarator: &VariableDeclarator<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if kind != VariableDeclarationKind::Const || declarator.type_annotation.is_some() {
            return Ok(false);
        }
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return Ok(false);
        };
        let Some((operand, annotation)) = declarator.init.as_ref().and_then(asserted_operand)
        else {
            return Ok(false);
        };
        if !is_plain_read(operand) {
            return Ok(false);
        }
        // A class keeps the ordinary assertion: an erased instance converts
        // back to its class with its identity, and its object type is not a
        // shape the value may lack but the instance itself.
        if let TSType::TSTypeReference(reference) = annotation
            && let TSTypeName::IdentifierReference(name) = &reference.type_name
            && self.classes.contains(name.name.as_str())
        {
            return Ok(false);
        }
        let Ok(asserted) = self.ts_type_to_hir(annotation) else {
            return Ok(false);
        };
        if !matches!(
            self.ctx.krate.types.get(asserted),
            Some(Type::Class { .. } | Type::Dict(..))
        ) {
            return Ok(false);
        }
        let mark = self.speculation_mark(body);
        let value = self.expression(operand, body)?;
        // not a new erasure: compared against, and the operand is already erased
        let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        if Self::expr_ty(body, value) != unknown_ty {
            self.roll_back_to(mark, body);
            return Ok(false);
        }
        let symbol = self.intern_source_name(binding.name.as_str());
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty: unknown_ty,
            mutable: false,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.scope.bind(binding.name.as_str().to_owned(), local);
        self.scope
            .mark_assertion_view(local, binding.name.as_str().to_owned(), asserted);
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat,
                ty: unknown_ty,
                value: Some(value),
            },
        );
        Ok(true)
    }

    /// The erased value behind `expression` when it names an assertion view
    /// (through parentheses), for a read that only probes it: a member read or
    /// a truthiness test. `None` for any other expression.
    pub(in crate::lowering) fn assertion_view_read(
        &self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(identifier) = peel(expression, Wrappers::Parens) else {
            return None;
        };
        let local = self.scope.lookup(identifier.name.as_str())?;
        body.local_at(local)?;
        self.scope.assertion_view(local, identifier.name.as_str())?;
        Some(body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty: Self::local_ty(body, local),
            span: self.span(identifier.span.start, identifier.span.end),
        }))
    }

    /// `view && guard` in a value position, where `view` is an assertion view
    /// and `guard` is a boolean: the guard behind the view's truthiness.
    ///
    /// TypeScript types the expression as the guard's `boolean` (an object type
    /// is never falsy to the checker), and a falsy view at run time is a value
    /// outside that type; within it, the expression is `false`. This is the
    /// guard shape of a type predicate (`return view && !!view[key]`). `None`,
    /// having lowered nothing, for any other expression.
    pub(in crate::lowering) fn assertion_view_guard(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != oxc::ast::ast::LogicalOperator::And {
            return Ok(None);
        }
        let mark = self.speculation_mark(body);
        let Some(view) = self.assertion_view_read(&logical.left, body) else {
            return Ok(None);
        };
        let span = self.span(logical.span.start, logical.span.end);
        let lhs = self.lowered_condition_expression(view, span, body)?;
        let rhs = self.expression(&logical.right, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if Self::expr_ty(body, rhs) != bool_ty {
            self.roll_back_to(mark, body);
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: smelt_hir::BinOp::And,
                lhs,
                rhs,
            },
            ty: bool_ty,
            span,
        })))
    }
}

/// The operand and asserted type of `(operand as T)` / `<T>operand`, through parentheses.
pub(in crate::lowering) fn asserted_operand<'e, 'a>(
    object: &'e Expression<'a>,
) -> Option<(&'e Expression<'a>, &'e TSType<'a>)> {
    match object {
        Expression::ParenthesizedExpression(inner) => asserted_operand(&inner.expression),
        Expression::TSAsExpression(assertion) => {
            Some((&assertion.expression, &assertion.type_annotation))
        }
        Expression::TSTypeAssertion(assertion) => {
            Some((&assertion.expression, &assertion.type_annotation))
        }
        _ => None,
    }
}

/// Whether lowering `expression` only reads: a binding, `this`, or a
/// non-optional static member / literal-index element chain over one.
pub(in crate::lowering) fn is_plain_read(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_) | Expression::ThisExpression(_) => true,
        Expression::ParenthesizedExpression(inner) => is_plain_read(&inner.expression),
        Expression::StaticMemberExpression(member) => {
            !member.optional && is_plain_read(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            !member.optional
                && matches!(
                    member.expression,
                    Expression::NumericLiteral(_) | Expression::StringLiteral(_)
                )
                && is_plain_read(&member.object)
        }
        _ => false,
    }
}
