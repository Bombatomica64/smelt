//! The JavaScript iterator protocol over hand-written iterators.
//!
//! JavaScript iterates an object (`for...of`, spread, `Array.from`, array
//! destructuring, `new Set`/`new Map`) by calling its `[Symbol.iterator]()`
//! and driving the iterator that answers: `next()` until a step is `done`,
//! and `return()` when the consumer leaves early (`IteratorClose`).
//!
//! Smelt's typed iterator is the generator carrier: `Iterator<T, R, N>` and
//! its lib relatives lower to [`Type::Generator`] (`SmeltGenerator<T, R, N>`)
//! and a step `IteratorResult<T, R>` to [`Type::GeneratorResult`]
//! (`SmeltGeneratorResult<T, R>`), see `lowering::ty::annotations`. Every
//! consumer already drives that carrier. This module makes a hand-written
//! iterator one:
//!
//! * an iterator object literal `{ next, return }` in an `Iterator<T>` slot,
//!   and an instance of a class with `next(): IteratorResult<T>` (in such a
//!   slot, or answered by `[Symbol.iterator]()`), becomes a generator through
//!   [`ModuleBuilder::protocol_generator`]: a synthesized generator closure
//!   that calls `next()`, yields each step's value, and calls `return()` from
//!   a `finally` around the `yield` when the consumer closes it early;
//! * a step literal `{ value, done }` in an `IteratorResult<T, R>` slot is the
//!   step itself ([`ModuleBuilder::iterator_result_literal`]).
//!
//! Requirements (see `docs/design/iterator-protocol.md`): the element type `T`
//! is the one the source declares, concrete or generic, never erased; `next()`
//! runs exactly when JavaScript runs it (once per step, lazily, so an infinite
//! iterator is fine); `return()` runs exactly when the consumer leaves before
//! the iterator is done (`break`, `return`, a throw out of the loop body, a
//! destructuring that took what it binds) and never after a `done` step or a
//! `next()` that threw.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, PropertyKind};
use oxc::ast_visit::{Visit, walk};
use smelt_hir::{
    Body, ClosureExpr, ClosureThis, Expr, ExprId, ExprKind, FunctionType, Literal, LocalDecl,
    LocalId, Param, Pattern, Span, Stmt, Type, TypeId,
};

/// How the adapter generator reaches a hand-written iterator's `next` and
/// `return`.
#[derive(Clone, Copy, Debug)]
enum ProtocolAccess {
    /// The methods of a class instance (the adapter's one parameter).
    Method {
        /// The instance's type.
        instance_ty: TypeId,
        /// `next()`'s step type.
        next_ty: TypeId,
        /// `return()`'s result type, when the class declares one.
        return_ty: Option<TypeId>,
    },
    /// The function-valued members of a record or interface value (the
    /// adapter's one parameter), called as the closures they are.
    Member {
        /// The record's type.
        instance_ty: TypeId,
        /// `next`'s function type.
        next_fn: TypeId,
        /// `return`'s function type, when the record declares one.
        return_fn: Option<TypeId>,
    },
    /// A generator of another type than the slot's (`Generator<T>`, whose
    /// resume channel is erased, in an `Iterator<T>` slot): resumed with
    /// `next` and closed with `return` (the adapter's one parameter).
    Generator {
        /// The generator's type.
        generator_ty: TypeId,
        /// Its step type.
        step_ty: TypeId,
    },
    /// An iterator object literal's `next` and `return` closures (the
    /// adapter's parameters, in that order).
    Closures {
        /// `next`'s function type.
        next_fn: TypeId,
        /// `return`'s function type, when the literal spells one.
        return_fn: Option<TypeId>,
    },
}

impl ProtocolAccess {
    /// Whether the iterator has a `return()` for the adapter to close it with.
    const fn closes(self) -> bool {
        match self {
            Self::Method { return_ty, .. } => return_ty.is_some(),
            Self::Generator { .. } => true,
            Self::Member { return_fn, .. } | Self::Closures { return_fn, .. } => {
                return_fn.is_some()
            }
        }
    }
}

/// Which protocol method the adapter calls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProtocolCall {
    /// `next()`.
    Next,
    /// `return()`.
    Return,
}

/// Finds a `this` that belongs to the function being visited: one outside any
/// nested non-arrow function, whose own `this` it would be.
#[derive(Default)]
struct OwnThisFinder {
    /// Whether such a `this` was seen.
    found: bool,
}

impl<'a> Visit<'a> for OwnThisFinder {
    fn visit_this_expression(&mut self, _it: &oxc::ast::ast::ThisExpression) {
        self.found = true;
    }

    fn visit_function(
        &mut self,
        _it: &oxc::ast::ast::Function<'a>,
        _flags: oxc::syntax::scope::ScopeFlags,
    ) {
        // A nested `function` has its own `this`.
    }

    fn visit_class(&mut self, _it: &oxc::ast::ast::Class<'a>) {
        // So do a nested class's members.
    }
}

