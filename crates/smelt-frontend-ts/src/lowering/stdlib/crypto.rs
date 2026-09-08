//! `WebCrypto` lowering for the TypeScript frontend.
//!
//! The three keyless members of the `crypto` global, each lowered to a concrete
//! type rather than to the erased record `crypto` used to be:
//!
//! | source | HIR type |
//! | --- | --- |
//! | `crypto.randomUUID()` | `String` |
//! | `crypto.getRandomValues(view)` | `Uint8Array` (the argument, filled) |
//! | `crypto.subtle.digest(name, data)` | `Promise<Uint8Array>` |
//!
//! # Two spellings, one rule
//!
//! `crypto` is an ambient global AND a `node:crypto` export, so the same call
//! arrives either dotted (`crypto.randomUUID()`) or bare after an import
//! (`randomUUID()`). Both spellings are entries in the shared recognition
//! table (`smelt_stdlib::recognition`) selecting the SAME rule, and the arity
//! and result type are decided here from the rule — so neither spelling has a
//! lowering of its own to drift.
//!
//! A program with its own `randomUUID` or its own `crypto` is unaffected: every
//! handler below declines a name the module binds itself, exactly as
//! `createServer` does.
//!
//! # The algorithm identifier
//!
//! `digest`'s first argument is the spec's `AlgorithmIdentifier`: a string, or
//! an object whose `name` is one. Both reach the same lowering because the
//! *object* form is reduced to its `name` field HERE, at the one place that
//! knows the argument's lowered type — so the generated Rust takes a plain
//! `&str` and the emitted helper never has to inspect a record.
//!
//! # What is not modeled
//!
//! `subtle.importKey`/`sign`/`verify`. See `CRYPTO_KEY_REASON` in
//! `smelt_stdlib::host_modules`: those need a `CryptoKey` value and its key
//! material, which is a surface rather than three calls, and they stay declared
//! so using one is a named blocker.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, CryptoOp, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Lower a recognized `WebCrypto` call.
    ///
    /// Registered in the exact-call dispatch chain, so the receiver spelling has
    /// already been matched against the recognition table by
    /// `stdlib_dispatch::call_rule` and only the arguments are left to lower.
    pub(in crate::lowering) fn crypto_call(
        &mut self,
        rule: RuleId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.crypto_name_is_shadowed(call) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        match rule {
            RuleId::TsCryptoRandomUuid => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::CryptoOp {
                        op: CryptoOp::RandomUuid,
                        args: Vec::new(),
                    },
                    ty,
                    span,
                })))
            }
            RuleId::TsCryptoGetRandomValues => {
                let [view] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "`crypto.getRandomValues` takes exactly one argument, the view to fill",
                    ));
                };
                let view = self.argument(view, body)?;
                // The result IS the argument, so it is the argument's type: the
                // spec fills in place and answers the same object, and typing
                // the call as a fresh view would lose that.
                let ty = Self::expr_ty(body, view);
                // A typed-array VIEW is still the byte-backed host record by
                // design (see `StdlibClass::ByteArray`), and a program that
                // allocates the view it wants filled — `new Uint8Array(n)` — is
                // holding exactly that. Both shapes are accepted, and codegen
                // fills whichever it was handed; refusing the erased one would
                // refuse the API's normal spelling. A value that is neither is
                // refused rather than silently ignored, because the previous
                // lowering's answer to everything was to hand the argument back
                // UNFILLED.
                if !self.is_byte_array_type(ty)
                    && !matches!(
                        self.ctx.krate.types.get(ty),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                    )
                {
                    return Err(SmeltError::unsupported(
                        span,
                        "`crypto.getRandomValues` fills a typed-array view; this argument is neither a byte view nor a value that can hold one",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::CryptoOp {
                        op: CryptoOp::GetRandomValues,
                        args: Vec::from([view]),
                    },
                    ty,
                    span,
                })))
            }
            RuleId::TsCryptoDigest => {
                let [algorithm, data] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "`crypto.subtle.digest` takes exactly two arguments, the algorithm and the data to hash",
                    ));
                };
                let algorithm = self.argument(algorithm, body)?;
                let algorithm = self.crypto_algorithm_name(algorithm, span, body)?;
                let data = self.argument(data, body)?;
                if !self.is_byte_array_type(Self::expr_ty(body, data)) {
                    return Err(SmeltError::unsupported(
                        span,
                        "`crypto.subtle.digest` hashes a concrete byte view (the value `TextEncoder.encode` and the body readers answer); the erased typed-array views are not modeled as digest input yet",
                    ));
                }
                let bytes_ty = self.byte_array_type();
                let ty = self.ctx.krate.types.intern(Type::Future(bytes_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::CryptoOp {
                        op: CryptoOp::Digest,
                        args: Vec::from([algorithm, data]),
                    },
                    ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Reduce a spec `AlgorithmIdentifier` argument to its name string.
    ///
    /// The spec accepts `"SHA-256"` and `{ name: "SHA-256" }` interchangeably,
    /// and normalizing here is what lets the emitted helper take a `&str`. A
    /// string argument passes through; a record argument becomes a field read
    /// of `name`, which is an ordinary lowering rather than a new node.
    ///
    /// Anything else is refused rather than erased: an algorithm the compiler
    /// cannot see the *name* of would have to be inspected at run time through
    /// the dynamic boundary, and a value that reaches a hash function is
    /// exactly where a silent erasure is worst.
    fn crypto_algorithm_name(
        &mut self,
        algorithm: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let ty = Self::expr_ty(body, algorithm);
        if matches!(self.ctx.krate.types.get(ty), Some(Type::String)) {
            return Ok(algorithm);
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let name = self.intern_source_name("name");
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver: algorithm,
                field: name,
            },
            ty: string_ty,
            span,
        }))
    }

    /// Return whether the module binds the name this call would recognize.
    ///
    /// A source `function randomUUID()`, a `const crypto = ..`, or an import of
    /// either name from a module Smelt lowers is the program's own value, and
    /// the recognized global must give way to it. The name checked is the ROOT
    /// of the callee: for `crypto.subtle.digest(..)` that is `crypto`, so
    /// shadowing the namespace takes its whole surface with it.
    fn crypto_name_is_shadowed(&self, call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        let Some(root) = Self::callee_root_name(&call.callee) else {
            return false;
        };
        self.scope.is_bound(root) || self.classes.contains(root) || self.import_alias_resolved(root)
    }

    /// Return the root identifier of a call's callee, when it has one.
    ///
    /// `randomUUID` for a bare call, `crypto` for `crypto.randomUUID` and for
    /// `crypto.subtle.digest` alike.
    fn callee_root_name<'source>(
        callee: &'source oxc::ast::ast::Expression<'_>,
    ) -> Option<&'source str> {
        match callee {
            oxc::ast::ast::Expression::Identifier(identifier) => Some(identifier.name.as_str()),
            oxc::ast::ast::Expression::StaticMemberExpression(member) => {
                Self::callee_root_name(&member.object)
            }
            _ => None,
        }
    }
}
