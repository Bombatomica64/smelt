//! Static functions of the `Promise`, `Array`, `Reflect`, `Object` and
//! `Symbol` namespaces read as values (`const resolve = Promise.resolve`,
//! `items.map(Array.of)`, `const has = Reflect.has`).
//!
//! JavaScript hands out the builtin function object itself, so a call through
//! the copy answers what the direct call answers. Each value here is a
//! synthesized closure whose body is the operation the direct call lowers to,
//! over the closure's parameters. The builtin is generic (`Promise.resolve<T>`,
//! `Array.from<T>`, `Reflect.has(target: object, ..)`) and a closure value has
//! one Rust signature for every call through it, so each parameter is the
//! tagged value at that runtime-narrowing boundary, as for the `JSON` values
//! (`json_static_function_member`): no static instantiation of the type
//! parameter exists at the read. The result is what the direct call answers
//! for an erased argument.
//!
//! Behavioral requirement: a call through the value answers what the direct
//! call with the same arguments answers, and the value reports Node's `name`
//! and `length` (`name_builtin_static_function_value`, applied by the caller).
//! Type preservation: the closure's parameter and result types are the ones
//! the direct call gives its arguments and result at an erased argument.

use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use smelt_hir::{
    AsyncOp, Body, ErasureReason, Expr, ExprKind, FunctionType, Literal, LocalDecl, Param,
    PropertyLookup, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// The value of `Promise.resolve`/`reject`/`all`, `Array.from`/`of`,
    /// `Reflect.has` or `Object.freeze` read as a function (see the module
    /// docs), or `None` for any other member or a shadowed namespace.
    pub(in crate::lowering) fn builtin_static_function_value(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        let owner = ["Promise", "Array", "Reflect", "Object", "Symbol"]
            .into_iter()
            .find(|owner| self.names_host(object.name.as_str(), owner))?;
        let span = self.span(member.span.start, member.span.end);
        let erased = |this: &mut Self, detail: &str| {
            this.erase_at(ErasureReason::RuntimeNarrowing, span, detail)
        };
        match (owner, member.property.name.as_str()) {
            // `Promise.resolve(value?)`: a future settled with the value
            // after the microtask deferral the direct call's `Resolve` keeps.
            ("Promise", "resolve") => {
                let value_ty = erased(self, "Promise.resolve value");
                let future_ty = self.ctx.krate.types.intern(Type::Future(value_ty));
                Some(self.builtin_value_closure(
                    [("value", value_ty)],
                    (None, 0),
                    future_ty,
                    span,
                    outer_body,
                    |this, body, [first]| {
                        this.require_promise_receiver("PromiseResolve", span, body);
                        let duration = this.zero_duration(span, body);
                        body.push_expr(Expr {
                            kind: ExprKind::AsyncOp {
                                op: AsyncOp::Resolve,
                                args: vec![duration, first],
                            },
                            ty: future_ty,
                            span,
                        })
                    },
                ))
            }
            // `Promise.reject(reason?)`: a future rejected with the reason.
            ("Promise", "reject") => {
                let reason_ty = erased(self, "Promise.reject reason");
                let future_ty = self.ctx.krate.types.intern(Type::Future(reason_ty));
                Some(self.builtin_value_closure(
                    [("reason", reason_ty)],
                    (None, 0),
                    future_ty,
                    span,
                    outer_body,
                    |this, body, [first]| {
                        this.require_promise_receiver("PromiseReject", span, body);
                        let duration = this.zero_duration(span, body);
                        body.push_expr(Expr {
                            kind: ExprKind::AsyncOp {
                                op: AsyncOp::Reject,
                                args: vec![duration, first],
                            },
                            ty: future_ty,
                            span,
                        })
                    },
                ))
            }
            // `Promise.all(values)`: the values read as a list of futures (a
            // plain value is an already-settled one), awaited in order.
            ("Promise", "all") => {
                let values_ty = erased(self, "Promise.all values");
                let item_ty = erased(self, "Promise.all value");
                let future_item_ty = self.ctx.krate.types.intern(Type::Future(item_ty));
                let pending_list_ty = self.ctx.krate.types.intern(Type::List(future_item_ty));
                let results_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                let future_ty = self.ctx.krate.types.intern(Type::Future(results_ty));
                Some(self.builtin_value_closure(
                    [("values", values_ty)],
                    (None, 1),
                    future_ty,
                    span,
                    outer_body,
                    |this, body, [first]| {
                        this.require_promise_receiver("Promise.all", span, body);
                        let futures = body.push_expr(Expr {
                            kind: ExprKind::UnknownCast {
                                value: first,
                                target: pending_list_ty,
                            },
                            ty: pending_list_ty,
                            span,
                        });
                        body.push_expr(Expr {
                            kind: ExprKind::AsyncOp {
                                op: AsyncOp::All,
                                args: vec![futures],
                            },
                            ty: future_ty,
                            span,
                        })
                    },
                ))
            }
            // `Array.from(items)`: the items' elements copied into a new array,
            // read through their iterator as the direct call reads them.
            ("Array", "from") => {
                let items_ty = erased(self, "Array.from items");
                let item_ty = erased(self, "Array.from item");
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                Some(self.builtin_value_closure(
                    [("items", items_ty)],
                    (None, 1),
                    list_ty,
                    span,
                    outer_body,
                    |this, body, [first]| {
                        let source = this.iterated_value(first, span, body).unwrap_or(first);
                        let source_ty = Self::expr_ty(body, source);
                        this.array_from_elements(source, source_ty, false, span, body)
                    },
                ))
            }
            // `Array.of(...items)`: the arguments themselves, as a new array.
            ("Array", "of") => {
                let item_ty = erased(self, "Array.of item");
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                Some(self.builtin_value_closure(
                    [("items", list_ty)],
                    (Some(0), 0),
                    list_ty,
                    span,
                    outer_body,
                    |_, _, [first]| first,
                ))
            }
            // `Reflect.has(target, key)`: the `in` operator, `key in target`,
            // along the prototype chain.
            ("Reflect", "has") => {
                let target_ty = erased(self, "Reflect.has target");
                let key_ty = erased(self, "Reflect.has key");
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                Some(self.builtin_value_closure(
                    [("target", target_ty), ("propertyKey", key_ty)],
                    (None, 2),
                    bool_ty,
                    span,
                    outer_body,
                    |_, body, [target, key]| {
                        body.push_expr(Expr {
                            kind: ExprKind::DictContainsKey {
                                dict: target,
                                key,
                                lookup: PropertyLookup::PrototypeChain,
                            },
                            ty: bool_ty,
                            span,
                        })
                    },
                ))
            }
            // `Object.freeze(o)` answers its argument, as the direct call
            // does (the frozen flag is not modelled there either).
            ("Object", "freeze") => {
                let value_ty = erased(self, "Object.freeze value");
                Some(self.builtin_value_closure(
                    [("o", value_ty)],
                    (None, 1),
                    value_ty,
                    span,
                    outer_body,
                    |_, _, [first]| first,
                ))
            }
            // `Symbol.for(key)`: the registry symbol for the key, as the
            // direct call with a run-time key answers it. Both parameter and
            // result are typed: the builtin is not generic.
            ("Symbol", "for") => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let symbol_ty = self.ctx.krate.types.intern(Type::Symbol);
                Some(self.builtin_value_closure(
                    [("key", string_ty)],
                    (None, 1),
                    symbol_ty,
                    span,
                    outer_body,
                    |_, body, [key]| {
                        body.push_expr(Expr {
                            kind: ExprKind::SymbolFor { key },
                            ty: symbol_ty,
                            span,
                        })
                    },
                ))
            }
            // `Symbol.keyFor(sym)`: the registry key, or `undefined`.
            ("Symbol", "keyFor") => {
                let symbol_ty = self.ctx.krate.types.intern(Type::Symbol);
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let result_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Optional(string_ty, smelt_hir::Absence::Undefined));
                Some(self.builtin_value_closure(
                    [("sym", symbol_ty)],
                    (None, 1),
                    result_ty,
                    span,
                    outer_body,
                    |this, body, [sym]| this.symbol_key_for_expr(sym, span, body),
                ))
            }
            _ => None,
        }
    }

    /// Check the receiver at invocation time. All receiver-binding forms
    /// invoke this same closure, including a detached call with no receiver.
    fn require_promise_receiver(&mut self, builtin: &str, span: Span, body: &mut Body) {
        let none = self.ctx.krate.types.intern(Type::None);
        let check = body.push_expr(Expr {
            kind: ExprKind::RequirePromiseReceiver {
                builtin: builtin.to_owned(),
            },
            ty: none,
            span,
        });
        body.push_stmt(Stmt::Expr(check));
    }

    /// The `0` delay operand the `Resolve`/`Reject` async ops carry for their
    /// microtask deferral.
    fn zero_duration(&mut self, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: float_ty,
            span,
        })
    }

    /// Synthesize a closure value over `params` (named and typed) whose body
    /// returns what `make_body` builds from the parameter reads.
    ///
    /// `(rest, required)`: the index of a rest parameter, and how many leading
    /// parameters a call must supply; the trailing ones read `undefined` when
    /// omitted, as a JavaScript builtin's optional parameters do.
    fn builtin_value_closure<const N: usize>(
        &mut self,
        params: [(&str, smelt_hir::TypeId); N],
        (rest, required): (Option<usize>, usize),
        return_ty: smelt_hir::TypeId,
        span: Span,
        outer_body: &mut Body,
        make_body: impl FnOnce(&mut Self, &mut Body, [smelt_hir::ExprId; N]) -> smelt_hir::ExprId,
    ) -> smelt_hir::ExprId {
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::with_capacity(N);
        let reads = params.map(|(source_name, ty)| {
            let name = self.intern_source_name(source_name);
            let local = closure_body.push_local(LocalDecl {
                name: Some(name),
                ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name,
                local,
                ty,
                span,
            });
            closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            })
        });
        let result = make_body(self, &mut closure_body, reads);
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body = self.ctx.krate.push_body(closure_body);
        let required_params = (required < N).then_some(required);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.iter().map(|&(_, ty)| ty).collect(),
            rest,
            required_params,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest,
                required_params,
                return_ty,
                captures: Vec::new(),
                body,
                name: None,
                length: None,
                function_item: None,
                this_binding: smelt_hir::ClosureThis::Own,
                span,
            }),
            ty: closure_ty,
            span,
        })
    }
}