impl ModuleBuilder<'_> {
    /// Whether `ty` is a class or interface the program declares (its item, or
    /// the signatures the class prepass collected for one still lowering), the
    /// only types whose members a protocol probe asks about. A host class (a
    /// regex match, a `Date`) declares no iterator members, and asking would
    /// intern the erased type of the miss.
    fn declares_own_members(&self, ty: TypeId) -> bool {
        let Some(&Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        self.class_by_symbol(name).is_some()
            || self.find_interface(name).is_some()
            || crate::lowering::support::source_spelling(&self.ctx.krate, name).is_some_and(
                |text| self.classes.methods(text).is_some() || self.classes.fields(text).is_some(),
            )
    }

    /// The protocol `value` of type `ty` exposes as a hand-written iterator:
    /// a class instance whose `next()` takes no argument and answers a typed
    /// step (`IteratorResult<T, R>`), or a record/interface value whose `next`
    /// member is such a function. `return` is used when present, so the
    /// consumer can close the iterator. `None` for any other type, a
    /// generator included (it is the carrier already).
    fn iterator_protocol_access(&mut self, ty: TypeId) -> Option<ProtocolAccess> {
        if let Some(Type::Generator {
            is_async: false,
            yield_ty,
            return_ty,
            ..
        }) = self.ctx.krate.types.get(ty).cloned()
        {
            let step_ty = self.ctx.krate.types.intern(Type::GeneratorResult {
                yield_ty,
                return_ty,
            });
            return Some(ProtocolAccess::Generator {
                generator_ty: ty,
                step_ty,
            });
        }
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty).cloned() else {
            return None;
        };
        if !self.declares_own_members(ty) {
            return None;
        }
        let next = self.intern_source_name("next");
        let close = self.intern_source_name("return");
        // A miss records an erasure for the absent member; the question is a
        // probe, so nothing it recorded stays.
        let records_before = self.ctx.krate.erasures.len();
        let member_fn = |this: &mut Self, member| match this.class_field_type(ty, member) {
            Ok(member_ty) => match this.ctx.krate.types.get(member_ty) {
                Some(Type::Function(function))
                    if function.params.is_empty() || function.required_params == Some(0) =>
                {
                    Some((member_ty, function.return_ty))
                }
                _ => None,
            },
            Err(_) => None,
        };
        let next_member = member_fn(self, next);
        let return_member = member_fn(self, close);
        self.ctx.krate.erasures.truncate(records_before);
        let (next_fn, next_ty) = next_member?;
        if !matches!(
            self.ctx.krate.types.get(next_ty),
            Some(Type::GeneratorResult { .. })
        ) {
            return None;
        }
        if self.find_interface(name).is_some() {
            Some(ProtocolAccess::Member {
                instance_ty: ty,
                next_fn,
                return_fn: return_member.map(|(function, _)| function),
            })
        } else {
            Some(ProtocolAccess::Method {
                instance_ty: ty,
                next_ty,
                return_ty: return_member.map(|(_, returned)| returned),
            })
        }
    }

    /// The step type `access`'s `next()` answers.
    fn protocol_step_type(&self, access: ProtocolAccess) -> Option<TypeId> {
        let step = match access {
            ProtocolAccess::Method { next_ty, .. } => next_ty,
            ProtocolAccess::Generator { step_ty, .. } => step_ty,
            ProtocolAccess::Member { next_fn, .. } | ProtocolAccess::Closures { next_fn, .. } => {
                match self.ctx.krate.types.get(next_fn) {
                    Some(Type::Function(function)) => function.return_ty,
                    _ => return None,
                }
            }
        };
        matches!(
            self.ctx.krate.types.get(step),
            Some(Type::GeneratorResult { .. })
        )
        .then_some(step)
    }

    /// The generator type a hand-written iterator with step type `step` is
    /// driven as when no slot states one: its own `T` and `R`, and a resume
    /// channel of `undefined` (the adapter calls `next()` with no argument,
    /// as `for...of` does).
    fn protocol_generator_type(&mut self, step: TypeId) -> Option<TypeId> {
        let Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) = self.ctx.krate.types.get(step).cloned()
        else {
            return None;
        };
        let next_ty = self.ctx.krate.types.intern(Type::None);
        Some(self.ctx.krate.types.intern(Type::Generator {
            is_async: false,
            yield_ty,
            return_ty,
            next_ty,
        }))
    }

    /// One call of `access`'s `next()` or `return()` inside the adapter body
    /// `adapter`, whose parameters are `params`.
    fn protocol_call(
        &mut self,
        access: ProtocolAccess,
        call: ProtocolCall,
        params: &[LocalId],
        span: Span,
        adapter: &mut Body,
    ) -> Option<ExprId> {
        let member = match call {
            ProtocolCall::Next => self.intern_source_name("next"),
            ProtocolCall::Return => self.intern_source_name("return"),
        };
        let function_ty =
            |types: &smelt_hir::TypeInterner, function: TypeId| match types.get(function) {
                Some(Type::Function(function)) => Some(function.return_ty),
                _ => None,
            };
        match access {
            ProtocolAccess::Generator {
                generator_ty,
                step_ty,
            } => {
                let generator = adapter.push_expr(Expr {
                    kind: ExprKind::Local(*params.first()?),
                    ty: generator_ty,
                    span,
                });
                let kind = match call {
                    ProtocolCall::Next => smelt_hir::GeneratorResumeKind::Next,
                    ProtocolCall::Return => smelt_hir::GeneratorResumeKind::Return,
                };
                Some(adapter.push_expr(Expr {
                    kind: ExprKind::GeneratorNext {
                        generator,
                        value: None,
                        kind,
                    },
                    ty: step_ty,
                    span,
                }))
            }
            ProtocolAccess::Method {
                instance_ty,
                next_ty,
                return_ty,
            } => {
                let returned = match call {
                    ProtocolCall::Next => next_ty,
                    ProtocolCall::Return => return_ty?,
                };
                let receiver = adapter.push_expr(Expr {
                    kind: ExprKind::Local(*params.first()?),
                    ty: instance_ty,
                    span,
                });
                Some(adapter.push_expr(Expr {
                    kind: ExprKind::Method {
                        receiver,
                        method: member,
                        args: Vec::new(),
                    },
                    ty: returned,
                    span,
                }))
            }
            ProtocolAccess::Member {
                instance_ty,
                next_fn,
                return_fn,
            } => {
                let function = match call {
                    ProtocolCall::Next => next_fn,
                    ProtocolCall::Return => return_fn?,
                };
                let returned = function_ty(&self.ctx.krate.types, function)?;
                let receiver = adapter.push_expr(Expr {
                    kind: ExprKind::Local(*params.first()?),
                    ty: instance_ty,
                    span,
                });
                let callee = adapter.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: member,
                    },
                    ty: function,
                    span,
                });
                Some(adapter.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: Vec::new(),
                    },
                    ty: returned,
                    span,
                }))
            }
            ProtocolAccess::Closures { next_fn, return_fn } => {
                let (function, local) = match call {
                    ProtocolCall::Next => (next_fn, *params.first()?),
                    ProtocolCall::Return => (return_fn?, *params.get(1)?),
                };
                let returned = function_ty(&self.ctx.krate.types, function)?;
                let callee = adapter.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: function,
                    span,
                });
                Some(adapter.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: Vec::new(),
                    },
                    ty: returned,
                    span,
                }))
            }
        }
    }

    /// The generator of type `generator_ty` that drives the hand-written
    /// iterator `access` reaches through `args`: an immediately called,
    /// synthesized generator closure equivalent to
    ///
    /// ```text
    /// function* (it) {
    ///   while (true) {
    ///     const step = it.next();
    ///     if (step.done) return;
    ///     let open = true;
    ///     try { yield step.value; open = false; }
    ///     finally { if (open) it.return(); }
    ///   }
    /// }
    /// ```
    ///
    /// The generator is lazy, so `next()` runs once per step the consumer
    /// asks for. A consumer that leaves early resumes it with `Return` at the
    /// `yield` (as it closes any generator), which runs the `finally` and so
    /// the iterator's `return()`; a normal resume clears `open` first, and a
    /// `done` step or a throwing `next()` leave the loop outside the `try`, so
    /// neither closes the iterator, as `IteratorClose` specifies. An iterator
    /// without `return()` has no `try`.
    fn protocol_generator(
        &mut self,
        access: ProtocolAccess,
        args: Vec<ExprId>,
        generator_ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        let Some(Type::Generator {
            is_async: false,
            yield_ty,
            return_ty,
            next_ty,
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return None;
        };
        let step_ty = self.protocol_step_type(access)?;
        let Some(Type::GeneratorResult {
            yield_ty: step_yield_ty,
            return_ty: step_return_ty,
        }) = self.ctx.krate.types.get(step_ty).cloned()
        else {
            return None;
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut adapter = Body::new(None, span);
        adapter.set_generator(true);

        // Parameters: the iterator, or its `next` and `return` closures.
        let param_specs: Vec<(&str, TypeId)> = match access {
            ProtocolAccess::Method { instance_ty, .. }
            | ProtocolAccess::Member { instance_ty, .. }
            | ProtocolAccess::Generator {
                generator_ty: instance_ty,
                ..
            } => vec![("__smelt_iterator", instance_ty)],
            ProtocolAccess::Closures { next_fn, return_fn } => {
                let mut specs = vec![("__smelt_iterator_next", next_fn)];
                if let Some(return_fn) = return_fn {
                    specs.push(("__smelt_iterator_return", return_fn));
                }
                specs
            }
        };
        let mut params = Vec::new();
        let mut param_locals = Vec::new();
        for (name, ty) in param_specs {
            let symbol = self.intern_synthesized_name(name);
            let local = adapter.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            adapter.params.push(local);
            param_locals.push(local);
            params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
        }

        // `const step = it.next();`
        let loop_block = adapter.push_block(span);
        let step_symbol = self.intern_synthesized_name("__smelt_iterator_step");
        let step_local = adapter.push_local(LocalDecl {
            name: Some(step_symbol),
            ty: step_ty,
            mutable: false,
            span,
        });
        let step_pat = adapter.push_pattern(Pattern::Binding(step_local));
        let next_call = self.protocol_call(
            access,
            ProtocolCall::Next,
            &param_locals,
            span,
            &mut adapter,
        )?;
        adapter.push_stmt_to_block(
            loop_block,
            Stmt::Let {
                pat: step_pat,
                ty: step_ty,
                value: Some(next_call),
            },
        );

        // `if (step.done) return step.value;` (no value when `R` is undefined).
        let done_step = adapter.push_expr(Expr {
            kind: ExprKind::Local(step_local),
            ty: step_ty,
            span,
        });
        let done = adapter.push_expr(Expr {
            kind: ExprKind::GeneratorDone { result: done_step },
            ty: bool_ty,
            span,
        });
        let completed = adapter.push_block(span);
        let completion = if matches!(self.ctx.krate.types.get(return_ty), Some(Type::None)) {
            None
        } else {
            let returned_step = adapter.push_expr(Expr {
                kind: ExprKind::Local(step_local),
                ty: step_ty,
                span,
            });
            let returned = adapter.push_expr(Expr {
                kind: ExprKind::GeneratorValue {
                    result: returned_step,
                },
                ty: step_return_ty,
                span,
            });
            Some(if step_return_ty == return_ty {
                returned
            } else {
                adapter.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: returned },
                    ty: return_ty,
                    span,
                })
            })
        };
        adapter.push_stmt_to_block(completed, Stmt::Return(completion));
        adapter.push_stmt_to_block(
            loop_block,
            Stmt::If {
                cond: done,
                then_block: completed,
                else_block: None,
            },
        );

        // `yield step.value;`
        let yielded_step = adapter.push_expr(Expr {
            kind: ExprKind::Local(step_local),
            ty: step_ty,
            span,
        });
        let value = adapter.push_expr(Expr {
            kind: ExprKind::GeneratorValue {
                result: yielded_step,
            },
            ty: step_yield_ty,
            span,
        });
        let value = if step_yield_ty == yield_ty {
            value
        } else {
            adapter.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: yield_ty,
                span,
            })
        };
        let suspend = adapter.push_expr(Expr {
            kind: ExprKind::GeneratorYield { value },
            ty: next_ty,
            span,
        });
        if access.closes() {
            // `let open = true; try { yield ..; open = false; }
            //  finally { if (open) it.return(); }`
            let open_symbol = self.intern_synthesized_name("__smelt_iterator_open");
            let open_local = adapter.push_local(LocalDecl {
                name: Some(open_symbol),
                ty: bool_ty,
                mutable: true,
                span,
            });
            let open_pat = adapter.push_pattern(Pattern::Binding(open_local));
            let open_init = adapter.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            });
            adapter.push_stmt_to_block(
                loop_block,
                Stmt::Let {
                    pat: open_pat,
                    ty: bool_ty,
                    value: Some(open_init),
                },
            );
            let protected = adapter.push_block(span);
            adapter.push_stmt_to_block(protected, Stmt::Expr(suspend));
            let open_target = adapter.push_expr(Expr {
                kind: ExprKind::Local(open_local),
                ty: bool_ty,
                span,
            });
            let closed = adapter.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty: bool_ty,
                span,
            });
            adapter.push_stmt_to_block(
                protected,
                Stmt::Assign {
                    target: open_target,
                    value: closed,
                },
            );
            let finally = adapter.push_block(span);
            let closing = adapter.push_block(span);
            let return_call = self.protocol_call(
                access,
                ProtocolCall::Return,
                &param_locals,
                span,
                &mut adapter,
            )?;
            adapter.push_stmt_to_block(closing, Stmt::Expr(return_call));
            let still_open = adapter.push_expr(Expr {
                kind: ExprKind::Local(open_local),
                ty: bool_ty,
                span,
            });
            adapter.push_stmt_to_block(
                finally,
                Stmt::If {
                    cond: still_open,
                    then_block: closing,
                    else_block: None,
                },
            );
            adapter.push_stmt_to_block(
                loop_block,
                Stmt::TryCatch {
                    body: protected,
                    catch_binding: None,
                    catch_body: None,
                    finally_body: Some(finally),
                },
            );
        } else {
            adapter.push_stmt_to_block(loop_block, Stmt::Expr(suspend));
        }
        let forever = adapter.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        adapter.push_stmt(Stmt::While {
            cond: forever,
            body: loop_block,
        });

        let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let body_id = self.ctx.krate.push_body(adapter);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: generator_ty,
            is_async: false,
            // MIR marks a closure whose body can throw (a throwing `next()`).
            may_throw: false,
        }));
        let closure = body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty: generator_ty,
                captures: Vec::new(),
                body: body_id,
                name: None,
                length: None,
                function_item: None,
                this_binding: ClosureThis::Own,
                span,
            }),
            ty: closure_ty,
            span,
        });
        Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: closure,
                args,
            },
            ty: generator_ty,
            span,
        }))
    }

    /// The return type of the zero-argument `[Symbol.iterator]()` method
    /// `value`'s class declares, with the method's name; `None` for any
    /// other value.
    fn symbol_iterator_method(
        &mut self,
        value: ExprId,
        body: &Body,
    ) -> Option<(smelt_hir::Symbol, TypeId)> {
        let value_ty = Self::expr_ty(body, value);
        if !matches!(self.ctx.krate.types.get(value_ty), Some(Type::Class { .. })) {
            return None;
        }
        let iterator = self.intern_symbol_key("__smelt_symbol_iterator");
        if !self.declares_own_members(value_ty) {
            return None;
        }
        // A miss records an erasure for the absent member; the question is a
        // probe, so nothing it recorded stays.
        let records_before = self.ctx.krate.erasures.len();
        let method = match self.class_field_type(value_ty, iterator) {
            Ok(iterator_ty) => match self.ctx.krate.types.get(iterator_ty).cloned() {
                Some(Type::Function(function)) if function.params.is_empty() => {
                    Some(function.return_ty)
                }
                _ => None,
            },
            Err(_) => None,
        };
        self.ctx.krate.erasures.truncate(records_before);
        method.map(|returned| (iterator, returned))
    }

    /// `value[Symbol.iterator]()` as the method call it is, typed as the
    /// method is declared, when `value`'s class declares it with a type that
    /// is no erasure (a generator, an `Iterator<T>`, a class); `None`
    /// otherwise.
    pub(in crate::lowering) fn symbol_iterator_method_call(
        &mut self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        let (method, returned) = self.symbol_iterator_method(value, body)?;
        if self.type_mentions_unknown(returned) {
            return None;
        }
        Some(body.push_expr(Expr {
            kind: ExprKind::Method {
                receiver: value,
                method,
                args: Vec::new(),
            },
            ty: returned,
            span,
        }))
    }

    /// `value` driven as the generator its iteration is, when it is an
    /// instance whose class declares a zero-argument `[Symbol.iterator]()`:
    /// the method's generator when it is one, else the adapter over the
    /// hand-written iterator it answers ([`Self::protocol_generator`]).
    /// `None` for any other value (a generator, a list or a collection is
    /// iterated as itself).
    ///
    /// # Errors
    /// Unsupported, when `[Symbol.iterator]()` answers a value whose `next()`
    /// is not typed as `IteratorResult<T>` (an erased object literal): it
    /// would otherwise reach the paths for an erased value, which iterate the
    /// instance itself as one item.
    pub(in crate::lowering) fn iteration_generator(
        &mut self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Some((iterator, returned_ty)) = self.symbol_iterator_method(value, body) else {
            return Ok(None);
        };
        let call = |into: &mut Body, ty| {
            into.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver: value,
                    method: iterator,
                    args: Vec::new(),
                },
                ty,
                span,
            })
        };
        match self.ctx.krate.types.get(returned_ty) {
            Some(Type::Generator {
                is_async: false, ..
            }) => return Ok(Some(call(body, returned_ty))),
            Some(Type::Generator { is_async: true, .. }) => return Ok(None),
            _ => {}
        }
        if let Some(access) = self.iterator_protocol_access(returned_ty)
            && let Some(step) = self.protocol_step_type(access)
            && let Some(generator_ty) = self.protocol_generator_type(step)
        {
            let iterator_value = call(body, returned_ty);
            if let Some(generator) =
                self.protocol_generator(access, vec![iterator_value], generator_ty, span, body)
            {
                return Ok(Some(generator));
            }
        }
        Err(SmeltError::unsupported(
            span,
            "iterating an object whose `[Symbol.iterator]()` answers an iterator whose `next()` is not typed as `IteratorResult<T>` is not lowered yet (declare `[Symbol.iterator](): Iterator<T>`)",
        ))
    }

    /// `value`, or the generator [`Self::iteration_generator`] drives it as.
    ///
    /// # Errors
    /// As [`Self::iteration_generator`].
    pub(in crate::lowering) fn iterated_value(
        &mut self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        Ok(self
            .iteration_generator(value, span, body)?
            .unwrap_or(value))
    }

    /// The list an array pattern destructures when its source `value` is an
    /// iterable other than a list: a synchronous generator, or an instance
    /// iterated through `[Symbol.iterator]()`. With `count` (no rest element)
    /// the pattern takes at most that many values and then closes the
    /// iterator, as JavaScript's destructuring does, so an infinite iterator
    /// is fine; with a rest element it drains it. Any other value is
    /// returned unchanged.
    ///
    /// # Errors
    /// As [`Self::iteration_generator`].
    pub(in crate::lowering) fn destructured_iterable(
        &mut self,
        value: ExprId,
        count: Option<usize>,
        span: Span,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let generator = self.iterated_value(value, span, body)?;
        let Some(Type::Generator {
            is_async: false,
            yield_ty,
            ..
        }) = self
            .ctx
            .krate
            .types
            .get(Self::expr_ty(body, generator))
            .cloned()
        else {
            return Ok(value);
        };
        let items_ty = self.ctx.krate.types.intern(Type::List(yield_ty));
        Ok(match count {
            Some(count) => body.push_expr(Expr {
                kind: ExprKind::GeneratorTake { generator, count },
                ty: items_ty,
                span,
            }),
            None => Self::generator_items_expr(generator, items_ty, span, body),
        })
    }

    /// `value` converted for a slot typed `hint`, where the iterator protocol
    /// gives the conversion: a hand-written iterator instance in an
    /// `Iterator<T>` slot is driven through the adapter generator, and an
    /// iterable (a generator, or an instance with `[Symbol.iterator]()`) in
    /// an `Iterable<T>` slot (lowered as a list) is drained into the list of
    /// its items. Any other value is returned unchanged.
    ///
    /// # Errors
    /// As [`Self::iteration_generator`].
    pub(in crate::lowering) fn iterator_value_at_slot(
        &mut self,
        value: ExprId,
        hint: Option<TypeId>,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let Some(hint) = hint else {
            return Ok(value);
        };
        let value_ty = Self::expr_ty(body, value);
        if value_ty == hint {
            return Ok(value);
        }
        let span = Self::body_expr_span(body, value);
        match self.ctx.krate.types.get(hint).cloned() {
            Some(Type::Generator {
                is_async: false,
                yield_ty,
                return_ty,
                ..
            }) => {
                if let Some(access) = self.iterator_protocol_access(value_ty)
                    && let Some(step) = self.protocol_step_type(access)
                    && let Some(Type::GeneratorResult {
                        yield_ty: step_yield_ty,
                        return_ty: step_return_ty,
                    }) = self.ctx.krate.types.get(step).cloned()
                    && self.type_assignable_to(step_yield_ty, yield_ty)
                    // A generator is re-typed only between concrete step
                    // types (an erased resume channel is all that differs);
                    // a generic or erased one is the specialization's to
                    // resolve, not an adapter's.
                    && !(matches!(access, ProtocolAccess::Generator { .. })
                        && [step_yield_ty, step_return_ty, yield_ty, return_ty]
                            .into_iter()
                            .any(|ty| self.type_mentions_unknown(ty) || self.type_mentions_type_param(ty)))
                    && let Some(generator) =
                        self.protocol_generator(access, vec![value], hint, span, body)
                {
                    return Ok(generator);
                }
                Ok(value)
            }
            Some(Type::List(item_ty)) => {
                let generator = match self.ctx.krate.types.get(value_ty) {
                    Some(Type::Generator {
                        is_async: false, ..
                    }) => Some(value),
                    Some(Type::Class { .. }) => self.iteration_generator(value, span, body)?,
                    _ => None,
                };
                let Some(generator) = generator else {
                    return Ok(value);
                };
                let Some(Type::Generator { yield_ty, .. }) = self
                    .ctx
                    .krate
                    .types
                    .get(Self::expr_ty(body, generator))
                    .cloned()
                else {
                    return Ok(value);
                };
                if !self.type_assignable_to(yield_ty, item_ty) {
                    return Ok(value);
                }
                let items_ty = self.ctx.krate.types.intern(Type::List(yield_ty));
                Ok(Self::generator_items_expr(generator, items_ty, span, body))
            }
            _ => Ok(value),
        }
    }

    /// An object literal in a slot of the iterator protocol, lowered as what
    /// that slot holds: a step `{ value, done }` in an `IteratorResult<T, R>`
    /// slot ([`Self::iterator_result_literal`]) and an iterator `{ next,
    /// return }` in an `Iterator<T>` slot ([`Self::iterator_object_literal`]).
    /// `None` for any other slot or literal.
    ///
    /// # Errors
    /// As the two lowerings.
    pub(in crate::lowering) fn iterator_protocol_literal(
        &mut self,
        object: &ObjectExpression<'_>,
        hint: Option<TypeId>,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Some(hint) = hint else {
            return Ok(None);
        };
        match self.ctx.krate.types.get(hint) {
            Some(Type::GeneratorResult { .. }) => self.iterator_result_literal(object, hint, body),
            Some(Type::Generator {
                is_async: false, ..
            }) => self.iterator_object_literal(object, hint, body),
            _ => Ok(None),
        }
    }

    /// Whether a computed key is `Symbol.iterator`.
    pub(in crate::lowering) fn is_symbol_iterator_key(&self, key: &PropertyKey<'_>) -> bool {
        matches!(
            key,
            PropertyKey::StaticMemberExpression(member)
                if member.property.name == "iterator"
                    && matches!(&member.object, Expression::Identifier(object) if self.names_host(object.name.as_str(), "Symbol"))
        )
    }

    /// The static name of an object literal property key, when it is one.
    fn literal_key_name<'k>(key: &'k PropertyKey<'_>) -> Option<&'k str> {
        match key {
            PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str()),
            PropertyKey::StringLiteral(literal) => literal.value.as_str(),
            _ => None,
        }
    }

    /// A step literal `{ value, done }` in an `IteratorResult<T, R>` slot
    /// (`result_ty`): `{ value: v, done: false }` (or no `done`) is the step
    /// yielding `v` at `T`, `{ value: v, done: true }` (or no `value`) the
    /// completion with `v` at `R`, and a computed `done` chooses between the
    /// two at run time, with `value` evaluated once, before `done` when it is
    /// spelled first. `None` (the literal lowers as a record) for a literal
    /// with other properties, a spread, an accessor or a method.
    ///
    /// # Errors
    /// Unsupported, when the value is not assignable to the arm it fills.
    fn iterator_result_literal(
        &mut self,
        object: &ObjectExpression<'_>,
        result_ty: TypeId,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) = self.ctx.krate.types.get(result_ty).cloned()
        else {
            return Ok(None);
        };
        let mut value_expr = None;
        let mut done_expr = None;
        let mut value_first = true;
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Ok(None);
            };
            if property.computed || property.method || property.kind != PropertyKind::Init {
                return Ok(None);
            }
            match Self::literal_key_name(&property.key) {
                Some("value") if value_expr.is_none() => value_expr = Some(&property.value),
                Some("done") if done_expr.is_none() => {
                    value_first = value_expr.is_some();
                    done_expr = Some(&property.value);
                }
                _ => return Ok(None),
            }
        }
        let span = self.span(object.span.start, object.span.end);
        let returns_nothing = matches!(self.ctx.krate.types.get(return_ty), Some(Type::None));
        let static_done = match done_expr
            .map(|done| crate::lowering::peel::peel(done, crate::lowering::peel::Wrappers::Parens))
        {
            None => Some(false),
            Some(Expression::BooleanLiteral(literal)) => Some(literal.value),
            Some(_) => None,
        };
        match static_done {
            Some(false) => {
                let Some(value_expr) = value_expr else {
                    return Ok(None);
                };
                let value = self.expression_with_hint(value_expr, body, Some(yield_ty))?;
                self.ensure_step_value(value, yield_ty, "a yielded step's value", body)?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::GeneratorYielded { value },
                    ty: result_ty,
                    span,
                })))
            }
            Some(true) => {
                let value = value_expr
                    .map(|value_expr| {
                        let hint = (!returns_nothing).then_some(return_ty);
                        self.expression_with_hint(value_expr, body, hint)
                    })
                    .transpose()?;
                if let Some(value) = value
                    && !returns_nothing
                {
                    self.ensure_step_value(value, return_ty, "a completed step's value", body)?;
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::GeneratorCompleted { value },
                    ty: result_ty,
                    span,
                })))
            }
            None => {
                let (Some(value_expr), Some(done_expr)) = (value_expr, done_expr) else {
                    return Ok(None);
                };
                // Both properties evaluate once, in source order, into
                // compiler locals the two arms read.
                let bind = |this: &mut Self,
                            expression: &Expression<'_>,
                            hint,
                            into: &mut Body|
                 -> Result<(LocalId, TypeId), SmeltError> {
                    let lowered = this.expression_with_hint(expression, into, hint)?;
                    let ty = Self::expr_ty(into, lowered);
                    let local = into.push_local(LocalDecl {
                        name: None,
                        ty,
                        mutable: false,
                        span,
                    });
                    let pat = into.push_pattern(Pattern::Binding(local));
                    this.push_stmt_to_sink(
                        into,
                        Stmt::Let {
                            pat,
                            ty,
                            value: Some(lowered),
                        },
                    );
                    Ok((local, ty))
                };
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                let ((value_local, value_ty), (done_local, done_ty)) = if value_first {
                    let value = bind(self, value_expr, Some(yield_ty), body)?;
                    let done = bind(self, done_expr, Some(bool_ty), body)?;
                    (value, done)
                } else {
                    let done = bind(self, done_expr, Some(bool_ty), body)?;
                    let value = bind(self, value_expr, Some(yield_ty), body)?;
                    (value, done)
                };
                if done_ty != bool_ty {
                    return Err(SmeltError::unsupported(
                        span,
                        "an iterator step's computed `done` must be a boolean",
                    ));
                }
                let read = |into: &mut Body, local, ty| {
                    into.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty,
                        span,
                    })
                };
                let yielded_value = read(body, value_local, value_ty);
                self.ensure_step_value(yielded_value, yield_ty, "a yielded step's value", body)?;
                let completed_value = read(body, value_local, value_ty);
                if !returns_nothing {
                    self.ensure_step_value(
                        completed_value,
                        return_ty,
                        "a completed step's value",
                        body,
                    )?;
                }
                let yielded = body.push_expr(Expr {
                    kind: ExprKind::GeneratorYielded {
                        value: yielded_value,
                    },
                    ty: result_ty,
                    span,
                });
                let completed = body.push_expr(Expr {
                    kind: ExprKind::GeneratorCompleted {
                        value: Some(completed_value),
                    },
                    ty: result_ty,
                    span,
                });
                let cond = read(body, done_local, bool_ty);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr: completed,
                        else_expr: yielded,
                    },
                    ty: result_ty,
                    span,
                })))
            }
        }
    }

    /// Refuse a step value whose type is not assignable to the arm `target`
    /// it fills (`what` names the arm).
    fn ensure_step_value(
        &self,
        value: ExprId,
        target: TypeId,
        what: &str,
        body: &Body,
    ) -> Result<(), SmeltError> {
        let value_ty = Self::expr_ty(body, value);
        if value_ty == target || self.type_assignable_to(value_ty, target) {
            return Ok(());
        }
        Err(SmeltError::unsupported(
            Self::body_expr_span(body, value),
            format!("{what} is not assignable to the iterator's declared type"),
        ))
    }

    /// An iterator literal `{ next, return }` in an `Iterator<T, R, N>` slot
    /// (`generator_ty`): the adapter generator over its `next` and `return`
    /// functions ([`Self::protocol_generator`]), each lowered as a function
    /// answering `IteratorResult<T, R>`. A `[Symbol.iterator]` or `throw`
    /// function member is allowed and not kept: the generator is its own
    /// iterator, and no consumer of a synchronous iterator calls `throw`.
    ///
    /// # Errors
    /// Unsupported, for a literal without `next`, with another member (the
    /// state of such an iterator belongs in closures or a class), or whose
    /// `next`/`return` reads its own `this`.
    fn iterator_object_literal(
        &mut self,
        object: &ObjectExpression<'_>,
        generator_ty: TypeId,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Some(Type::Generator {
            is_async: false,
            yield_ty,
            return_ty,
            ..
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return Ok(None);
        };
        let span = self.span(object.span.start, object.span.end);
        let refuse = |this: &Self, at: oxc::span::Span, why: &str| {
            Err(SmeltError::unsupported(
                this.span(at.start, at.end),
                format!("an iterator object literal {why} is not lowered yet"),
            ))
        };
        let mut next = None;
        let mut close = None;
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return refuse(self, object.span, "with a spread member");
            };
            let is_function = matches!(
                property.value,
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            );
            if property.kind != PropertyKind::Init {
                return refuse(self, property.span, "with an accessor");
            }
            if property.computed {
                if is_function && self.is_symbol_iterator_key(&property.key) {
                    continue;
                }
                return refuse(self, property.span, "with a computed member");
            }
            match Self::literal_key_name(&property.key) {
                Some("next") if next.is_none() => next = Some(&property.value),
                Some("return") if close.is_none() => close = Some(&property.value),
                Some("throw") if is_function => {}
                _ => {
                    return refuse(
                        self,
                        property.span,
                        "with members besides `next` and `return` (keep its state in closures or a class)",
                    );
                }
            }
        }
        let Some(next) = next else {
            return refuse(self, object.span, "without `next`");
        };
        let step_ty = self.ctx.krate.types.intern(Type::GeneratorResult {
            yield_ty,
            return_ty,
        });
        let step_fn = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: step_ty,
            is_async: false,
            may_throw: false,
        }));
        let lower_member = |this: &mut Self, member: &Expression<'_>, into: &mut Body| {
            if let Expression::FunctionExpression(function) = member {
                let mut finder = OwnThisFinder::default();
                if let Some(function_body) = &function.body {
                    walk::walk_function_body(&mut finder, function_body);
                }
                if finder.found {
                    return Err(SmeltError::unsupported(
                        this.span(function.span.start, function.span.end),
                        "an iterator object literal whose `next`/`return` reads `this` is not lowered yet (use an arrow over closure state, or a class)",
                    ));
                }
            }
            let lowered = this.expression_with_hint(member, into, Some(step_fn))?;
            let lowered_ty = Self::expr_ty(into, lowered);
            let answers_step = matches!(
                this.ctx.krate.types.get(lowered_ty),
                Some(Type::Function(function))
                    if function.return_ty == step_ty
                        && (function.params.is_empty() || function.required_params == Some(0))
            );
            if !answers_step {
                return Err(SmeltError::unsupported(
                    Self::body_expr_span(into, lowered),
                    "an iterator's `next`/`return` must be a function answering its `IteratorResult<T>`",
                ));
            }
            Ok::<_, SmeltError>((lowered, lowered_ty))
        };
        // Members evaluate in source order.
        let next_first = close.is_none_or(|close| {
            oxc::span::GetSpan::span(next).start < oxc::span::GetSpan::span(close).start
        });
        let (next_value, close_value) = if next_first {
            let next_value = lower_member(self, next, body)?;
            let close_value = close
                .map(|close| lower_member(self, close, body))
                .transpose()?;
            (next_value, close_value)
        } else {
            let close_value = close
                .map(|close| lower_member(self, close, body))
                .transpose()?;
            let next_value = lower_member(self, next, body)?;
            (next_value, close_value)
        };
        let access = ProtocolAccess::Closures {
            next_fn: next_value.1,
            return_fn: close_value.map(|(_, ty)| ty),
        };
        let mut args = vec![next_value.0];
        args.extend(close_value.map(|(value, _)| value));
        Ok(self.protocol_generator(access, args, generator_ty, span, body))
    }
}
