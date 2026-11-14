//! Member calls: `receiver.method(..)`, computed method calls, class statics and
//! private callable fields, and the receiver facts those calls are decided on.

use crate::SmeltError;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::spread_arguments::CallArg;
use crate::lowering::support::source_spelling;
use crate::lowering::{ModuleBuilder, RestParam};
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, FunctionType, Item, Literal, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Lower a member call `object.property(args)` once the builtin, static and
    /// namespace pre-passes in [`Self::call_expression`] have all declined it.
    ///
    /// The callee's *spelling* is passed apart from its parts so that every
    /// member-call syntax shares one lowering: a `StaticMemberExpression`
    /// (`o.m()`) and a `PrivateFieldExpression` (`o.#m()`) differ only in which
    /// AST node carries the property name, and an ES private method is
    /// registered under its bare source name exactly like a public one.
    ///
    /// * `object` — the receiver expression, lowered first so its type is known.
    /// * `property_name` — the method's source name, without any `#` sigil.
    /// * `member_span` — the whole member expression, used for method resolution
    ///   diagnostics.
    /// * `member_optional` — whether the ACCESS was optional (`o?.m()`); the
    ///   CALL's own optionality (`o.m?.()`) is read from `call.optional`.
    ///
    /// Three results are possible: `ExprKind::ListNext` for the iterator
    /// protocol's `next()` on a list receiver, `ExprKind::OptionalMethod` when
    /// either the access or the receiver type is optional, and
    /// `ExprKind::Method` otherwise. A call that resolved to neither a concrete
    /// method item nor a typed return is a dynamic boundary: an erased member call.
    pub(in crate::lowering) fn member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        object: &Expression<'_>,
        property_name: &str,
        member_span: oxc::span::Span,
        member_optional: bool,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if property_name == "next" && call.arguments.is_empty() {
            let receiver = self.expression(object, body)?;
            let receiver_ty = Self::expr_ty(body, receiver);
            if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(receiver_ty) {
                // An iterator result is a type of its own: `IteratorResult<T, undefined>`.
                // `.done` and `.value` read that protocol, and a field of those
                // names on an `Optional` value is that value's own field.
                let yield_ty = *item_ty;
                let return_ty = self.ctx.krate.types.intern(Type::None);
                let ty = self.ctx.krate.types.intern(Type::GeneratorResult {
                    yield_ty,
                    return_ty,
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ListNext { list: receiver },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
        }
        let receiver = self.expression(object, body)?;
        let method = self.intern_source_name(property_name);
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access = call.optional
            || member_optional
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            );
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        let (return_ty, method_item) =
            self.resolve_method(access_receiver_ty, method, member_span)?;
        // A METHOD callee is the sixth fixed-arity argument collection, and it
        // needs the same spread expansion as the other five: `sink.take(...triple)`
        // bound the whole tuple to the first parameter and let the emitter default
        // the rest. See `lowering::spread_arguments`.
        let expanded = self.expanded_call_arguments(&call.arguments, body)?;
        // A trailing LIST spread binds the method's remaining fixed parameters
        // to its elements, as it does for every other fixed-arity callee
        // (Hono's `(...args) => this.#newResponse(...args)`). Without this the
        // list became parameter 0 and the emitter defaulted the rest.
        let expanded = match self.class_method_fixed_params(access_receiver_ty, method) {
            Some(params) => self.distribute_trailing_list_spread(expanded, &params, body)?,
            None => expanded,
        };
        let callback_param_hints =
            self.class_method_callback_param_hints(access_receiver_ty, method);
        let rest_signature = self.class_method_rest_signature(access_receiver_ty, method);
        let spread_positions = expanded
            .iter()
            .map(|arg| matches!(arg, CallArg::UnexpandedSpread(_)))
            .collect::<Vec<_>>();
        let mut args = Vec::with_capacity(expanded.len());
        for (index, arg) in expanded.into_iter().enumerate() {
            if let CallArg::Source(source) = arg
                && (property_name == "test"
                    || self.ctx.krate.types.get(return_ty) == Some(&Type::Unknown))
                && matches!(
                    source,
                    Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
                )
            {
                let ty = if property_name == "test" {
                    self.erase(ErasureReason::TestHarness)
                } else {
                    // not a new erasure: the method's result is already erased
                    self.unknown_type(NotNewErasure::ErasedUpstream)
                };
                args.push(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(source.span().start, source.span().end),
                }));
            } else {
                let hint = callback_param_hints.get(index).copied().flatten();
                args.push(self.lower_call_arg(arg, hint, body)?);
            }
        }
        // A resolved generic method instantiates its OWN type parameters (to
        // `unknown`, what its erased definition returns); class parameters
        // were already substituted from the receiver by `resolve_method`.
        let has_rest = rest_signature.is_some();
        if let Some((rest_index, params)) = rest_signature {
            let span = self.span(call.span.start, call.span.end);
            args =
                self.pack_rest_arguments(args, &spread_positions, rest_index, &params, span, body);
        }
        // A generic method's own type parameters are bound per call the way
        // a generic free function's are (from the arguments and the
        // checker's answer, `instantiated_call_return`): the method is
        // emitted with real Rust generics, so its call is as typed as theirs.
        let return_ty = if method_item.0 == u32::MAX {
            return_ty
        } else {
            let method_type_params = self.callee_own_type_params(method_item);
            match self.class_method_fixed_params(access_receiver_ty, method) {
                Some(params) if !method_type_params.is_empty() && !has_rest => self
                    .instantiated_call_return(
                        &method_type_params,
                        &params,
                        &args,
                        return_ty,
                        call,
                        body,
                    )?,
                _ => self.instantiate_generic_method_result(&method_type_params, return_ty),
            }
        };
        if method_item.0 == u32::MAX
            && self.ctx.krate.types.get(return_ty) == Some(&Type::Unknown)
            && !self.receiver_declares_in_progress_method(
                access_receiver_ty,
                method,
                member_span,
            )?
        {
            // No declaration answers this method, so the value decides at run
            // time through the erased call ABI.
            let span = self.span(call.span.start, call.span.end);
            // `valueOf()` on a closed prototype is the value itself:
            // `Object.prototype.valueOf` for an array, and each primitive
            // wrapper's `valueOf` for its primitive. An absent optional
            // receiver (`x?.valueOf()`) is `undefined`, which is the receiver
            // too.
            if property_name == "valueOf"
                && args.is_empty()
                && self.value_of_is_identity(access_receiver_ty)
            {
                return Ok(receiver);
            }
            if let Some(blocked) =
                self.closed_prototype_call_blocked(access_receiver_ty, property_name, span, body)?
            {
                return Ok(blocked);
            }
            // not a new erasure: the resolved method already returns `unknown`
            let ty = self.unknown_type(NotNewErasure::ErasedUpstream);
            let value = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: receiver },
                ty,
                span,
            });
            let callee = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver: value,
                    field: method,
                },
                ty,
                span,
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty,
                span,
            }));
        }
        if optional_access {
            let receiver = if call.optional || member_optional {
                self.optionalize_index_receiver(receiver, body)
            } else {
                receiver
            };
            let ty = self.optional_chain_result_type(return_ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::OptionalMethod {
                    receiver,
                    method,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::Method {
                receiver,
                method,
                args,
            },
            ty: return_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Report a call of a method a closed prototype does not model
    /// ([`Self::closed_prototype_missing`]); `None` when the method is modelled.
    ///
    /// Outside the test tier the program cannot run as written, so it is a
    /// transpile-time error. In a test-tier file one unmodelled method must not
    /// take the rest of the suite down, so the call site throws a `TypeError`
    /// naming the missing model instead: the test that reaches it fails with
    /// that reason and its siblings run. It is deliberately not the
    /// answer-everything `UnmodeledImport` value, through which a test would
    /// pass having asserted nothing.
    fn closed_prototype_call_blocked(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: &str,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(prototype) = self.closed_prototype_missing(receiver_ty, method) else {
            return Ok(None);
        };
        let message = format!("`{prototype}.prototype.{method}` is not modelled");
        if !self.test_tier_module {
            return Err(SmeltError::unsupported(span, message));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let never_ty = self.ctx.krate.types.intern(Type::Never);
        let mut closure_body = Body::new(None, span);
        let text = closure_body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(message)),
            ty: string_ty,
            span,
        });
        let error =
            self.error_object_from_message(Some(text), "TypeError", span, &mut closure_body);
        closure_body.push_stmt(Stmt::Throw(error));
        let body_id = self.ctx.krate.push_body(closure_body);
        let function = FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: never_ty,
            is_async: false,
            may_throw: true,
        };
        let closure_ty = self.ctx.krate.types.intern(Type::Function(function));
        let callee = body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: Vec::new(),
                rest: None,
                required_params: None,
                return_ty: never_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        });
        // not a new erasure: the erased call this replaces answered `unknown`
        let ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee,
                args: Vec::new(),
            },
            ty,
            span,
        })))
    }

    /// The prototype whose missing `method` a call on `receiver_ty` would
    /// reach, when that receiver's prototype is closed: a primitive, an array
    /// or a tuple.
    ///
    /// Such a receiver answers exactly the members Smelt models. A method no
    /// static lowering claimed reaches the erased call ABI, where the runtime
    /// answers only the [`smelt_stdlib::erased_prototype_methods`] tables (a
    /// number or boolean answers none), so any other name always threw
    /// `TypeError: value is not a function` at run time where JavaScript runs
    /// the method. That is a missing model, reported here at transpile time.
    /// Every other receiver (erased, generic, a union, a record, a class) can
    /// hold the member at run time and keeps the erased call.
    fn closed_prototype_missing(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: &str,
    ) -> Option<&'static str> {
        use smelt_stdlib::erased_prototype_methods::{
            erased_array_prototype_method, erased_string_prototype_method,
        };
        let (prototype, answered) = match self.ctx.krate.types.get(receiver_ty)? {
            Type::String => ("String", erased_string_prototype_method(method).is_some()),
            Type::List(_) | Type::Tuple(_) => {
                ("Array", erased_array_prototype_method(method).is_some())
            }
            Type::Float | Type::Int | Type::FixedInt(_) => ("Number", false),
            Type::Bool => ("Boolean", false),
            Type::BigInt => ("BigInt", false),
            _ => return None,
        };
        (!answered && method != "toString").then_some(prototype)
    }

    /// Whether `receiver_ty.valueOf()` is the receiver itself: an array, a
    /// tuple or a primitive, whose prototypes no source can redefine.
    fn value_of_is_identity(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(
                Type::String
                    | Type::List(_)
                    | Type::Tuple(_)
                    | Type::Float
                    | Type::Int
                    | Type::FixedInt(_)
                    | Type::Bool
                    | Type::BigInt
                    | Type::Symbol
            )
        )
    }

    /// Contextual types for function-valued parameters of a class method.
    ///
    /// A function literal passed to a method is contextually typed by the
    /// method's declared parameter, exactly as it is for a free-function call:
    /// `bag.fold((acc, x) => acc + x, 0)` against
    /// `fold(cb: (acc: number, x: number) => number, init: number)` gives the
    /// arrow `(f64, f64) -> f64`. Without the hint the arrow's parameters were
    /// `Unknown`, and the call adapted an erased closure into the typed
    /// parameter (and did not even type-check against a `&dyn Fn` parameter).
    ///
    /// Entry `i` is `Some(param_ty)` only when the receiver is a class, the
    /// method's type is a function, and parameter `i` is itself a function type
    /// with no unresolved type parameter (a method-generic `(x: U) => V` still
    /// needs the instantiation the free-function path infers from the sibling
    /// arguments). Every other position is `None`: only function types are
    /// offered.
    fn class_method_callback_param_hints(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> Vec<Option<smelt_hir::TypeId>> {
        if !matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Class { .. })
        ) {
            return Vec::new();
        }
        let Ok(method_ty) = self.class_field_type(receiver_ty, method) else {
            return Vec::new();
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(method_ty).cloned() else {
            return Vec::new();
        };
        let fixed = function.rest.unwrap_or(function.params.len());
        function
            .params
            .iter()
            .take(fixed)
            .map(|param| {
                (matches!(self.ctx.krate.types.get(*param), Some(Type::Function(_)))
                    && !self.overload_constraint_contains_unresolved_type_param(*param))
                .then_some(*param)
            })
            .collect()
    }

    /// The rest position and parameter types of a class method with a rest
    /// parameter, or `None`.
    fn class_method_rest_signature(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> Option<(usize, Vec<smelt_hir::TypeId>)> {
        if !matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Class { .. })
        ) {
            return None;
        }
        let method_ty = self.class_field_type(receiver_ty, method).ok()?;
        let Some(Type::Function(function)) = self.ctx.krate.types.get(method_ty).cloned() else {
            return None;
        };
        let rest = function.rest?;
        function.params.get(rest)?;
        Some((rest, function.params))
    }

    /// Bind a rest parameter to the arguments from its position on.
    ///
    /// JavaScript collects them into one array. A single spread of a list in
    /// that position is that array already (`m(...xs)` into `m(...rest)`), so
    /// it is passed through; any other tail becomes a list literal.
    fn pack_rest_arguments(
        &self,
        mut args: Vec<smelt_hir::ExprId>,
        spread_positions: &[bool],
        rest_index: usize,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> Vec<smelt_hir::ExprId> {
        // A missing fixed argument before the rest position is an omitted
        // optional one: a `None` at the parameter's own type, as the
        // fixed-arity value-call path synthesizes.
        for param_ty in params.iter().copied().take(rest_index).skip(args.len()) {
            args.push(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: param_ty,
                span,
            }));
        }
        let rest_ty = params[rest_index];
        let tail = args.split_off(rest_index);
        let single_list_spread = tail.len() == 1
            && spread_positions.get(rest_index).copied().unwrap_or(false)
            && matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, tail[0])),
                Some(Type::List(_))
            );
        let packed = if single_list_spread {
            tail[0]
        } else {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(tail),
                ty: rest_ty,
                span,
            })
        };
        args.push(packed);
        args
    }

    /// The list-typed rest parameter of `function`, when it has one.
    pub(in crate::lowering) fn list_rest_param(
        &self,
        function: &smelt_hir::FunctionType,
    ) -> Option<RestParam> {
        function.rest.and_then(|index| {
            let param = self.type_param_constraint_or_self(*function.params.get(index)?);
            match self.ctx.krate.types.get(param) {
                Some(Type::List(item_ty)) => Some(RestParam {
                    index,
                    item_ty: *item_ty,
                }),
                _ => None,
            }
        })
    }

    /// Pack the arguments of a call through a function VALUE into its parameters.
    ///
    /// A rest parameter receives one list, so `ClosureCall` operands line up with
    /// parameters one to one. `spread_positions` marks which supplied arguments
    /// are spreads, by position. A callee with no list-typed rest keeps its
    /// arguments as they are.
    pub(in crate::lowering) fn pack_value_call_arguments(
        &self,
        function: &smelt_hir::FunctionType,
        args: Vec<smelt_hir::ExprId>,
        spread_positions: &[bool],
        span: Span,
        body: &mut Body,
    ) -> Vec<smelt_hir::ExprId> {
        let Some(rest_index) = function.rest else {
            return args;
        };
        let rest_is_list = function.params.len() == rest_index + 1
            && matches!(
                self.ctx.krate.types.get(function.params[rest_index]),
                Some(Type::List(_))
            );
        if !rest_is_list {
            return args;
        }
        self.pack_rest_arguments(
            args,
            spread_positions,
            rest_index,
            &function.params,
            span,
            body,
        )
    }

    /// The parameter types of a class method with no rest parameter, or `None`
    /// when the receiver is not a class or the method is variadic or untyped.
    fn class_method_fixed_params(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> Option<Vec<smelt_hir::TypeId>> {
        if !matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Class { .. })
        ) {
            return None;
        }
        let method_ty = self.class_field_type(receiver_ty, method).ok()?;
        let Some(Type::Function(function)) = self.ctx.krate.types.get(method_ty).cloned() else {
            return None;
        };
        function.rest.is_none().then_some(function.params)
    }

    /// Return whether a member call through a TYPED function-valued field
    /// supplies its class-instance receiver as `this`.
    ///
    /// JavaScript binds `this` from the call: `router.match(m, p)` runs the
    /// stored function with `this === router` even when the field holds a
    /// plain function (Hono's `match: typeof match<..> = match`, whose body
    /// reads `this.buildAllMatchers()` and assigns `this.match`). The erased
    /// call ABI already installs that receiver; a callee that kept its
    /// concrete `Type::Function` did not, so the function observed
    /// `undefined` and every erased read through it came back empty.
    ///
    /// Binding wraps the callable in a closure of the SAME Rust type that
    /// installs the receiver (lazily erased, see the `BindThis` emission) and
    /// forwards its arguments. That forwarding is exact only when every
    /// argument travels by value, so the rule is gated to signatures whose
    /// parameters are scalars, strings, erased values, optionals of those or
    /// callables: a structural parameter can be passed by `&mut` reference
    /// through the call site itself, and a synchronous function only, since
    /// the receiver is installed for the duration of the call and an `async`
    /// body reads it after the call has returned its future. Other signatures
    /// keep the receiver-less call they had.
    ///
    /// The receiver must be a class instance: that is the value whose erased
    /// view keeps the instance's identity and prototype members, which is what
    /// the callee's `this` reads resolve against. The whole-program
    /// `UnobservedReceiverBind` MIR pass removes the bind again from programs
    /// that never read `this`.
    fn typed_field_callee_takes_class_receiver(
        &self,
        dispatch_ty: smelt_hir::TypeId,
        receiver: smelt_hir::ExprId,
        property: &str,
        body: &Body,
    ) -> bool {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(dispatch_ty) else {
            return false;
        };
        if function.is_async || !function.mutable_params.is_empty() {
            return false;
        }
        if !function
            .params
            .iter()
            .all(|param| self.type_is_by_value_call_argument(*param))
        {
            return false;
        }
        let Some(Type::Class { name, .. }) =
            self.ctx.krate.types.get(Self::expr_ty(body, receiver))
        else {
            return false;
        };
        self.class_declares_data_field(*name, property)
    }

    /// Return whether `property` names a DATA field (not a method) of the
    /// generated class `class` or of a class it extends.
    ///
    /// A method is emitted as a Rust method whose receiver is `self`, so a call
    /// to it never needs the dynamic `this` channel — even where the class
    /// also carries a slot of that name for virtual dispatch. Only a field that
    /// STORES a function value (`match: typeof match<..> = match`) runs code
    /// that can only find its receiver through the channel.
    fn class_declares_data_field(&self, class: smelt_hir::Symbol, property: &str) -> bool {
        let mut current = Some(class);
        for _ in 0_u32..64_u32 {
            let Some(class_symbol) = current else {
                return false;
            };
            let Some(item) = self.class_by_symbol(class_symbol) else {
                return false;
            };
            let names_property = |symbol: smelt_hir::Symbol| {
                self.ctx.krate.symbols.get(symbol) == Some(property)
                    || self.ctx.krate.names.get(symbol) == Some(property)
            };
            if item.methods.iter().any(|method| {
                matches!(self.item_ref(*method), Item::Function(function) if names_property(function.name))
            }) || item
                .abstract_methods
                .iter()
                .any(|method| names_property(method.name))
            {
                return false;
            }
            if item.fields.iter().any(|field| names_property(field.name)) {
                return true;
            }
            current = item.base;
        }
        false
    }

    /// Return whether a typed closure parameter of `ty` is always passed by
    /// value, so a forwarding wrapper passes exactly what the call site did.
    fn type_is_by_value_call_argument(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Function(_),
            ) => true,
            Some(Type::Optional(inner)) => self.type_is_by_value_call_argument(*inner),
            _ => false,
        }
    }

    /// Return whether `receiver_ty` is a class still being lowered that
    /// DECLARES `method` itself.
    ///
    /// `resolve_method` answers such a call with the declared return type and
    /// no item (`ItemId(u32::MAX)`), because the class's item is registered
    /// only once all its members are lowered; MIR resolves the call by name
    /// later. When the method has no return annotation that type is still
    /// `Unknown`, which made the pair look exactly like a genuinely dynamic
    /// member call, and `member_call` replaced the whole call with `undefined`:
    /// `add() { this.#insertPath(m, p) }` inside the same class dropped the
    /// call — and every side effect and throw in it (Hono's reg-exp router
    /// `add` never inserted a path or raised `UnsupportedPathError`).
    fn receiver_declares_in_progress_method(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Result<bool, SmeltError> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(receiver_ty).cloned()
        else {
            return Ok(false);
        };
        if self.class_by_symbol(name).is_some() {
            return Ok(false);
        }
        Ok(self
            .in_progress_class_method_return(name, &args, method, span)?
            .is_some())
    }

    /// Lower `receiver[key]()` over a receiver whose member set is known.
    ///
    /// JavaScript reads the property named by `key` and calls it. When the
    /// receiver's type is a class or interface, that set of names is static, so
    /// the call is a CHOICE among known methods and lowers as one — a chain of
    /// `key === "<name>"` tests, each arm the ordinary method call. A dynamic
    /// lookup is never needed and never emitted.
    ///
    /// This is a silent-wrong-value fix, not a missing feature.
    /// `computed_member` answers `Unknown` for such a read, and the arm below it
    /// turns an `Unknown` callee into `undefined`, so
    /// `function pick(req: Req, key: 'json' | 'text'): string { return req[key]() }`
    /// emitted `return String::new()` — the whole body replaced by a default,
    /// with no diagnostic. Hono's `await req[cacheKey]()` (`src/request.ts`) is
    /// that shape.
    ///
    /// Deliberately narrow, and each limit is about not evaluating something
    /// twice rather than about the shape being rare:
    ///
    /// * the receiver and the key must both be plain identifier references, so
    ///   the single lowered expression each produces can be referenced from
    ///   every arm without being evaluated more than once;
    /// * the call must take no arguments, for the same reason — an argument
    ///   would be lowered once and referenced from every arm, and binding it to
    ///   a temporary first is a separate change. `receiver[key]()` is the
    ///   accessor-style shape this form is written in;
    /// * every candidate must answer the same return type, so the chain has one
    ///   type. Candidates that disagree fall through to the existing lowering
    ///   rather than being joined into a union here.
    ///
    /// The last candidate is the chain's `else` with no test of its own: the
    /// source's key type says the key is one of these names, and a final
    /// comparison whose failure has no answer would need a thrown `TypeError`
    /// the surrounding expression position cannot hold.
    pub(in crate::lowering) fn computed_method_call_over_members(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !call.arguments.is_empty() || call.optional || member.optional {
            return Ok(None);
        }
        if !matches!(&member.object, Expression::Identifier(_))
            || !matches!(&member.expression, Expression::Identifier(_))
        {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let mut candidates: Vec<(smelt_hir::Symbol, String)> = Vec::new();
        if let Some(class) = self.class_by_symbol(name).cloned() {
            for method_item in &class.methods {
                let Item::Function(function) = self.item_ref(*method_item) else {
                    continue;
                };
                // A lowered class method carries its receiver as the leading
                // parameter, so a JavaScript-nullary method has exactly one.
                // An interface method SIGNATURE does not (see the arm below):
                // the two shapes count differently and each is checked in its
                // own terms rather than through one shared number.
                if function.params.len() > 1 || function.rest.is_some() {
                    continue;
                }
                let method = function.name;
                let source = self
                    .ctx
                    .krate
                    .symbols
                    .source(method)
                    .unwrap_or_default()
                    .to_owned();
                if source.is_empty() {
                    return Ok(None);
                }
                candidates.push((method, source));
            }
        } else if let Some(interface) = self.find_interface(name).cloned() {
            for signature in &interface.methods {
                if !signature.params.is_empty() || signature.rest.is_some() {
                    continue;
                }
                let source = self
                    .ctx
                    .krate
                    .symbols
                    .source(signature.name)
                    .unwrap_or_default()
                    .to_owned();
                if source.is_empty() {
                    return Ok(None);
                }
                candidates.push((signature.name, source));
            }
        } else {
            return Ok(None);
        }
        if candidates.len() < 2 {
            return Ok(None);
        }
        let member_span = member.span;
        let mut return_ty = None;
        for (method, _) in &candidates {
            let (candidate_ty, _) = self.resolve_method(receiver_ty, *method, member_span)?;
            if matches!(self.ctx.krate.types.get(candidate_ty), Some(Type::Unknown)) {
                return Ok(None);
            }
            match return_ty {
                None => return_ty = Some(candidate_ty),
                Some(existing) if existing == candidate_ty => {}
                Some(_) => return Ok(None),
            }
        }
        let Some(return_ty) = return_ty else {
            return Ok(None);
        };
        let key = self.expression(&member.expression, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        if Self::expr_ty(body, key) != string_ty {
            return Ok(None);
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let call_span = self.span(call.span.start, call.span.end);
        let key_span = self.span(member.expression.span().start, member.expression.span().end);
        let arm = |builder: &mut Self, target: &mut Body, method: smelt_hir::Symbol| {
            let _ = builder;
            target.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver,
                    method,
                    args: Vec::new(),
                },
                ty: return_ty,
                span: call_span,
            })
        };
        let last_method = candidates
            .last()
            .map(|(method, _)| *method)
            .ok_or_else(|| {
                SmeltError::unsupported(call_span, "computed method call has no candidate members")
            })?;
        let mut expr = arm(self, body, last_method);
        for (method, source) in candidates.iter().rev().skip(1) {
            let literal = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(source.clone())),
                ty: string_ty,
                span: key_span,
            });
            let cond = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: smelt_hir::BinOp::JsStrictEq,
                    lhs: key,
                    rhs: literal,
                },
                ty: bool_ty,
                span: key_span,
            });
            let then_expr = arm(self, body, *method);
            expr = body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr,
                    else_expr: expr,
                },
                ty: return_ty,
                span: call_span,
            });
        }
        Ok(Some(expr))
    }

    /// Lower a qualified static method call `Class.staticMethod(args)`.
    ///
    /// Resolves the receiver identifier to a user class and, when it declares a
    /// `staticMethod`, lowers the call as a direct call to the receiver-free
    /// associated function item so codegen emits `Class::staticMethod(args)`.
    /// Returns `None` when the callee is not a `Class.method(..)` static call so
    /// the caller can fall through to the ordinary member-call path.
    ///
    /// A generic static method's own type parameters are instantiated as in
    /// [`Self::instantiate_generic_method_result`].
    pub(in crate::lowering) fn class_static_method_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        // The receiver must be a bare class name not shadowed by a local, or
        // `this` in a static field initializer (the class itself).
        let Some(object) = self.static_method_receiver_class(&member.object) else {
            return Ok(None);
        };
        let Some(class_item) = self.classes.item(&object) else {
            return Ok(None);
        };
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(None);
        };
        let Some(static_item) = self.resolve_class_static_method(
            &self.static_methods_through_chain(class),
            member.property.name.as_str(),
        ) else {
            return Ok(None);
        };
        let Item::Function(function) = self.item_ref(static_item) else {
            return Ok(None);
        };
        let return_ty = function.return_ty;
        let rest = function.rest;
        let params = function
            .params
            .iter()
            .map(|param| param.ty)
            .collect::<Vec<_>>();
        let method_type_params = self.callee_own_type_params(static_item);
        let span = self.span(call.span.start, call.span.end);
        // Arguments go through the shared spread rules (a tuple spread
        // expands, a trailing list spread binds the fixed parameters), and a
        // rest parameter collects the tail into its list. Each argument is
        // otherwise lowered as-is; the Rust emitter coerces it to the
        // associated function's declared parameter type at the call site, the
        // same way a direct free-function call is adapted.
        let expanded = self.expanded_call_arguments(&call.arguments, body)?;
        let expanded = if rest.is_none() {
            self.distribute_trailing_list_spread(expanded, &params, body)?
        } else {
            expanded
        };
        let spread_positions = expanded
            .iter()
            .map(|arg| matches!(arg, CallArg::UnexpandedSpread(_)))
            .collect::<Vec<_>>();
        let mut args = Vec::with_capacity(expanded.len());
        for argument in expanded {
            args.push(self.lower_call_arg(argument, None, body)?);
        }
        if let Some(rest_index) = rest
            && rest_index < params.len()
        {
            let span = self.span(call.span.start, call.span.end);
            args =
                self.pack_rest_arguments(args, &spread_positions, rest_index, &params, span, body);
        }
        let return_ty = if rest.is_none() && !method_type_params.is_empty() {
            self.instantiated_call_return(&method_type_params, &params, &args, return_ty, call, body)?
        } else {
            self.instantiate_generic_method_result(&method_type_params, return_ty)
        };
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(static_item),
            ty: self.item_expr_type(static_item, span)?,
            span,
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span,
        })))
    }

    /// Return whether a static-member expression names `Class.staticMethod` on a
    /// bare, unshadowed class identifier.
    ///
    /// This mirrors the preconditions of [`Self::class_static_method_call`] so
    /// other member-call dispatchers can defer such calls to it instead of
    /// lowering them as callable-field reads (which would erase the concrete
    /// associated function to the runtime `SmeltUnknown` call ABI). It is a
    /// read-only check: the receiver identifier must resolve to a declared class
    /// (not a local) whose `static_methods` include the accessed property.
    pub(in crate::lowering) fn static_member_is_class_static_method(
        &self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> bool {
        let Some(object) = self.static_method_receiver_class(&member.object) else {
            return false;
        };
        let Some(class_item) = self.classes.item(&object) else {
            return false;
        };
        let Item::Class(class) = self.item_ref(class_item) else {
            return false;
        };
        self.resolve_class_static_method(
            &self.static_methods_through_chain(class),
            member.property.name.as_str(),
        )
        .is_some()
    }

    /// The class a `Class.staticMethod(..)` receiver names: a bare class
    /// identifier no local shadows, or `this` inside a static field
    /// initializer (`ModuleBuilder::static_initializer_this`).
    fn static_method_receiver_class(&self, object: &Expression<'_>) -> Option<String> {
        match object {
            Expression::Identifier(object) => {
                (!self.scope.is_bound(object.name.as_str())).then(|| object.name.to_string())
            }
            Expression::ThisExpression(_) => self.static_initializer_this(),
            _ => None,
        }
    }

    /// The static methods `class` declares, then those of each ancestor: a
    /// subclass inherits its base's statics.
    fn static_methods_through_chain(&self, class: &smelt_hir::Class) -> Vec<smelt_hir::ItemId> {
        let mut methods = class.static_methods.clone();
        let mut base = class.base;
        for _ in 0..64_u32 {
            let Some(declared) = base.and_then(|name| self.class_by_symbol(name)) else {
                break;
            };
            methods.extend(declared.static_methods.iter().copied());
            base = declared.base;
        }
        methods
    }

    /// Return the static method item on a class matching `method_name`, if any.
    fn resolve_class_static_method(
        &self,
        static_methods: &[smelt_hir::ItemId],
        method_name: &str,
    ) -> Option<smelt_hir::ItemId> {
        static_methods.iter().copied().find(|item| {
            matches!(
                self.item_ref(*item),
                Item::Function(function)
                    if self.ctx.krate.symbols.get(function.name) == Some(method_name)
            )
        })
    }

    /// Lower calls where an object property itself is a callable value.
    ///
    /// This covers TypeScript patterns like `args.callback(value)`, where the
    /// property is typed as a function or as a nullishable union containing a
    /// function. Class methods still use the normal method-call path.
    pub(in crate::lowering) fn callable_static_member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        // `Class.staticMethod(...)` is a receiver-free associated-function call,
        // resolved authoritatively by `class_static_method_call` (dispatched
        // right after this helper). It must not be lowered here as a callable
        // *field* read on the class value: the callee-field lowering below would
        // route it through the erased `SmeltUnknown` call ABI instead of the
        // concrete `Class::staticMethod` associated function.
        if self.static_member_is_class_static_method(member) {
            return Ok(None);
        }
        if matches!(member.property.name.as_str(), "call" | "apply")
            && let Some(expr) = self.erased_function_method_call(member, call, body)?
        {
            return Ok(Some(expr));
        }
        if member.property.name == "call" {
            let callable = self.expression(&member.object, body)?;
            let callable_ty = Self::expr_ty(body, callable);
            if let Some(function_ty) = self.function_member_type(callable_ty)
                && let Some(Type::Function(function)) =
                    self.ctx.krate.types.get(function_ty).cloned()
            {
                // `fn.call(thisArg, a, b)` binds `this` from the LEADING operand
                // and passes only the rest positionally. Whether that operand is
                // also a positional ARGUMENT depends on the callee's own ABI, and
                // the call's arity settles it -- the same way a same-named
                // variadic sibling's rest slot is settled by arity rather than by
                // the slot's type.
                //
                // A plain function or closure has no receiver slot: Smelt erases
                // the JavaScript `this` binding for it, so a source
                // `function (this: any, arg)` lowers to a ONE-parameter closure
                // and the leading operand must be dropped, exactly as
                // `callback_apply_method_to_body_expr` drops it for `apply`.
                // Passing it through bound the callee's first parameter to the
                // `this` object: es-toolkit's `memoize` calls `fn.call(this, arg)`,
                // so `memoize((x: number) => x + 10)(5)` added 10 to the `this`
                // object instead of to 5. `flow` and `overArgs` do the same.
                //
                // A class METHOD does have a receiver slot -- it is lowered
                // receiver-first, `(Example, String)` for `greet(name: string)` --
                // so there the leading operand IS the first parameter and dropping
                // it shifts every argument left. The decorated-member parity
                // fixture reaches exactly that shape once the specializer
                // materializes `value.call(this, name)` against the concrete
                // method.
                //
                // A fixed-arity callee whose parameter count already accounts for
                // every operand is the receiver-first case; anything else (an
                // arity that only fits without the receiver, or a variadic callee,
                // which declares no receiver slot to fill) drops it.
                let receiver_is_positional =
                    function.rest.is_none() && function.params.len() == call.arguments.len();
                let span = self.span(call.span.start, call.span.end);
                // A receiver-less callee still OBSERVES the leading operand as
                // its `this`: it is dropped from the positional list, then bound
                // as the receiver installed for this one call (`fn.call(this, x)`
                // in es-toolkit's `memoize` resolver and `flow`). A receiver-first
                // class method needs no bind: its `this` IS its first parameter.
                let callable = if receiver_is_positional {
                    callable
                } else {
                    match call.arguments.first() {
                        Some(argument) => {
                            let this_arg = self.argument(argument, body)?;
                            self.bind_this_receiver(callable, this_arg, body, span)
                        }
                        None => callable,
                    }
                };
                let args = call
                    .arguments
                    .iter()
                    .skip(usize::from(!receiver_is_positional))
                    .map(|argument| self.argument(argument, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let spread_positions = call
                    .arguments
                    .iter()
                    .skip(usize::from(!receiver_is_positional))
                    .map(Argument::is_spread)
                    .collect::<Vec<_>>();
                // Binding `this` erases the callee, and an erased call takes one
                // operand per argument.
                let callable_is_typed = matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, callable)),
                    Some(Type::Function(_))
                );
                let args = if callable_is_typed {
                    self.pack_value_call_arguments(&function, args, &spread_positions, span, body)
                } else {
                    args
                };
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee: callable,
                        args,
                    },
                    ty: function.return_ty,
                    span,
                })));
            }
        }
        if member.property.name == "apply" {
            // `fn.apply(thisArg, argsArray)` is the sibling of the `call` arm
            // above: the LEADING operand binds `this` and the TRAILING operand is
            // an array whose elements become the positional arguments.
            //
            // Without this arm, `apply` on a value whose type is a concrete
            // `Type::Function` fell through to `static_member_no_absent_fallback`,
            // which reads `apply` as an ordinary (absent) field of the function
            // and answers a literal `null` — so `return func.apply(this, args)`
            // emitted `return SmeltUnknown::Null;` and the call vanished with no
            // diagnostic. Only the *erased* (`SmeltUnknown`) receiver reached the
            // runtime `smelt_function_method(.., "apply")` path, so the same
            // source line behaved differently depending on whether the callee had
            // kept its static type.
            //
            // The `this` operand is dropped rather than passed positionally,
            // exactly as `callback_apply_method_to_body_expr` (which this arm
            // delegates to, so the two spellings cannot diverge) already does for
            // the closure-body form: Smelt erases the JavaScript `this` binding
            // for a plain function, so a source `function (this: any, ...rest)`
            // lowers to a closure with no receiver slot. The arity test the `call`
            // arm uses to detect a receiver-first class method is unavailable here
            // — `apply`'s argument list is one runtime array whose length is not
            // statically known — so `apply` keeps the uniform drop.
            let callable = self.expression(&member.object, body)?;
            let callable_ty = Self::expr_ty(body, callable);
            // `function_member_type` resolves every statically callable receiver
            // shape — a bare function, a nullishable or union callee, and a
            // callable interface/object. All of them get the same typed `apply`,
            // so the argument list travels as arguments rather than through the
            // runtime member-lookup fallback. es-toolkit's `throttle` calls
            // `.apply` on a callable-object receiver and needs exactly that.
            //
            // No argument count is supplied to the overload picker: `apply`'s own
            // arity is `(thisArg, argsArray)` and says nothing about how many
            // arguments the callee will receive — that is the runtime length of
            // the array, which is why the call is a spread in the first place.
            if let Some(function_ty) = self.function_member_type(callable_ty)
                && let Some(Type::Function(function)) =
                    self.ctx.krate.types.get(function_ty).cloned()
            {
                let span = self.span(call.span.start, call.span.end);
                // A class METHOD is lowered receiver-first, so its `this` operand
                // IS its first parameter and would have to be PREPENDED to the
                // spread list rather than dropped -- something the packed-argument
                // ABI cannot express. Dropping it there would shift every argument
                // left and silently compute the wrong thing, which is the exact
                // failure class this whole arm exists to remove, so the shape is
                // reported instead. No corpus reaches it today; the diagnostic is
                // here so that when one does it fails loudly rather than quietly.
                if self.static_member_is_concrete_class_method(callable, member, body) {
                    return Err(SmeltError::unsupported(
                        span,
                        "apply on a receiver-first class method is not lowered yet",
                    ));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|argument| self.argument(argument, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return self
                    .callback_apply_method_to_body_expr(
                        callable,
                        callable_ty,
                        &args,
                        function.return_ty,
                        body,
                        span,
                    )
                    .map(Some);
            }
        }
        let Ok(callee) = self.static_member_no_absent_fallback(member, body) else {
            return Ok(None);
        };
        if self.static_member_is_concrete_class_method(callee, member, body) {
            return Ok(None);
        }
        let callee_ty = Self::expr_ty(body, callee);
        // A method no model claimed, read off a closed prototype (a string, an
        // array, a number), would only throw at run time; see
        // `closed_prototype_missing`.
        if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown)
            && let Some(Expr {
                kind: ExprKind::Field { receiver, .. },
                ..
            }) = body.expr_at(callee)
        {
            let receiver_ty = Self::expr_ty(body, *receiver);
            let span = self.span(call.span.start, call.span.end);
            if let Some(blocked) = self.closed_prototype_call_blocked(
                receiver_ty,
                member.property.name.as_str(),
                span,
                body,
            )? {
                return Ok(Some(blocked));
            }
        }
        let has_spread = Self::call_has_spread_arguments(call);
        // A spread call dispatches through the runtime `SmeltUnknown` call ABI
        // whenever *either* the callee field value or the receiving object is
        // erased to `SmeltUnknown` at codegen. That ABI flattens the whole
        // argument list, so the spread must be packed into a single runtime
        // list operand (`ClosureCallSpread`) instead of letting `self.argument`
        // collapse `...rest` into a bare positional operand or letting the
        // concrete-rest branch below build a typed `[fixed.., rest_list]`
        // expansion (which the dynamic ABI would then re-wrap into a nested
        // array).
        //
        // Two independent signals force the dynamic ABI:
        //
        //   * the *callee field's* type is itself a dynamic call surface
        //     (`unknown`/type-parameter/union/erased callable object), or
        //   * the *receiver's* type erases to `SmeltUnknown` at codegen — e.g.
        //     a generic interface/type-alias instantiation such as
        //     `Funnel<Args>` whose member read therefore returns a runtime
        //     `SmeltUnknown` value regardless of the field's declared shape.
        let callee_dispatches_dynamically = self.type_is_dynamic_call_surface(callee_ty);
        let receiver_dispatches_dynamically =
            self.static_member_receiver_dispatches_dynamically(member, body);
        if has_spread && (callee_dispatches_dynamically || receiver_dispatches_dynamically) {
            let unknown = self.erase_unless_unknown(callee_ty, ErasureReason::DynamicOperator);
            // Re-type the callee operand as `SmeltUnknown` whenever it would
            // otherwise be emitted as a concrete Rust function: an erased
            // callable object, or a field read whose *declared* type is a
            // concrete function but whose *receiver* erases to `SmeltUnknown`
            // (so the field read actually yields a runtime `SmeltUnknown`).
            // The `ClosureCallSpread` emitter dispatches a `SmeltUnknown`
            // callee through the runtime call ABI that consumes the packed
            // argument list, whereas a `Type::Function` callee would be called
            // as a typed Rust closure and reject the flattened `Vec` operand.
            // Type-parameter and union callees already carry a non-function
            // type, so they route through the runtime ABI without a cast.
            let callee_needs_unknown_cast = self.is_callable_object_erased_class(callee_ty)
                || matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Function(_)));
            let callable = if callee_needs_unknown_cast {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: callee },
                    ty: unknown,
                    span: self.span(member.span.start, member.span.end),
                })
            } else {
                callee
            };
            let packed = self.packed_spread_call_arguments(unknown, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee: callable,
                    args: packed,
                },
                ty: unknown,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // A spread call whose callee is a *concrete* function with a rest
        // parameter keeps its typed `[fixed.., rest_list]` expansion: codegen
        // dispatches it through the concrete-function path whose Rust rest
        // parameter absorbs the trailing list. `self.argument` would otherwise
        // collapse `...rest` into a single bare positional operand and lose the
        // spread, so route it through the shared spread expansion helper.
        if has_spread
            && let Some(function_ty) = self.function_member_type(callee_ty)
            && let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
            && let Some(rest) = self.list_rest_param(&function)
        {
            let callable = if callee_ty == function_ty {
                callee
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: callee },
                    ty: function_ty,
                    span: self.span(member.span.start, member.span.end),
                })
            };
            let args = self.spread_closure_call_arguments(&function, Some(rest), call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee: callable,
                    args,
                },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // A tuple spread distributes positionally here too: this is the path a
        // FUNCTION-TYPED FIELD callee takes (`router.add(...routes[i])` in
        // hono's smart router), and treating the spread as one argument put the
        // whole tuple in parameter 0 and let the emitter pad the rest with
        // defaults.
        //
        // An overloaded callable-interface field (`app.use('/p', handler)`
        // against `use: MiddlewareHandlerInterface`) selects the overload from
        // the call's argument count and probed argument types, exactly as a
        // callable-interface VALUE callee does; the first declared signature
        // is only the answer when there is no call site to select against.
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        let expanded = self.expanded_call_arguments(&call.arguments, body)?;
        let arg_count = expanded.len();
        // A spread expanded into several positions no longer lines up with the
        // per-source-argument probes, so selection falls back to arity alone.
        let probed_arg_tys = if arg_count == call.arguments.len() {
            probed_arg_tys
        } else {
            Vec::new()
        };
        // A trailing LIST spread has no length to select an overload by, so the
        // signature is chosen without an arity, and the list is distributed
        // over its fixed parameters as for every other fixed-arity callee
        // (`ctx.respond(...parts)`). Without this the list was parameter 0.
        let (expanded, arg_count) = if matches!(expanded.last(), Some(CallArg::UnexpandedSpread(_)))
            && self.ctx.krate.types.get(callee_ty) != Some(&Type::Unknown)
            && let Some(selected) = self.function_member_type_for_args(callee_ty, None, &[])
            && let Some(Type::Function(selected)) = self.ctx.krate.types.get(selected).cloned()
            && selected.rest.is_none()
        {
            let distributed =
                self.distribute_trailing_list_spread(expanded, &selected.params, body)?;
            let count = distributed.len();
            (distributed, count)
        } else {
            (expanded, arg_count)
        };
        let mut args = Vec::with_capacity(expanded.len());
        for argument in expanded {
            args.push(self.lower_call_arg(argument, None, body)?);
        }
        // A prototype method read off an ERASED value (`anyValue.at(-1)`) is
        // resolved by the runtime on the value it meets (see
        // `dispatch_builtin_call`), and a value with no such method must throw
        // JavaScript's `TypeError: x.at is not a function` when CALLED.
        // Synthesizing a typed function signature from the arguments would
        // adapt the read into a typed callback, whose adapter has no callee to
        // call for a non-callable value and answers a default instead. The
        // callee therefore stays erased, and the call takes the erased call ABI,
        // which invokes whatever callable the read produced or throws.
        if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown)
            && smelt_stdlib::is_erased_prototype_call(
                member.property.name.as_str(),
                call.arguments.len(),
            )
            && self.receiver_is_statically_erased(&member.object, body)
        {
            let span = self.span(call.span.start, call.span.end);
            let callee = self.bind_member_call_receiver(callee, member, body, span);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: callee_ty,
                span,
            })));
        }
        let (function_ty, function) = if let Some(function_ty) =
            self.function_member_type_for_args(callee_ty, Some(arg_count), &probed_arg_tys)
        {
            let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
            else {
                return Ok(None);
            };
            (function_ty, function)
        } else if self.is_nullishable_type(callee_ty) || self.type_contains_unknown(callee_ty) {
            let unknown = if self.type_contains_unknown(callee_ty) {
                // not a new erasure: the callee already carries an erased type
                self.unknown_type(NotNewErasure::ErasedUpstream)
            } else {
                self.erase(ErasureReason::ByName)
            };
            let params = args
                .iter()
                .map(|arg| Self::expr_ty(body, *arg))
                .collect::<Vec<_>>();
            let function = FunctionType {
                params,
                rest: None,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty: unknown,
                is_async: false,
                may_throw: false,
            };
            let function_ty = self
                .ctx
                .krate
                .types
                .intern(Type::Function(function.clone()));
            (function_ty, function)
        } else {
            return Ok(None);
        };
        // The receiver is bound onto the member READ, before the call's own
        // coercion re-types it. A member read whose declared type is `unknown`
        // is coerced to a synthesized `Type::Function` built from the argument
        // types, so asking the coerced operand what ABI it uses always answered
        // "a concrete function" and no ordinary method call ever carried a
        // receiver. The bind keeps the read's own type, so the coercion below is
        // unaffected by it.
        let span = self.span(call.span.start, call.span.end);
        let callee = self.bind_member_call_receiver(callee, member, body, span);
        let callable = if callee_ty == function_ty
            || self.callable_interface_known_only_by_predeclaration(callee_ty)
        {
            // A callable interface this module only knows through its
            // predeclared call signatures (an import cycle lowers this module
            // first) is called AS the record: the emitter invokes its
            // `__smelt_call` slot with the slot's own signature, which the
            // interface's full lowering makes more precise than the
            // predeclaration can be (see
            // `ModuleBuilder::predeclare_interface_call_signatures`).
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(member.span.start, member.span.end),
            })
        };
        if function
            .params
            .iter()
            .any(|param| self.concrete_type_requires_never_value(*param))
            && !Self::call_has_spread_arguments(call)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "calls through function types with never parameters are not lowered",
            ));
        }
        let call_span = self.span(call.span.start, call.span.end);
        let args = self.pack_value_call_arguments(&function, args, &[], call_span, body);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callable,
                args,
            },
            ty: function.return_ty,
            span,
        })))
    }

    /// Lower `receiver.#field(args)` where `#field` holds a FUNCTION.
    ///
    /// A private name can denote either a method or a field, and a field may
    /// hold a closure: Hono's `HonoRequest` declares
    /// `#cachedBody = (key: keyof Body) => ...` and calls `this.#cachedBody('text')`
    /// from six methods. That is a CALL OF A FIELD, not a method call, and the
    /// public spelling has always been lowered as one (see
    /// [`Self::callable_static_member_call`]); the private spelling had no such
    /// path, so it lowered as `ExprKind::Method` and MIR then failed with
    /// `class method '#cached_body' is not resolvable` — there is no method of
    /// that name to resolve. The blocker was masked while `context.ts` and
    /// `request.ts` still had earlier ones.
    ///
    /// Returns `None` when the private member is not a function-typed field, so
    /// a genuine private METHOD keeps the receiver-first method lowering.
    ///
    /// The receiver is restricted to an identifier or `this` — the only shapes
    /// whose lowering has no side effect — because deciding between the two
    /// lowerings requires reading the receiver's type, and the method fallback
    /// lowers the receiver again.
    pub(in crate::lowering) fn private_callable_field_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::PrivateFieldExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if call.optional || member.optional {
            return Ok(None);
        }
        if !matches!(
            &member.object,
            Expression::Identifier(_) | Expression::ThisExpression(_)
        ) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let field = self.intern_private_name(member.field.name.as_str());
        // Only a declared FIELD takes this path. A private METHOD also answers
        // a function type through `class_field_type` (as a bound method value),
        // and routing it here would replace its direct receiver-first call with
        // the erased callable-value ABI. A private name is either a method or a
        // field, never both, so the field's existence is the whole test — a
        // field that is CALLED and is not callable is not valid TypeScript.
        if !self.class_declares_field(name, field) {
            return Ok(None);
        }
        let Ok(callee) = self.private_field_member(
            &member.object,
            member.field.name.as_str(),
            member.span,
            body,
        ) else {
            return Ok(None);
        };
        let callee_ty = Self::expr_ty(body, callee);
        let typed = self.function_member_type(callee_ty).map(|function_ty| {
            let function = match self.ctx.krate.types.get(function_ty) {
                Some(Type::Function(function)) => Some(function.clone()),
                _ => None,
            };
            (function_ty, function)
        });
        let args = self.private_callable_field_arguments(call, typed.as_ref(), body)?;
        let (function_ty, function) = match typed {
            Some((function_ty, Some(function))) => (function_ty, function),
            Some((_, None)) => return Ok(None),
            // An ERASED callable field: Hono's `#renderer: Renderer` resolves
            // through a conditional type (`ContextRenderer extends Function ?
            // ...`) that carries no callable shape, so the read is `unknown`.
            // The call is then the dynamic callable boundary, and the ABI is
            // synthesized from the argument types exactly as the public
            // callable-field path does for the same shape.
            None if self.is_nullishable_type(callee_ty)
                || self.type_contains_unknown(callee_ty) =>
            {
                let unknown = if self.type_contains_unknown(callee_ty) {
                    // not a new erasure: the callee already carries an erased type
                    self.unknown_type(NotNewErasure::ErasedUpstream)
                } else {
                    self.erase(ErasureReason::ByName)
                };
                let params = args
                    .iter()
                    .map(|arg| Self::expr_ty(body, *arg))
                    .collect::<Vec<_>>();
                let function = FunctionType {
                    params,
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: unknown,
                    is_async: false,
                    may_throw: false,
                };
                let function_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Function(function.clone()));
                (function_ty, function)
            }
            None => return Ok(None),
        };
        // `#renderer: Renderer | undefined` is declared optional and assigned
        // through `??=` before the call, so the read's type is the optional (or
        // union) that carries the callable. The call site asserts the callable
        // member, exactly as the public callable-field path does.
        let callable = if callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(member.span.start, member.span.end),
            })
        };
        let call_span = self.span(call.span.start, call.span.end);
        let args = self.pack_value_call_arguments(&function, args, &[], call_span, body);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callable,
                args,
            },
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Arguments of a private callable-field call.
    ///
    /// A typed fixed-arity callee takes its arguments through the spread
    /// choke point (`lowering::spread_arguments`), so a trailing list spread
    /// is distributed over its parameters; an erased callee synthesizes its
    /// ABI from the argument types, so each source argument is one argument.
    fn private_callable_field_arguments(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        typed: Option<&(smelt_hir::TypeId, Option<FunctionType>)>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        if let Some((_, Some(function))) = typed
            && function.rest.is_none()
        {
            let expanded = self.expanded_call_arguments(&call.arguments, body)?;
            let expanded =
                self.distribute_trailing_list_spread(expanded, &function.params, body)?;
            return expanded
                .into_iter()
                .map(|argument| self.lower_call_arg(argument, None, body))
                .collect();
        }
        call.arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect()
    }

    /// Whether class `class_name` declares a field named `field`.
    ///
    /// Both stores are consulted: the sidecar registered while the class is
    /// still being lowered (a private field is called from the same class's
    /// methods, so its class item does not exist yet) and the finished class
    /// item for a class from another module.
    fn class_declares_field(
        &self,
        class_name: smelt_hir::Symbol,
        field: smelt_hir::Symbol,
    ) -> bool {
        let field_ty = source_spelling(&self.ctx.krate, class_name)
            .map(str::to_owned)
            .and_then(|class_text| self.classes.fields(&class_text).cloned())
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|candidate| candidate.name == field)
                    .map(|candidate| candidate.ty)
            })
            .or_else(|| {
                self.class_by_symbol(class_name).and_then(|class| {
                    class
                        .fields
                        .iter()
                        .find(|candidate| candidate.name == field)
                        .map(|candidate| candidate.ty)
                })
            });
        field_ty.is_some()
    }

    /// Bind `object` as the receiver of an `object.method(..)` callable field read.
    ///
    /// In JavaScript the receiver of a member call is the object the member was
    /// read FROM, so a callable stored as an ordinary property sees that object
    /// as `this`. Without this the receiver was simply never supplied and any
    /// `this` in the callee body answered the dynamic channel's default
    /// (`undefined`) -- es-toolkit's `ary`/`unary`/`spread` specs all store a
    /// wrapper on an object and call it as a method precisely to observe that.
    ///
    /// Concrete class methods never reach here: `callable_static_member_call`
    /// defers them to `class_static_method_call`, and they are lowered
    /// receiver-first with `this` as a real bound parameter, so binding the
    /// dynamic channel as well would install the receiver twice.
    ///
    /// `callee` is the lowered member read: its shape decides whether there is
    /// a receiver at all, its type decides whether the callee can observe one,
    /// and the bind is returned in its place so the call's own coercion applies
    /// on top of the bound value rather than underneath it.
    fn bind_member_call_receiver(
        &mut self,
        callee: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        let Some(expr) = body.expr_at(callee) else {
            return callee;
        };
        let (ExprKind::Field { receiver, .. } | ExprKind::OptionalField { receiver, .. }) =
            expr.kind
        else {
            return callee;
        };
        // Only a callee that is invoked through the ERASED call ABI takes a
        // receiver here. A callee that kept a concrete `Type::Function` is
        // invoked as a typed Rust closure whose argument passing is part of its
        // signature -- `&mut` structural parameters are forwarded by reference
        // through the call site itself -- so interposing a bound wrapper would
        // change how the callee is CALLED, not just what `this` resolves to,
        // and silently drop caller-visible mutations. `.call`, `.apply` and
        // `.bind` are unaffected by this restriction: there the source names the
        // receiver explicitly, so honoring it is not optional and the wrapper is
        // built for that callee's own signature.
        let callee_ty = Self::expr_ty(body, callee);
        // A dynamic call surface is asked about FIRST, on the read's own type: a
        // member read typed `unknown` (or a type parameter, or a union) is the
        // ordinary shape of a callable stored as a property, and it is invoked
        // through the runtime call ABI whatever concrete signature the call site
        // later coerces it to.
        let dispatch_ty = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => callee_ty,
            // A nullishable or callable-object read resolves to the function
            // type it carries, which is what says how the callee is invoked.
            _ => self.function_member_type(callee_ty).unwrap_or(callee_ty),
        };
        let callee_uses_erased_call_abi = match self.ctx.krate.types.get(dispatch_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
            // The variadic `(...args: unknown[]) => unknown` spelling is the one
            // `Type::Function` that codegen renders as the erased
            // `SmeltErasedFunction` struct rather than a typed Rust closure, so
            // it takes a receiver the same way an `unknown` callee does. Every
            // other `Type::Function` keeps its concrete parameter list, which is
            // what makes its arguments (`&mut` structural parameters included)
            // travel by the call site's own ABI.
            Some(Type::Function(function)) => {
                !function.is_async
                    && function.rest == Some(0)
                    && matches!(function.params.as_slice(), [param]
                    if matches!(
                        self.ctx.krate.types.get(*param),
                        Some(Type::List(item))
                            if matches!(
                                self.ctx.krate.types.get(*item),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
                            )
                    ))
            }
            _ => false,
        };
        if !callee_uses_erased_call_abi {
            if self.typed_field_callee_takes_class_receiver(
                dispatch_ty,
                receiver,
                member.property.name.as_str(),
                body,
            ) {
                return self.bind_this_receiver(callee, receiver, body, span);
            }
            return callee;
        }
        // A read whose DECLARED type is a function but whose receiver erases to
        // `SmeltUnknown` produces a runtime `SmeltUnknown` value, not the owned
        // erased-function struct that hosts the typed bind, so there is nothing
        // to hang the binding on. A read already typed as a dynamic call
        // surface has no such mismatch: it erases either way and the runtime
        // `smelt_bind_this` handles both a function and a callable object.
        if !matches!(
            self.ctx.krate.types.get(callee_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.static_member_receiver_dispatches_dynamically(member, body)
        {
            return callee;
        }
        self.bind_this_receiver(callee, receiver, body, span)
    }

    /// `f.call(thisArg, ..)` / `f.apply(thisArg, args)` on an ERASED `f`
    /// (`unknown`, or a local of `unknown | undefined`), as one
    /// [`ExprKind::ErasedFunctionMethodCall`]; `None` for any other receiver,
    /// and for a spread argument list, which the member-call paths below
    /// handle.
    ///
    /// The member is resolved at run time, as JavaScript resolves it: a
    /// function runs with `thisArg` as its `this`, an object with its own
    /// `call` runs that. The typed `call` arm cannot take an erased callee:
    /// with no arity to go by it bound the leading operand as an argument,
    /// so `typeof value === "function" ? value.call(object) : value`
    /// (es-toolkit's `result`) called `value` with `object` as its first
    /// parameter and no `this`.
    fn erased_function_method_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if Self::call_has_spread_arguments(call) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        // A local of `unknown | undefined` read at its narrowed `unknown` is
        // taken as stored: the operation erases it anyway.
        let receiver = self.stored_erased_callee(receiver, body);
        let receiver_ty = Self::expr_ty(body, receiver);
        let erased = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Unknown) => true,
            Some(Type::Optional(inner)) => {
                matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
                    && matches!(
                        body.expr_at(receiver).map(|expr| &expr.kind),
                        Some(ExprKind::Local(_))
                    )
            }
            _ => false,
        };
        if !erased {
            return Ok(None);
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let method = self.intern_source_name(member.property.name.as_str());
        // not a new erasure: the receiver is already erased, and so is what
        // calling it answers.
        let ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ErasedFunctionMethodCall {
                receiver,
                method,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return true when a member access names an actual lowered class method.
    pub(in crate::lowering) fn static_member_is_concrete_class_method(
        &mut self,
        callee: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &Body,
    ) -> bool {
        let Some(expr) = body.expr_at(callee) else {
            return false;
        };
        let (ExprKind::Field { receiver, .. } | ExprKind::OptionalField { receiver, .. }) =
            expr.kind
        else {
            return false;
        };
        let receiver_ty = self.optional_receiver_inner_type(Self::expr_ty(body, receiver));
        let method = self.intern_source_name(member.property.name.as_str());
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, callee)),
            Some(Type::Function(_))
        ) && self.receiver_has_callable_storage_field(receiver_ty, method)
        {
            return false;
        }
        // An `ItemId` answers this question when there is one, but its ABSENCE
        // does not answer it: `resolve_method` returns `ItemId(u32::MAX)` for
        // several receivers that do declare the method perfectly concretely, and
        // for each of them the caller would then lower the call as a callable
        // *field* read — routing it through the erased `SmeltUnknown` call ABI
        // (the receiver converted with `into_smelt_unknown`, the method looked
        // up as a dynamic object field and invoked dynamically) even though the
        // emitter can dispatch it directly.
        //
        // Two receiver kinds reach that state, and the failure is worse for the
        // second:
        //
        //   * a UNION has no *single* method item by design — dispatch happens
        //     per tagged-union arm rather than through one function
        //     (`union_method_expr`), so every arm declaring the method is what
        //     makes it concrete;
        //   * a class whose own body is STILL BEING LOWERED has no registered
        //     class item yet, so `class_by_symbol` misses and `resolve_method`
        //     falls to `in_progress_class_method_return`, which types the call
        //     but has no item to name. That is every `this.sibling(..)` call
        //     written inside a class method body. Reading it as a callable field
        //     did not merely erase the call, it LOST it: the class has no such
        //     struct field, so the emitter answered the read with a method-value
        //     stub (`Rc::new(|_| Ok(SmeltUnknown::Null))`), the call returned
        //     `null`, and the method body never ran — with no diagnostic, in a
        //     shape as ordinary as a class calling its own helper.
        //
        // So the item id is consulted first and the declaration second: whether
        // the receiver DECLARES the method is the question being asked, and
        // `receiver_declares_method` answers it for both kinds from the
        // registries that are populated before any method body is lowered. The
        // genuine callable-storage fields that must stay closure calls are
        // already excluded by `receiver_has_callable_storage_field` above.
        if self
            .resolve_method(receiver_ty, method, member.span)
            .is_ok_and(|(_, item)| item.0 != u32::MAX)
        {
            return true;
        }
        self.receiver_declares_method(receiver_ty, method)
    }

    /// Return true when a class or inherited base explicitly stores a callable
    /// field for the member. These fields model virtual dispatch slots and
    /// must be invoked as closures instead of direct class methods.
    pub(in crate::lowering) fn receiver_has_callable_storage_field(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> bool {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(receiver_ty).cloned()
        else {
            return false;
        };
        let class_name = source_spelling(&self.ctx.krate, name).map(str::to_owned);
        let is_callable_field = |fields: &[smelt_hir::Field]| {
            fields
                .iter()
                .find(|item| item.name == field)
                .is_some_and(|item| {
                    matches!(self.ctx.krate.types.get(item.ty), Some(Type::Function(_)))
                })
        };
        if class_name
            .as_deref()
            .and_then(|class_name| self.classes.fields(class_name))
            .is_some_and(|fields| is_callable_field(fields))
        {
            return true;
        }
        // `self.classes` only records fields for classes lowered in THIS
        // module. A class imported from another module is known here only by
        // its lowered HIR item, which carries the same virtual-dispatch slots
        // (abstract methods become callable fields, see
        // `virtual_method_field_names`). Without this fallback an abstract
        // method called on an imported abstract base lowered as a direct
        // method call that MIR cannot resolve (date-fns `setter.set(..)` on
        // `Setter` from `parse/_lib/Setter.ts`).
        if self
            .class_by_symbol(name)
            .is_some_and(|class| is_callable_field(&class.fields))
        {
            return true;
        }
        let Some((base, base_args)) = self
            .class_by_symbol(name)
            .and_then(|class| class.base.map(|base| (base, class.base_args.clone())))
            // Symbol-keyed, not name-keyed: see `class_field_type`. A renamed
            // class shares its source spelling with the class that displaced it,
            // so a by-name base fallback can answer with the receiver itself.
            .or_else(|| self.classes.base_of_symbol(name).cloned())
        else {
            return false;
        };
        let substitutions = self
            .type_argument_substitution(
                &self
                    .class_by_symbol(name)
                    .map(|class| class.type_params.clone())
                    .unwrap_or_default(),
                &args,
                self.span(0, 0),
            )
            .ok();
        let base_args = if let Some(substitutions) = substitutions {
            base_args
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &substitutions))
                .collect()
        } else {
            base_args
        };
        let base_ty = self.ctx.krate.types.intern(Type::Class {
            name: base,
            args: base_args,
        });
        self.receiver_has_callable_storage_field(base_ty, field)
    }
}
