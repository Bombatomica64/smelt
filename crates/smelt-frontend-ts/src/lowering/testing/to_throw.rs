//! The expected-error argument of `expect(fn).toThrow(expected)`.
//!
//! Vitest (like Jest) does not only ask whether the callback threw: the
//! optional argument says WHAT must have been thrown, and a throw that does not
//! match fails the assertion (or, under `.not`, passes it). The rules, from
//! `toThrowMatchers` in `@vitest/expect`, are chosen by the argument's kind:
//!
//! * no argument — any throw matches;
//! * a string — the thrown message CONTAINS it;
//! * a `RegExp` — it TESTS the thrown message;
//! * a class (an `Error` constructor) — the thrown value is an INSTANCE of it.
//!
//! "The thrown message" is the thrown value's `message` when that is a string,
//! and the value converted with `String(..)` otherwise (a thrown string is its
//! own message). That is exactly what a `string`-typed catch binding receives
//! from the error channel (`smelt_thrown_message`, or the recovered panic
//! message), so the message rules bind the catch at `string` and compare
//! directly; only the class rule needs the thrown VALUE (an `unknown` binding).
//! Neither introduces control flow into the handler: a branch there would make
//! the structurizer re-emit the rest of the test body once per arm.
//!
//! This module builds that predicate and the catch local it reads. The
//! statement that runs the callback (`expect_to_throw_statement`) binds the
//! local and records the predicate as its "threw a matching error" flag, so an
//! unexpected error fails the test instead of being accepted as "it threw".

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprId, ExprKind, LocalDecl, LocalId, Span, Type, TypeId};

impl ModuleBuilder<'_> {
    /// Build the catch local and the predicate "the caught value matches
    /// `expected`" (module docs).
    ///
    /// Returns `Ok(None)` for an argument shape this lowering does not model
    /// (an asymmetric matcher, an error INSTANCE); the caller then keeps the
    /// any-throw check, with no catch binding, for it.
    pub(in crate::lowering) fn expected_error_predicate(
        &mut self,
        expected_arg: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<(LocalId, ExprId)>, SmeltError> {
        let span = self.span(expected_arg.span().start, expected_arg.span().end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        // A class NAME is an `instanceof` test on the thrown value. Classes are
        // not first-class values in Smelt, so the name is resolved nominally,
        // exactly as `caught instanceof Name` would be. A local class (or a
        // modeled host class) is known here; an IMPORTED name may be a class or
        // a value (`toThrowError(MESSAGE)`), so it is first tried as a value and
        // only a value that is neither a string nor a RegExp counts as a class.
        let class_name = match expected_arg.as_expression() {
            Some(Expression::Identifier(ident)) if !self.scope.is_bound(ident.name.as_str()) => {
                Some(ident.name.as_str())
            }
            _ => None,
        };
        let instance_of = |this: &mut Self, name: &str, target: &mut Body| {
            let unknown_ty = this.ctx.krate.types.intern(Type::Unknown);
            let caught = this.caught_local(unknown_ty, span, target);
            let value = Self::caught_read(caught, unknown_ty, span, target);
            let class = this.intern_type_name(name);
            let test = target.push_expr(Expr {
                kind: ExprKind::InstanceOf { value, class },
                ty: bool_ty,
                span,
            });
            (caught, test)
        };
        if let Some(name) = class_name
            && (self.classes.contains(name)
                || self.classes.is_pending(name)
                || Self::instanceof_builtin_target(name))
        {
            return Ok(Some(instance_of(self, name, body)));
        }
        let imported_class_candidate = class_name.filter(|name| self.imports.is_value(name));
        let expected = match self.argument(expected_arg, body) {
            Ok(expected) => expected,
            Err(_) if imported_class_candidate.is_some() => {
                return Ok(imported_class_candidate.map(|name| instance_of(self, name, body)));
            }
            Err(error) => return Err(error),
        };
        let expected_ty = Self::expr_ty(body, expected);
        let is_regexp = matches!(
            self.ctx.krate.types.get(expected_ty),
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(*name).is_some_and(|name| name == "RegExp")
        );
        let is_string = matches!(self.ctx.krate.types.get(expected_ty), Some(Type::String));
        if !is_regexp && !is_string {
            return Ok(imported_class_candidate.map(|name| instance_of(self, name, body)));
        }
        // The thrown message: a `string` catch binding (module docs).
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let caught = self.caught_local(string_ty, span, body);
        let message = Self::caught_read(caught, string_ty, span, body);
        if is_regexp {
            let test = body.push_expr(Expr {
                kind: ExprKind::RegexTest {
                    regex: expected,
                    haystack: message,
                },
                ty: bool_ty,
                span,
            });
            return Ok(Some((caught, test)));
        }
        let oxc_span = expected_arg.span();
        let contains = self.contains_expr(message, expected, oxc_span, body)?;
        Ok(Some((caught, contains)))
    }

    /// Declare the catch binding the predicate reads, at `ty`.
    fn caught_local(&mut self, ty: TypeId, span: Span, body: &mut Body) -> LocalId {
        let name = self.intern_source_name("error");
        body.push_local(LocalDecl {
            name: Some(name),
            ty,
            mutable: false,
            span,
        })
    }

    /// Read the catch binding.
    fn caught_read(caught: LocalId, ty: TypeId, span: Span, body: &mut Body) -> ExprId {
        body.push_expr(Expr {
            kind: ExprKind::Local(caught),
            ty,
            span,
        })
    }
}
