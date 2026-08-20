//! Variadic lowering for functions that read their own `arguments` object.
//!
//! # Why a function that reads `arguments` must be variadic
//!
//! A JavaScript `arguments` object is the *actual* argument list of the call, not
//! the function's declared parameter list. The two differ constantly:
//!
//! ```js
//! function fn(_a, _b, _c) { return Array.from(arguments); }
//! ary(fn, 2)('a', 'b', 'c', 'd');   // fn('a', 'b')       -> ['a', 'b']
//! rest(fn)(1, 2, 3, 4);             // fn(1, 2, [3, 4])   -> [1, 2, [3, 4]]
//! ```
//!
//! Smelt used to emit such a function at its *declared* arity, and the erased-call
//! boundary padded a short call up to that arity:
//!
//! ```ignore
//! Rc::new(move |smelt_args: Vec<SmeltUnknown>| callback(
//!     smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null),
//!     smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Null),
//!     smelt_args.get(2).cloned().unwrap_or(SmeltUnknown::Null),
//! ))
//! ```
//!
//! The count is gone by the time the body runs, and the padding is
//! indistinguishable from a real trailing `undefined` — which some specs
//! deliberately pass (`partial`'s placeholder specs expect
//! `[undefined, 'b', undefined]`). So `arguments` was built from the parameters
//! and answered `['a', 'b', null]` where JavaScript answers `['a', 'b']`.
//!
//! # The lowering
//!
//! A non-arrow function whose body reads `arguments` is genuinely variadic, so it
//! is lowered that way: its parameter list becomes a **single rest parameter**
//! holding the whole argument list, and each declared name is re-bound from that
//! list at function entry.
//!
//! ```ignore
//! // source                          lowered as
//! function fn(a, b, c) {             function fn(...__smelt_arguments) {
//!   … arguments …                      const a = __smelt_arguments[0];
//! }                                    const b = __smelt_arguments[1];
//!                                      const c = __smelt_arguments[2];
//!                                      … __smelt_arguments …
//!                                    }
//! ```
//!
//! This reuses the rest-parameter path end to end rather than inventing a side
//! channel: callers already pack their arguments into the rest list, the
//! erased-call boundary already forwards its whole `Vec<SmeltUnknown>` into it,
//! and `arguments` is then that list — exact count, no padding. A thread-local
//! "current call arguments" slot was the alternative and was rejected: nothing
//! identifies which callee a pushed frame belongs to, so a function reading
//! `arguments` through a *direct* call nested inside an erased call would read the
//! outer frame.
//!
//! Functions that never mention `arguments` are untouched and keep their declared
//! arity, so this costs nothing for the overwhelming majority of code.

use oxc::ast::ast::BindingPattern;

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, Expr, ExprKind, LocalDecl, LocalId, Literal, Param, Pattern, Stmt, Type};

/// The synthetic rest-parameter name holding a variadic function's argument list.
///
/// Spelled with the `__smelt_` prefix so it cannot collide with a source
/// identifier; a source parameter named `arguments` is rejected by TypeScript in
/// strict mode, and any other source name is free to shadow nothing here.
pub(in crate::lowering) const ARGUMENTS_LIST_NAME: &str = "__smelt_arguments";

/// Detects whether a function body reads its own `arguments` object.
///
/// Descends into arrow functions, which have no `arguments` of their own and
/// therefore read the enclosing function's. Stops at every non-arrow `function`
/// boundary, because such a function introduces its own `arguments` and its
/// reads belong to it, not to the function being scanned.
struct ArgumentsReferenceScanner {
    /// Set once a bare `arguments` reference is seen in the scanned scope.
    found: bool,
}

impl<'a> oxc::ast_visit::Visit<'a> for ArgumentsReferenceScanner {
    fn visit_identifier_reference(&mut self, identifier: &oxc::ast::ast::IdentifierReference<'a>) {
        if identifier.name == "arguments" {
            self.found = true;
        }
    }

    fn visit_function(
        &mut self,
        _function: &oxc::ast::ast::Function<'a>,
        _flags: oxc::semantic::ScopeFlags,
    ) {
        // A nested non-arrow function owns its own `arguments`; do not descend.
    }
}

