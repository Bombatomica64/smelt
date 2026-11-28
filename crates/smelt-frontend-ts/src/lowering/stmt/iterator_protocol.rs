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
//! * an iterator object literal `{ next, return, throw }` in an `Iterator<T>`
//!   slot, and an instance of a class with `next(): IteratorResult<T>` (in such
//!   a slot, or answered by `[Symbol.iterator]()`), becomes the carrier
//!   `SmeltGenerator::from_protocol` through [`ModuleBuilder::protocol_generator`]:
//!   each resume of the carrier calls the matching protocol method (`next()`
//!   or `next(v)`, `return()` or `return(v)`, `throw(e)`), so what a consumer
//!   sends reaches the iterator and `return()` runs when one closes it early;
//! * a step literal `{ value, done }` in an `IteratorResult<T, R>` slot is the
//!   step itself ([`ModuleBuilder::iterator_result_literal`]).
//!
//! Requirements (see `docs/design/iterator-protocol.md`): the element type `T`
//! is the one the source declares, concrete or generic, never erased; `next()`
//! runs exactly when JavaScript runs it (once per step, lazily, so an infinite
//! iterator is fine); `return()` runs exactly when the consumer leaves before
//! the iterator is done (`break`, `return`, a throw out of the loop body, a
//! destructuring that took what it binds) and never after a `done` step or a
//! `next()` that threw; a value sent with `next(v)`, `return(v)` or `throw(e)`
//! reaches the method, and a resume without one calls it with no argument.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, PropertyKind};
use oxc::ast_visit::{Visit, walk};
use smelt_hir::{
    Body, CaptureMode, ClosureCapture, ClosureExpr, ClosureThis, Expr, ExprId, ExprKind,
    FunctionType, LocalDecl, LocalId, Param, Pattern, Span, Stmt, Type, TypeId,
};

/// A hand-written iterator's protocol methods, each as the function type it
/// is called at (a class method's signature, a record member's or an object
/// literal's closure type), each answering a step `IteratorResult<T, R>`.
#[derive(Clone, Copy, Debug)]
struct ProtocolMethods {
    /// `next`, callable with no argument.
    next: TypeId,
    /// `return`, when the iterator has one, callable with no argument.
    close: Option<TypeId>,
    /// `throw`, when the iterator has one.
    throw: Option<TypeId>,
}

/// How the carrier reaches a hand-written iterator's protocol methods.
#[derive(Clone, Copy, Debug)]
enum ProtocolAccess {
    /// The methods of a class instance (the adapter's one parameter).
    Method {
        /// The instance's type.
        instance_ty: TypeId,
        /// Its protocol methods.
        methods: ProtocolMethods,
    },
    /// The function-valued members of a record or interface value (the
    /// adapter's one parameter), called as the closures they are.
    Member {
        /// The record's type.
        instance_ty: TypeId,
        /// Its protocol members.
        methods: ProtocolMethods,
    },
    /// A generator of another type than the slot's (`Generator<T>`, whose
    /// resume channel is erased, in an `Iterator<T>` slot): resumed with
    /// `next`, `return` and `throw` (the adapter's one parameter).
    Generator {
        /// The generator's type.
        generator_ty: TypeId,
        /// Its step type.
        step_ty: TypeId,
    },
    /// An iterator object literal's `next`, `return` and `throw` closures (the
    /// adapter's parameters, in that order, those present).
    Closures {
        /// The closures' types.
        methods: ProtocolMethods,
    },
}

/// Which protocol method a carrier closure calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProtocolCall {
    /// `next()` / `next(sent)`.
    Next,
    /// `return()` / `return(value)`.
    Return,
    /// `throw(error)`.
    Throw,
}

impl ProtocolCall {
    /// The source name of the method.
    const fn name(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Return => "return",
            Self::Throw => "throw",
        }
    }

    /// The generator resume this call is on a generator.
    const fn resume_kind(self) -> smelt_hir::GeneratorResumeKind {
        match self {
            Self::Next => smelt_hir::GeneratorResumeKind::Next,
            Self::Return => smelt_hir::GeneratorResumeKind::Return,
            Self::Throw => smelt_hir::GeneratorResumeKind::Throw,
        }
    }
}

/// What a carrier closure sends its protocol method.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProtocolArg {
    /// Nothing: `next()`, `return()`.
    Absent,
    /// The value the consumer sent, of this type: the carrier's resume type
    /// for `next(sent)`, its return type for `return(value)`, the thrown
    /// value for `throw(error)`.
    Sent(TypeId),
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
    /// a class instance whose `next()` is callable with no argument and
    /// answers a typed step (`IteratorResult<T, R>`), or a record/interface
    /// value whose `next` member is such a function. `return` (callable with
    /// no argument) and `throw` are used when present and answering a step,
    /// so a consumer can close the iterator and `yield*` can forward a throw.
    /// A generator of another type is driven through its own resumes. `None`
    /// for any other type.
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
        // A miss records an erasure for the absent member; the question is a
        // probe, so nothing it recorded stays.
        let records_before = self.ctx.krate.erasures.len();
        let member_fn = |this: &mut Self, call: ProtocolCall| {
            let member = this.intern_source_name(call.name());
            let member_ty = this.class_field_type(ty, member).ok()?;
            // A member's function type does not record how many parameters
            // are optional (`next(sent?: number)`); its declared signature does.
            let required = this.declared_required_params(name, member);
            this.protocol_method_type(member_ty, call, required)
        };
        let next = member_fn(self, ProtocolCall::Next);
        let close = member_fn(self, ProtocolCall::Return);
        let throw = member_fn(self, ProtocolCall::Throw);
        self.ctx.krate.erasures.truncate(records_before);
        let methods = ProtocolMethods {
            next: next?,
            close,
            throw,
        };
        if self.find_interface(name).is_some() {
            Some(ProtocolAccess::Member {
                instance_ty: ty,
                methods,
            })
        } else {
            Some(ProtocolAccess::Method {
                instance_ty: ty,
                methods,
            })
        }
    }

    /// `function_ty` when it can be called as the protocol method `call`: a
    /// function answering a step `IteratorResult<T, R>`, callable with no
    /// argument (`next`, `return`) or with at most the thrown value (`throw`).
    /// `required` overrides the function type's own count of required
    /// parameters (a class or interface member's declared signature).
    fn protocol_method_type(
        &self,
        function_ty: TypeId,
        call: ProtocolCall,
        required: Option<usize>,
    ) -> Option<TypeId> {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty) else {
            return None;
        };
        let most_required = usize::from(call == ProtocolCall::Throw);
        // A class method's function type records no count; the checker has
        // already accepted the value where an iterator is expected (or in a
        // `for...of`), which requires its `next()` callable with no argument,
        // and a call with fewer arguments passes `undefined` for the rest.
        let required = required.or(function.required_params).unwrap_or(0);
        (required <= most_required
            && matches!(
                self.ctx.krate.types.get(function.return_ty),
                Some(Type::GeneratorResult { .. })
            ))
        .then_some(function_ty)
    }

    /// How many leading parameters the method `member` of the class or
    /// interface `owner` requires, from its declared signature; `None` when
    /// neither declares it here (an inherited method).
    fn declared_required_params(
        &self,
        owner: smelt_hir::Symbol,
        member: smelt_hir::Symbol,
    ) -> Option<usize> {
        if let Some(interface) = self.find_interface(owner) {
            return interface
                .methods
                .iter()
                .find(|method| method.name == member)
                .and_then(|method| method.required_params);
        }
        let text = crate::lowering::support::source_spelling(&self.ctx.krate, owner)?;
        self.classes
            .methods(text)?
            .iter()
            .find(|method| method.name == member)
            .and_then(|method| method.required_params)
    }

    /// Whether `ty` is the erased record an object literal with
    /// function-valued members lowers to (`Dict<string, unknown>`).
    fn is_erased_record(&self, ty: TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Dict(key, value))
                if matches!(self.ctx.krate.types.get(*key), Some(Type::String))
                    && matches!(self.ctx.krate.types.get(*value), Some(Type::Unknown))
        )
    }

    /// The step type `access`'s `next()` answers.
    fn protocol_step_type(&self, access: ProtocolAccess) -> Option<TypeId> {
        let step = match access {
            ProtocolAccess::Generator { step_ty, .. } => step_ty,
            ProtocolAccess::Method { methods, .. }
            | ProtocolAccess::Member { methods, .. }
            | ProtocolAccess::Closures { methods } => {
                match self.ctx.krate.types.get(methods.next) {
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

    /// One call of `access`'s protocol method `call` with `args`, inside a
    /// carrier closure whose captures of the iterator are `captured` (its
    /// one value, or an object literal's closures in [`ProtocolAccess::Closures`]
    /// order).
    fn protocol_call(
        &mut self,
        access: ProtocolAccess,
        call: ProtocolCall,
        args: Vec<ExprId>,
        captured: &[LocalId],
        span: Span,
        closure: &mut Body,
    ) -> Option<ExprId> {
        let method_of = |methods: ProtocolMethods| match call {
            ProtocolCall::Next => Some(methods.next),
            ProtocolCall::Return => methods.close,
            ProtocolCall::Throw => methods.throw,
        };
        let returned = |types: &smelt_hir::TypeInterner, function: TypeId| match types.get(function)
        {
            Some(Type::Function(function)) => Some(function.return_ty),
            _ => None,
        };
        match access {
            ProtocolAccess::Generator {
                generator_ty,
                step_ty,
            } => {
                let generator = closure.push_expr(Expr {
                    kind: ExprKind::Local(*captured.first()?),
                    ty: generator_ty,
                    span,
                });
                Some(closure.push_expr(Expr {
                    kind: ExprKind::GeneratorNext {
                        generator,
                        value: args.first().copied(),
                        kind: call.resume_kind(),
                    },
                    ty: step_ty,
                    span,
                }))
            }
            ProtocolAccess::Method {
                instance_ty,
                methods,
            } => {
                let ty = returned(&self.ctx.krate.types, method_of(methods)?)?;
                let receiver = closure.push_expr(Expr {
                    kind: ExprKind::Local(*captured.first()?),
                    ty: instance_ty,
                    span,
                });
                let method = self.intern_source_name(call.name());
                Some(closure.push_expr(Expr {
                    kind: ExprKind::Method {
                        receiver,
                        method,
                        args,
                    },
                    ty,
                    span,
                }))
            }
            ProtocolAccess::Member {
                instance_ty,
                methods,
            } => {
                let function = method_of(methods)?;
                let ty = returned(&self.ctx.krate.types, function)?;
                let receiver = closure.push_expr(Expr {
                    kind: ExprKind::Local(*captured.first()?),
                    ty: instance_ty,
                    span,
                });
                let field = self.intern_source_name(call.name());
                let callee = closure.push_expr(Expr {
                    kind: ExprKind::Field { receiver, field },
                    ty: function,
                    span,
                });
                Some(closure.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty,
                    span,
                }))
            }
            ProtocolAccess::Closures { methods } => {
                let function = method_of(methods)?;
                let ty = returned(&self.ctx.krate.types, function)?;
                // The captures are `next`, then `return` and `throw` when present.
                let index = match call {
                    ProtocolCall::Next => 0,
                    ProtocolCall::Return => 1,
                    ProtocolCall::Throw => 1 + usize::from(methods.close.is_some()),
                };
                let callee = closure.push_expr(Expr {
                    kind: ExprKind::Local(*captured.get(index)?),
                    ty: function,
                    span,
                });
                Some(closure.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty,
                    span,
                }))
            }
        }
    }

    /// Whether `access`'s method `call` declares a parameter, so the value a
    /// consumer sends reaches it (`next(sent)`, `return(value)`,
    /// `throw(error)`). A generator takes one on every resume.
    fn protocol_method_takes_value(&self, access: ProtocolAccess, call: ProtocolCall) -> bool {
        let methods = match access {
            ProtocolAccess::Generator { .. } => return true,
            ProtocolAccess::Method { methods, .. }
            | ProtocolAccess::Member { methods, .. }
            | ProtocolAccess::Closures { methods } => methods,
        };
        let method = match call {
            ProtocolCall::Next => Some(methods.next),
            ProtocolCall::Return => methods.close,
            ProtocolCall::Throw => methods.throw,
        };
        method.is_some_and(|method| {
            matches!(self.ctx.krate.types.get(method), Some(Type::Function(function)) if !function.params.is_empty())
        })
    }

    /// The carrier closure calling `access`'s method `call`: `() =>
    /// it.next()`, `(sent: N) => it.next(sent)`, ..., built in `outer` (the
    /// adapter's body), capturing the adapter's parameters `iterator`. With
    /// [`ProtocolArg::Sent`] the closure takes the value, and passes it on
    /// when `pass` (the method declares a parameter).
    #[expect(
        clippy::too_many_arguments,
        reason = "one closure per protocol method; each argument is a distinct part of it"
    )]
    fn protocol_closure(
        &mut self,
        access: ProtocolAccess,
        call: ProtocolCall,
        arg: ProtocolArg,
        pass: bool,
        iterator: &[(LocalId, TypeId, smelt_hir::Symbol)],
        span: Span,
        outer: &mut Body,
    ) -> Option<ExprId> {
        let mut closure = Body::new(None, span);
        let mut captures = Vec::new();
        let mut captured = Vec::new();
        for &(source_local, ty, symbol) in iterator {
            let body_local = closure.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty,
                mode: CaptureMode::ByValue,
            });
            captured.push(body_local);
        }
        let mut params = Vec::new();
        let mut args = Vec::new();
        if let ProtocolArg::Sent(ty) = arg {
            let symbol = self.intern_synthesized_name("__smelt_iterator_sent");
            let local = closure.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            closure.params.push(local);
            params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
            if pass {
                args.push(closure.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty,
                    span,
                }));
            }
        }
        let call_expr = self.protocol_call(access, call, args, &captured, span, &mut closure)?;
        let return_ty = Self::expr_ty(&closure, call_expr);
        closure.blocks.first_mut()?.tail = Some(call_expr);
        let body_id = self.ctx.krate.push_body(closure);
        let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let required_params = Some(param_tys.len());
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            // MIR marks a closure whose body can throw (a throwing method).
            may_throw: false,
        }));
        Some(outer.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params,
                rest: None,
                required_params,
                return_ty,
                captures,
                body: body_id,
                name: None,
                length: None,
                function_item: None,
                this_binding: ClosureThis::Own,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// The carrier of type `generator_ty` over the hand-written iterator
    /// `access` reaches through `args`: an immediately called, synthesized
    /// closure over the iterator that answers
    /// `SmeltGenerator::from_protocol` (HIR op `GeneratorFromProtocol`) of a
    /// closure per protocol method,
    ///
    /// ```text
    /// (it) => from_protocol(
    ///   it,                          // identity of the adapted object
    ///   () => it.next(), (sent) => it.next(sent),
    ///   () => it.return(), (value) => it.return(value),
    ///   (error) => it.throw(error),
    /// )
    /// ```
    ///
    /// (those the iterator has). Every resume of the carrier is then one call
    /// of the matching method, as JavaScript's consumers make it: `for...of`
    /// calls `next()` with no argument and `return()` when it leaves early
    /// (`IteratorClose`), `gen.next(v)` and `yield*` send `v` to `next(v)`,
    /// and `yield*` forwards `throw(e)` and `return(v)`. The carrier is lazy,
    /// so an infinite iterator is fine.
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
            return_ty,
            next_ty,
            ..
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return None;
        };
        self.protocol_step_type(access)?;
        let mut adapter = Body::new(None, span);

        // Parameters: the iterator, or its `next`, `return` and `throw` closures.
        let param_specs: Vec<(&str, TypeId)> = match access {
            ProtocolAccess::Method { instance_ty, .. }
            | ProtocolAccess::Member { instance_ty, .. }
            | ProtocolAccess::Generator {
                generator_ty: instance_ty,
                ..
            } => vec![("__smelt_iterator", instance_ty)],
            ProtocolAccess::Closures { methods } => [
                ("__smelt_iterator_next", Some(methods.next)),
                ("__smelt_iterator_return", methods.close),
                ("__smelt_iterator_throw", methods.throw),
            ]
            .into_iter()
            .filter_map(|(name, ty)| Some((name, ty?)))
            .collect(),
        };
        let mut params = Vec::new();
        let mut iterator = Vec::new();
        for (name, ty) in param_specs {
            let symbol = self.intern_synthesized_name(name);
            let local = adapter.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            adapter.params.push(local);
            iterator.push((local, ty, symbol));
            params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
        }

        let (closes, throws) = match access {
            ProtocolAccess::Generator { .. } => (true, true),
            ProtocolAccess::Method { methods, .. }
            | ProtocolAccess::Member { methods, .. }
            | ProtocolAccess::Closures { methods } => {
                (methods.close.is_some(), methods.throw.is_some())
            }
        };
        // A resume or return channel of `undefined` sends nothing to forward.
        let carries =
            |this: &Self, ty: TypeId| !matches!(this.ctx.krate.types.get(ty), Some(Type::None));
        let closure = |this: &mut Self, call, arg, adapter: &mut Body| {
            let pass = this.protocol_method_takes_value(access, call);
            this.protocol_closure(access, call, arg, pass, &iterator, span, adapter)
        };
        let next = closure(self, ProtocolCall::Next, ProtocolArg::Absent, &mut adapter)?;
        let next_with = if carries(self, next_ty)
            && self.protocol_method_takes_value(access, ProtocolCall::Next)
        {
            Some(closure(
                self,
                ProtocolCall::Next,
                ProtocolArg::Sent(next_ty),
                &mut adapter,
            )?)
        } else {
            None
        };
        let close = if closes {
            Some(closure(
                self,
                ProtocolCall::Return,
                ProtocolArg::Absent,
                &mut adapter,
            )?)
        } else {
            None
        };
        let close_with = if closes
            && carries(self, return_ty)
            && self.protocol_method_takes_value(access, ProtocolCall::Return)
        {
            Some(closure(
                self,
                ProtocolCall::Return,
                ProtocolArg::Sent(return_ty),
                &mut adapter,
            )?)
        } else {
            None
        };
        let throw_with = if throws {
            // The thrown value is the carrier's throw channel, the tagged
            // value `SmeltGeneratorCommand::Throw` already carries (a
            // JavaScript throw may be any value); the method call converts it
            // to the parameter type the iterator's `throw` declares.
            let thrown = self.unknown_type(NotNewErasure::RuntimeHelperSignature);
            Some(closure(
                self,
                ProtocolCall::Throw,
                ProtocolArg::Sent(thrown),
                &mut adapter,
            )?)
        } else {
            None
        };
        let source = match access {
            ProtocolAccess::Method { instance_ty, .. }
            | ProtocolAccess::Member { instance_ty, .. } => {
                let (local, ..) = *iterator.first()?;
                Some(adapter.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: instance_ty,
                    span,
                }))
            }
            ProtocolAccess::Generator { .. } | ProtocolAccess::Closures { .. } => None,
        };
        let carrier = adapter.push_expr(Expr {
            kind: ExprKind::GeneratorFromProtocol {
                source,
                next,
                next_with,
                close,
                close_with,
                throw_with,
            },
            ty: generator_ty,
            span,
        });
        adapter.blocks.first_mut()?.tail = Some(carrier);

        let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let body_id = self.ctx.krate.push_body(adapter);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: generator_ty,
            is_async: false,
            may_throw: false,
        }));
        let adapter_closure = body.push_expr(Expr {
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
                callee: adapter_closure,
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
                // An object literal with methods (state members, a `next` that
                // reads `this`) is lowered as an erased record, which no typed
                // adapter can drive: refuse it here rather than hand an
                // erased record to a typed `Iterator<T>` slot.
                if self.is_erased_record(value_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "an iterator object with state members or a `next` that reads `this` is not lowered yet (Smelt lowers an object literal with methods as an erased record; keep the state in closures or a class)",
                    ));
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

    /// An iterator literal `{ next, return, throw }` in an `Iterator<T, R, N>`
    /// slot (`generator_ty`): the carrier over its functions
    /// ([`Self::protocol_generator`]), each lowered as a function answering
    /// `IteratorResult<T, R>`, in source order. A `[Symbol.iterator]` function
    /// member is allowed and not kept: the carrier is its own iterator.
    ///
    /// # Errors
    /// Unsupported, for a literal without `next`, with another member (the
    /// state of such an iterator belongs in closures or a class), or whose
    /// protocol functions read their own `this`.
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
        let mut members: Vec<(ProtocolCall, &Expression<'_>)> = Vec::new();
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
            let call = match Self::literal_key_name(&property.key) {
                Some("next") => ProtocolCall::Next,
                Some("return") => ProtocolCall::Return,
                Some("throw") => ProtocolCall::Throw,
                _ => {
                    return refuse(
                        self,
                        property.span,
                        "with members besides `next`, `return` and `throw` (keep its state in closures or a class)",
                    );
                }
            };
            if members.iter().any(|(seen, _)| *seen == call) {
                return refuse(self, property.span, "with a repeated protocol member");
            }
            members.push((call, &property.value));
        }
        if !members.iter().any(|(call, _)| *call == ProtocolCall::Next) {
            return refuse(self, object.span, "without `next`");
        }
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
        // Members evaluate in source order.
        let mut lowered = Vec::with_capacity(members.len());
        for (call, member) in members {
            if let Expression::FunctionExpression(function) = member {
                let mut finder = OwnThisFinder::default();
                if let Some(function_body) = &function.body {
                    walk::walk_function_body(&mut finder, function_body);
                }
                if finder.found {
                    return Err(SmeltError::unsupported(
                        self.span(function.span.start, function.span.end),
                        "an iterator object literal whose `next`/`return`/`throw` reads `this` is not lowered yet (use an arrow over closure state, or a class)",
                    ));
                }
            }
            let value = self.expression_with_hint(member, body, Some(step_fn))?;
            let value_ty = Self::expr_ty(body, value);
            let answers_step = self
                .protocol_method_type(value_ty, call, None)
                .is_some_and(|_| {
                    matches!(
                        self.ctx.krate.types.get(value_ty),
                        Some(Type::Function(function)) if function.return_ty == step_ty
                    )
                });
            if !answers_step {
                return Err(SmeltError::unsupported(
                    Self::body_expr_span(body, value),
                    "an iterator's `next`/`return`/`throw` must be a function answering its `IteratorResult<T>`",
                ));
            }
            lowered.push((call, value, value_ty));
        }
        let member = |call: ProtocolCall| lowered.iter().find(|(seen, ..)| *seen == call).copied();
        let next = member(ProtocolCall::Next);
        let close = member(ProtocolCall::Return);
        let throw = member(ProtocolCall::Throw);
        let Some((_, next_value, next_fn)) = next else {
            return refuse(self, object.span, "without `next`");
        };
        let access = ProtocolAccess::Closures {
            methods: ProtocolMethods {
                next: next_fn,
                close: close.map(|(_, _, ty)| ty),
                throw: throw.map(|(_, _, ty)| ty),
            },
        };
        let mut args = vec![next_value];
        args.extend(close.map(|(_, value, _)| value));
        args.extend(throw.map(|(_, value, _)| value));
        Ok(self.protocol_generator(access, args, generator_ty, span, body))
    }
}