/// The rewritten parameter list of a variadic `arguments`-reading function.
///
/// The caller owns scope bookkeeping (each lowering site saves and restores
/// bindings its own way), so the bound names are handed back rather than left
/// bound here.
pub(in crate::lowering) struct ArgumentsForwarding {
    /// The single synthesized rest parameter, ready to append to a signature.
    pub(in crate::lowering) params: Vec<Param>,
    /// The parameter types, in the same order as `params`.
    pub(in crate::lowering) param_tys: Vec<smelt_hir::TypeId>,
    /// Index of the rest parameter within `params` (always `0`).
    pub(in crate::lowering) rest_index: usize,
    /// Element type of the rest list, for a `RestParam` record.
    pub(in crate::lowering) item_ty: smelt_hir::TypeId,
    /// The rest parameter's local, which is what `arguments` reads.
    pub(in crate::lowering) list_local: LocalId,
    /// Source parameter names re-bound from the list, in declaration order.
    pub(in crate::lowering) bound: Vec<(String, LocalId)>,
}

impl ModuleBuilder<'_> {
    /// Return whether a non-arrow function's body reads its own `arguments`.
    pub(in crate::lowering) fn function_reads_arguments(
        function: &oxc::ast::ast::Function<'_>,
    ) -> bool {
        use oxc::ast_visit::Visit;
        let Some(body) = &function.body else {
            return false;
        };
        let mut scanner = ArgumentsReferenceScanner { found: false };
        for statement in &body.statements {
            scanner.visit_statement(statement);
        }
        scanner.found
    }

    /// Rewrite an `arguments`-reading function's parameters into one rest list.
    ///
    /// Pushes the rest local and the per-name `let` bindings into `body` (so the
    /// bindings precede every source statement the caller lowers next) and returns
    /// the signature pieces plus the names to bind in scope.
    ///
    /// Returns `None` — leaving the caller on its ordinary parameter path — when
    /// the function already declares a rest parameter (its `arguments` is then
    /// already exact, since the rest list carries the real tail) or when a
    /// parameter is a destructuring pattern rather than a plain identifier, which
    /// this rewrite cannot re-bind positionally.
    pub(in crate::lowering) fn arguments_forwarding_params(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        body: &mut Body,
    ) -> Result<Option<ArgumentsForwarding>, SmeltError> {
        if function.params.rest.is_some() {
            return Ok(None);
        }
        if !Self::function_reads_arguments(function) {
            return Ok(None);
        }
        if !function
            .params
            .items
            .iter()
            .all(|param| matches!(&param.pattern, BindingPattern::BindingIdentifier(_)))
        {
            return Ok(None);
        }
        let span = self.span(function.span.start, function.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let list_symbol = self.intern_source_name(ARGUMENTS_LIST_NAME);
        let list_local = body.push_local(LocalDecl {
            name: Some(list_symbol),
            ty: list_ty,
            mutable: false,
            span,
        });
        body.params.push(list_local);
        let params = vec![Param {
            name: list_symbol,
            local: list_local,
            ty: list_ty,
            span,
        }];
        let mut bound = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                continue;
            };
            // The declared annotation still types the body's use of the name. The
            // list element is `unknown`, so a concrete annotation needs the same
            // extraction any other erased-to-concrete read gets.
            let declared_ty = match &param.type_annotation {
                Some(annotation) => self.ts_type_to_hir(&annotation.type_annotation)?,
                None => unknown_ty,
            };
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(list_local),
                ty: list_ty,
                span,
            });
            let index_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(index as f64)),
                ty: float_ty,
                span,
            });
            let mut value = body.push_expr(Expr {
                kind: ExprKind::Index {
                    receiver,
                    index: index_expr,
                },
                ty: unknown_ty,
                span,
            });
            if declared_ty != unknown_ty {
                value = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value },
                    ty: declared_ty,
                    span,
                });
            }
            let symbol = self.intern_source_name(binding.name.as_str());
            let local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty: declared_ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt(Stmt::Let {
                pat,
                ty: declared_ty,
                value: Some(value),
            });
            bound.push((binding.name.as_str().to_owned(), local));
        }
        Ok(Some(ArgumentsForwarding {
            params,
            param_tys: vec![list_ty],
            rest_index: 0,
            item_ty: unknown_ty,
            list_local,
            bound,
        }))
    }

}

impl ArgumentsForwarding {
    /// Every source name this rewrite introduces, paired with its local.
    ///
    /// The synthetic rest name is included so the scope frame is self-describing;
    /// each lowering site binds and restores these its own way (the sites differ in
    /// how they snapshot scope), so the pairs are handed back rather than bound
    /// here.
    pub(in crate::lowering) fn binding_pairs(&self) -> Vec<(String, LocalId)> {
        let mut pairs = vec![(ARGUMENTS_LIST_NAME.to_owned(), self.list_local)];
        pairs.extend(self.bound.iter().cloned());
        pairs
    }
}
