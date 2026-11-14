//! Call-expression lowering hub: the order in which a call's decisions are taken,
//! and the builtin registry that routes a call to its standard-library family.

use crate::SmeltError;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::{ModuleBuilder, stdlib_dispatch};
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Type, TypeParamDef};
use smelt_stdlib::{DiagnosticCategory, RuleId};
use std::collections::HashMap;

/// Function pointer shape for builtin call lowering registry entries.
///
/// The builder lifetime is supplied by the impl block that owns the registry.
type BuiltinCallHandler<'builder> = fn(
    &mut ModuleBuilder<'builder>,
    &oxc::ast::ast::CallExpression<'_>,
    &mut Body,
) -> Result<Option<smelt_hir::ExprId>, SmeltError>;

/// Stripped callee shape produced when normalizing a global-alias namespace call.
///
/// Used by [`ModuleBuilder::global_alias_namespace_call`] to record which bare
/// spelling a global-rooted callee normalizes to before the synthetic call is
/// rebuilt and re-dispatched through the ordinary call path.
enum StrippedGlobalCallee<'a> {
    /// `<alias>.<fn>(...)` normalizes to the bare free function `<fn>`.
    FreeFn(&'a str),
    /// `<alias>.<Namespace>.<method>(...)` normalizes to `<Namespace>.<method>`.
    Namespace {
        /// Bare namespace identifier the alias receiver is stripped down to.
        namespace: &'a str,
        /// Method invoked on the normalized namespace.
        method: &'a str,
    },
}

/// The receiver of one member call while its builtin handlers run.
#[derive(Debug)]
pub(in crate::lowering) struct ReceiverFrame {
    /// The receiver's source span, or `None` for a call with no member callee.
    object: Option<oxc::span::Span>,
    /// What lowering the receiver answered, once a handler asked.
    lowered: DispatchedReceiver,
}

/// The answer one lowering of a dispatched receiver gave.
#[derive(Debug, Clone, Copy)]
enum DispatchedReceiver {
    /// No handler asked yet.
    Unasked,
    /// The receiver has no value of its own (a namespace, a class name).
    NoValue,
    /// The lowered receiver.
    Value(smelt_hir::ExprId),
}

/// The receiver of a member call, lowered once by the probe that asks whether
/// the receiver declares the called member, and reused by every later
/// unhinted lowering of that receiver node while the same call lowers.
///
/// Every route the call can take after the probe (the builtin handlers,
/// `callable_static_member_call`, `member_call`) lowers the receiver again at
/// the same program point, so the probe's lowering is the value they would
/// build. Rolling the probe back and
/// lowering again would make each level of a method chain `a.f().f().f()` lower
/// the whole chain below it twice or more: exponential in the chain's length.
#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct KeptReceiver {
    /// Address of the receiver's AST node.
    node: usize,
    /// The lowered receiver.
    expr: smelt_hir::ExprId,
    /// Expressions of the body once the receiver was lowered; a roll-back
    /// below this count discards the receiver.
    exprs_after: usize,
}

impl ReceiverFrame {
    /// The empty frame of `call`.
    fn of(call: &oxc::ast::ast::CallExpression<'_>) -> Self {
        let object = match &call.callee {
            Expression::StaticMemberExpression(member) => Some(member.object.span()),
            Expression::ComputedMemberExpression(member) => Some(member.object.span()),
            _ => None,
        };
        Self {
            object,
            lowered: DispatchedReceiver::Unasked,
        }
    }
}

impl<'builder> ModuleBuilder<'builder> {
    /// Report the blocker of an import used as a member-call receiver.
    ///
    /// Returns `Err` with the import's own blocker message when the callee is a
    /// member expression whose receiver chain roots in an identifier that
    /// [`ImportScope::unresolved_value_import`] has marked — `path.join(..)`,
    /// `errors.ValidationError.of(..)`, `_.map(..)`. The `Ok(None)` case is
    /// every other callee shape, so the ordinary dispatch chain continues.
    ///
    /// The return type is `Option<ExprId>` only so this reads like the other
    /// handlers in the chain; it never produces an expression.
    fn blocked_import_member_call(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut receiver = match &call.callee {
            Expression::StaticMemberExpression(member) => &member.object,
            Expression::ComputedMemberExpression(member) => &member.object,
            _ => return Ok(None),
        };
        // Walk to the root of the receiver chain: the blocked binding may sit
        // any number of member reads below the call.
        loop {
            match receiver {
                Expression::StaticMemberExpression(member) => receiver = &member.object,
                Expression::ComputedMemberExpression(member) => receiver = &member.object,
                Expression::Identifier(object) => {
                    let Some(blocker) = self.imports.unresolved_value_import(object.name.as_str())
                    else {
                        return Ok(None);
                    };
                    return Err(SmeltError::missing_stdlib(
                        self.span(object.span.start, object.span.end),
                        blocker.to_owned(),
                    ));
                }
                _ => return Ok(None),
            }
        }
    }

    /// Lower a call expression whose result has a contextual type.
    ///
    /// Only the immediately-invoked function form consumes that context today
    /// (see [`Self::immediately_invoked_function_call_with_hint`]); every other
    /// callee shape is dispatched by [`Self::call_expression`] unchanged. An
    /// IIFE callee is an inline function or arrow expression, which none of the
    /// identifier- and member-callee dispatchers in `call_expression` can match,
    /// so trying it first changes no other decision.
    pub(in crate::lowering) fn call_expression_with_hint(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if type_hint.is_some()
            && let Some(expr) =
                self.immediately_invoked_function_call_with_hint(call, body, type_hint)?
        {
            return Ok(expr);
        }
        self.call_expression(call, body)
    }

    /// Bind the callee type parameters the arguments left unbound from the
    /// type a type assertion gives the call spanning `span`
    /// (`differenceBy(a, b, f) as T[]` instantiates the callee's element type
    /// as the caller's `T`; see `ModuleBuilder::asserted_call_return`).
    ///
    /// Only an ASSERTION instantiates the call. The assertion converts the
    /// call's value to its target whatever the call is typed as, so typing
    /// the call at the asserted instantiation moves no conversion and claims
    /// nothing the program did not already claim. An ordinary contextual
    /// type (an annotated `const x: Record<string, number> = makeBag()`)
    /// does not: there the call is what the definition returns and the
    /// declaration converts it (`a_contextual_type_does_not_instantiate_the_call`).
    ///
    /// A parameter the arguments bound keeps its binding (bindings are
    /// substituted before the match), except a binding to a type parameter
    /// that is not in scope at the call site: that is an argument lowered
    /// against the callee's uninstantiated declaration (an `any` value
    /// coerced to the implementation's `ArrayLike<T>`), which names nothing
    /// here, so the context replaces it.
    pub(in crate::lowering) fn bind_from_asserted_return(
        &mut self,
        type_params: &[TypeParamDef],
        return_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) {
        let Some(hint) = self
            .asserted_call_return
            .filter(|(call_span, _)| *call_span == span)
            .map(|(_, hint)| hint)
        else {
            return;
        };
        substitutions.retain(|_, bound| !self.type_param_is_out_of_scope(*bound));
        let _ = self.infer_callee_type_bindings(type_params, return_ty, hint, substitutions);
    }

    /// Lower `local?.method(..)` on an absent-able STRING or ARRAY local.
    ///
    /// The builtin method handlers are keyed on the receiver's static type, and
    /// `string | undefined` is not `string`, so `base?.at(-1)` fell through to
    /// a field read of `at` and called a default callback (`null`), and
    /// `s?.indexOf('x')` was rejected outright (Hono's `mergePath`). The call
    /// is instead lowered once with the local narrowed to its present type —
    /// the handler sees an ordinary `string` receiver — and wrapped as
    /// `local present ? call : undefined`, which is exactly what `?.` means.
    fn optional_builtin_receiver_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if !member.optional {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return self.optional_builtin_receiver_value_call(call, member, body);
        };
        let name = object.name.as_str();
        let Some(local) = self.scope.lookup(name) else {
            return Ok(None);
        };
        // Decided from the local's (narrowed) type WITHOUT lowering the
        // receiver, so a call this rule declines lowers exactly as before.
        let local_ty = self
            .scope
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let Some(Type::Optional(inner)) = self.ctx.krate.types.get(local_ty).cloned() else {
            return Ok(None);
        };
        if !self.builtin_keyed_receiver_type(inner) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let fact = HashMap::from([(name.to_owned(), inner)]);
        let lowered = self.with_narrowing(Some(fact), |this| this.call_expression(call, body));
        let op = lowered?;
        Ok(Some(self.wrap_optional_receiver_method(
            receiver, op, call.span, body,
        )))
    }

    /// Whether the builtin method handlers are keyed on a receiver of type
    /// `ty`: a string, an array or tuple, or a regex match value (an array
    /// with extra properties, viewed as one by [`Self::match_array_view`]).
    fn builtin_keyed_receiver_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::String | Type::List(_) | Type::Tuple(_)) => true,
            Some(Type::Class { name, .. }) => {
                self.match_stdlib_class(*name) == Some(smelt_stdlib::StdlibClass::Match)
            }
            _ => false,
        }
    }

    /// Lower `receiver?.method(..)` for a receiver that is not a bare local
    /// (`s.match(re)?.join(",")`, `lookup(k)?.at(-1)`) whose type is
    /// `T | undefined` (or `| null`) for a builtin-keyed `T`; see
    /// [`Self::builtin_keyed_receiver_type`].
    ///
    /// JavaScript evaluates the receiver ONCE, and evaluates the call (its
    /// arguments included) only when the receiver is present. The lowering is
    /// exactly that: the receiver is stored in a temporary, and the call is
    /// lowered as the present arm of `temp present ? call : undefined` with
    /// the receiver node answering a read of the temporary narrowed to `T`
    /// (`pinned_nodes`), so every builtin rule sees an ordinary
    /// `T` receiver. Its result is `R | undefined` for the call's `R`. Any
    /// other receiver type is declined with nothing lowered.
    fn optional_builtin_receiver_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        let node = std::ptr::from_ref(&member.object).addr();
        if self.pinned_nodes.iter().any(|(entry, _, _)| *entry == node) {
            return Ok(None);
        }
        self.speculate(body, |this, body| {
            let block = body.push_block(span);
            let lowered = this.with_stmt_block(
                block,
                |this| -> Result<Option<smelt_hir::ExprId>, SmeltError> {
                    let receiver = this.expression(&member.object, body)?;
                    let receiver_ty = Self::expr_ty(body, receiver);
                    let Some(present_ty) = this.optional_present_type(receiver_ty) else {
                        return Ok(None);
                    };
                    if !this.builtin_keyed_receiver_type(present_ty) {
                        return Ok(None);
                    }
                    let temp = body.push_local(smelt_hir::LocalDecl {
                        name: Some(this.intern_synthesized_name("smelt_optional_receiver")),
                        ty: receiver_ty,
                        mutable: false,
                        span,
                    });
                    let pat = body.push_pattern(smelt_hir::Pattern::Binding(temp));
                    this.push_stmt_to_sink(
                        body,
                        smelt_hir::Stmt::Let {
                            pat,
                            ty: receiver_ty,
                            value: Some(receiver),
                        },
                    );
                    this.pinned_nodes.push((node, temp, present_ty));
                    let op = this.lower_conditional_arm(body, span, |this, body| {
                        this.call_expression(call, body)
                    });
                    this.pinned_nodes.pop();
                    let op = op?;
                    let temp_read = body.push_expr(Expr {
                        kind: ExprKind::Local(temp),
                        ty: receiver_ty,
                        span,
                    });
                    Ok(Some(this.wrap_optional_receiver_method(
                        temp_read, op, call.span, body,
                    )))
                },
            )?;
            let Some(conditional) = lowered else {
                return Ok(None);
            };
            if let Some(entry) = body.block_at_mut(block) {
                entry.tail = Some(conditional);
            }
            let ty = Self::expr_ty(body, conditional);
            Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Block(block),
                ty,
                span,
            })))
        })
    }

    /// The present type of a `T | undefined` / `T | null` receiver type: the
    /// inner `T` of an `Optional`, or the one non-nullish arm of a union.
    fn optional_present_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty)? {
            Type::Optional(inner) => Some(*inner),
            Type::Union(items) => {
                let mut present = items
                    .iter()
                    .copied()
                    .filter(|item| !matches!(self.ctx.krate.types.get(*item), Some(Type::None)));
                let only = present.next()?;
                (present.next().is_none() && items.len() > 1).then_some(only)
            }
            _ => None,
        }
    }

    /// Lower a call expression.
    ///
    /// An `Array.prototype` read method (`indexOf`, `slice`, `map`, ...)
    /// registers its receiver node in `array_view_receivers` for the duration
    /// of the call, so a regex-match receiver lowers to its array view and the
    /// ordinary list rules apply to it (see [`Self::match_array_view`]). Every
    /// other receiver lowers exactly as before: the view only replaces a value
    /// whose type is the match class.
    pub(in crate::lowering) fn call_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(call) = self.super_method_call(call, body)? {
            return Ok(call);
        }
        if let Some(regexp) = self.regexp_call_construction(call, body)? {
            return Ok(regexp);
        }
        let receiver = match &call.callee {
            Expression::StaticMemberExpression(member) => Some(&member.object),
            _ => None,
        };
        let span = self.span(call.span.start, call.span.end);
        if let Some(selected) =
            self.conditional_class_select(receiver, span, body, |this, body| {
                this.call_expression(call, body)
            })?
        {
            return Ok(selected);
        }
        let receiver_key = match &call.callee {
            Expression::StaticMemberExpression(member)
                if smelt_stdlib::is_array_non_mutating_method(member.property.name.as_str()) =>
            {
                Some(std::ptr::from_ref(&member.object).addr())
            }
            _ => None,
        };
        let Some(receiver_key) = receiver_key else {
            return self.call_expression_lowering(call, body);
        };
        self.with_array_view_receiver(receiver_key, |this| {
            this.call_expression_lowering(call, body)
        })
    }

    /// Lower a call expression once its receiver view (if any) is registered.
    /// Whether a callee of type `ty` is called through the erased callable ABI.
    ///
    /// True for an erased surface (`unknown`/`any`, a class or type-parameter
    /// surface, see [`Self::erased_or_union_surface`]) and for a union whose
    /// every arm is itself callable — `Handler | MiddlewareHandler`. A union of
    /// function types has no single typed signature to call through, and its
    /// storage is already the tagged runtime value, so the call dispatches on
    /// that value at run time exactly as JavaScript does.
    pub(in crate::lowering) fn callable_through_erased_abi(&self, ty: smelt_hir::TypeId) -> bool {
        if self.erased_or_union_surface(ty) {
            return true;
        }
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => self.callable_through_erased_abi(*inner),
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
                matches!(self.ctx.krate.types.get(item), Some(Type::Function(_)))
                    || self.callable_through_erased_abi(item)
            }),
            _ => false,
        }
    }

    /// `callee` re-read at its local's stored type when
    /// [`Self::erased_callee_read_ty`] says the narrowed read is only an
    /// unwrap the erased call does anyway; any other expression unchanged.
    pub(in crate::lowering) fn stored_erased_callee(
        &self,
        callee: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let Some((local, read_ty, span)) = body.expr_at(callee).and_then(|expr| match expr.kind {
            ExprKind::Local(local) => Some((local, expr.ty, expr.span)),
            _ => None,
        }) else {
            return callee;
        };
        let ty = self.erased_callee_read_ty(read_ty, Self::local_ty(body, local));
        if ty == read_ty {
            return callee;
        }
        body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty,
            span,
        })
    }

    /// The type a call reads its erased callee local at.
    ///
    /// A local of `unknown | undefined` read at its narrowed `unknown` is an
    /// unwrap of the `Option`. A call of it goes through the dynamic dispatch,
    /// which erases the callee anyway and throws for an absent one, so the
    /// call takes the local as it is stored and the unwrap is not a value of
    /// its own (es-toolkit's `value => lastValue(value)` on the
    /// `unknown | undefined` that `last(values)` returns). Any other read
    /// keeps `read_ty`.
    pub(in crate::lowering) fn erased_callee_read_ty(
        &self,
        read_ty: smelt_hir::TypeId,
        stored_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let stored_optional_unknown = matches!(
            self.ctx.krate.types.get(stored_ty),
            Some(Type::Optional(inner)) if matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
        );
        if stored_optional_unknown && self.ctx.krate.types.get(read_ty) == Some(&Type::Unknown) {
            stored_ty
        } else {
            read_ty
        }
    }

    /// Lower `callee(args)` for a callee whose static type has no typed call
    /// signature, through the erased callable ABI.
    ///
    /// The callee is asserted to `Unknown` (a no-op when it already is) and
    /// every source argument is passed, spread arguments packed, because the
    /// run-time value — not the static type — decides the arity. The result is
    /// `Unknown`: the source's own result type for such a call is `any` or the
    /// union of the arms' results, which the caller narrows.
    ///
    /// This is the one place a computed-member or asserted callee of that kind
    /// is lowered, so the call is never dropped: earlier fallbacks answered the
    /// call with `undefined` or with its first argument, which compiled and
    /// silently skipped the call (Hono's `#dispatch` never ran a route handler).
    pub(in crate::lowering) fn erased_value_call(
        &mut self,
        callee: smelt_hir::ExprId,
        callee_span: oxc::span::Span,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let callee = self.stored_erased_callee(callee, body);
        let callee_ty = Self::expr_ty(body, callee);
        let optional_unknown = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown)) =>
            {
                Some(*inner)
            }
            _ => None,
        };
        // The call result is decided by the run-time callee; a callee that is
        // already erased was recorded where it was erased.
        let unknown_ty = optional_unknown
            .unwrap_or_else(|| self.erase_unless_unknown(callee_ty, ErasureReason::ByName));
        let callee = if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown)
            || optional_unknown.is_some()
        {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: unknown_ty,
                span: self.span(callee_span.start, callee_span.end),
            })
        };
        if Self::call_has_spread_arguments(call) {
            let args = self.packed_spread_call_arguments(unknown_ty, call, body)?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread { callee, args },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        let args = call
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: unknown_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Lower one call with its own [`KeptReceiver`] slot.
    fn call_expression_lowering(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.kept_receivers.push(None);
        let lowered = self.call_expression_routes(call, body);
        self.kept_receivers.pop();
        lowered
    }

    /// The receiver the innermost call's probe kept, when `expression` is its node.
    pub(in crate::lowering) fn kept_receiver(
        &self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<smelt_hir::ExprId> {
        let kept = self.kept_receivers.last().copied().flatten()?;
        (kept.node == std::ptr::from_ref(expression).addr() && body.exprs.len() >= kept.exprs_after)
            .then_some(kept.expr)
    }

    /// Drop every kept receiver a roll-back to `exprs` expressions discards.
    pub(in crate::lowering) fn discard_kept_receivers_after(&mut self, exprs: usize) {
        for slot in &mut self.kept_receivers {
            if slot.is_some_and(|kept| kept.exprs_after > exprs) {
                *slot = None;
            }
        }
    }

    /// Try each route a call can lower through, in priority order.
    fn call_expression_routes(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A vitest asymmetric matcher (`expect.any(..)`, `expect.not.arrayContaining(..)`)
        // is a matcher VALUE, so it is recognized before any callee resolution:
        // `expect` used as a receiver rather than as a callee would otherwise
        // fall through to the generic erased-object path and become an empty
        // record. See `ModuleBuilder::vitest_asymmetric_matcher_call`.
        // A member call whose receiver chain roots in a BLOCKED import is that
        // import's blocker, reported before any stdlib member rule can claim
        // it. Without this, the first rule that recognizes the *member* name
        // wins and reports its own reason: `path.join('/tmp', 'x.json')` was
        // matched by the static array-join helper form (`_.join(items, sep)`),
        // which reads the first ARGUMENT as its receiver and rejected the
        // string with "array join requires an array receiver". That diagnostic
        // is true of nothing the source wrote. The check belongs here rather
        // than in each rule: it is one place, and it keeps the blocker's
        // wording tied to the import that caused it.
        if let Some(expr) = self.blocked_import_member_call(call)? {
            return Ok(expr);
        }
        if let Some(expr) = self.optional_builtin_receiver_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::ComputedMemberExpression(member) = &call.callee
            && let Some(expr) = self.computed_method_call_over_members(call, member, body)?
        {
            return Ok(expr);
        }
        if let Some(expr) = self.vitest_asymmetric_matcher_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.global_alias_namespace_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.array_prototype_method_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dispatch_builtin_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.immediately_invoked_function_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::ComputedMemberExpression(member) = &call.callee {
            return self.computed_member_value_call(call, member, body);
        }
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && self.names_host(object.name.as_str(), "console")
            && matches!(member.property.name.as_str(), "log" | "warn" | "error")
        {
            return self.console_output_call(call, member, body);
        }
        if let Some(expr) = self.callable_static_member_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.class_static_method_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::StaticMemberExpression(member) = &call.callee {
            return self.member_call(
                call,
                &member.object,
                member.property.name.as_str(),
                member.span,
                member.optional,
                body,
            );
        }
        // `receiver.#method(args)` is the same member call as
        // `receiver.method(args)`; the only difference is that ES private names
        // live in a separate namespace, so the property is spelled by a
        // `PrivateIdentifier` instead of an `IdentifierName`. Class lowering
        // already registers a private method under its bare source name (see
        // `PropertyKey::PrivateIdentifier` in `decls/functions.rs`), and
        // `private_field_member` reads a private FIELD through that same name,
        // so the call routes through the shared `member_call` path unchanged.
        // A private name can never denote a builtin namespace or a static
        // stdlib member, which is why this arm sits after the builtin/static
        // pre-passes above without needing to consult them.
        if let Expression::PrivateFieldExpression(member) = &call.callee {
            return self.private_member_call(call, member, body);
        }
        if let Some(expr) = self.local_callable_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::NewExpression(callee_new) = &call.callee {
            return self.constructed_value_call(call, callee_new, body);
        }
        if let Expression::CallExpression(callee_call) = &call.callee {
            return self.call_result_call(call, callee_call, body);
        }
        if let Some(callee_expression) = Self::transparent_asserted_callee(&call.callee) {
            return self.asserted_callee_call(call, callee_expression, body);
        }
        if let Expression::Identifier(callee_ident) = &call.callee {
            return self.identifier_call(call, callee_ident, body);
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "call expression is not lowered yet",
        ))
    }

    /// The contextual type of an object-literal argument passed where the
    /// callee declares a bare interface-bounded type parameter
    /// (`longest<L extends HasLen>(a: L, ..)` called as `longest({ len: 1 }, ..)`).
    ///
    /// TypeScript infers `L` as the literal's own object type, which has
    /// exactly the interface's shape when the literal names no property the
    /// interface does not declare. Typing the literal as the interface is then
    /// the same value, and it binds `L` to a type that implements the bound's
    /// generated trait, where the untyped literal would
    /// lower to a string-keyed record that implements nothing. A literal with
    /// an extra property, a spread or a computed key keeps its own type: there
    /// `L` really is wider than the interface.
    pub(in crate::lowering) fn object_literal_bound_hint(
        &self,
        hint: smelt_hir::TypeId,
        callee_type_params: &[TypeParamDef],
        arg: &Argument<'_>,
    ) -> Option<smelt_hir::TypeId> {
        let Argument::ObjectExpression(object) = arg else {
            return None;
        };
        let Some(Type::TypeParam { name }) = self.ctx.krate.types.get(hint) else {
            return None;
        };
        let constraint = callee_type_params
            .iter()
            .find(|param| param.name == *name)?
            .constraint?;
        let Some(Type::Class { name: bound, args }) = self.ctx.krate.types.get(constraint) else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        let interface = self.find_interface(*bound)?;
        if !interface.type_params.is_empty() {
            return None;
        }
        let named = |symbol: smelt_hir::Symbol, key: &str| {
            self.ctx.krate.symbols.get(symbol) == Some(key)
                || self.ctx.krate.names.get(symbol) == Some(key)
        };
        let declares = |key: &str| {
            interface.fields.iter().any(|field| named(field.name, key))
                || interface
                    .methods
                    .iter()
                    .any(|method| named(method.name, key))
        };
        object
            .properties
            .iter()
            .all(|property| match property {
                oxc::ast::ast::ObjectPropertyKind::ObjectProperty(property) => {
                    !property.computed
                        && property.key.static_name().is_some_and(|key| {
                            declares(&crate::lowering::js_text::static_name_text(key))
                        })
                }
                oxc::ast::ast::ObjectPropertyKind::SpreadProperty(_) => false,
            })
            .then_some(constraint)
    }

    /// Lower a call through the ordered builtin/stdlib dispatch registry.
    ///
    /// This is the single seam that owns "which builtin lowering, if any, claims
    /// this callee". Each registry entry re-matches the callee AST and returns
    /// `Ok(Some(expr))` only when it handles the call. The registry is consulted
    /// strictly in declaration order, so the first matching handler wins exactly
    /// as the previous sequential guard chain did. Adding a new builtin means
    /// adding one entry to [`Self::builtin_call_handlers`] at the right position
    /// instead of editing the long inline guard chain.
    ///
    /// Returns `Ok(None)` when no builtin handler matches, leaving the caller to
    /// continue with the generic call-lowering paths. The unsupported-collection
    /// probe keeps its original semantics: a positive match surfaces as an error,
    /// not as a handled call.
    pub(in crate::lowering) fn dispatch_builtin_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // A host-module function is recognized by the IMPORT its callee names
        // (`path.join`, `posixJoin`, `createHash`), not by a member name, so it
        // is asked first: the default-import receiver `path` is otherwise an
        // erased module binding, which the erased-prototype rule below would
        // hand to a run-time `join` lookup on an empty record.
        if let Some(expr) = self.host_module_call(call, body)? {
            return Ok(Some(expr));
        }
        // Every handler below claims a callee by its member NAME. A receiver
        // whose static type declares that member as a method owns the call —
        // `bag.reduce(..)`, `stack.push(..)`, `this.join(..)` on a user class
        // are that class's methods, not `Array.prototype`'s — so the whole
        // registry yields to ordinary method dispatch.
        if let Expression::StaticMemberExpression(member) = &call.callee
            && self.receiver_statically_declares_method(
                &member.object,
                member.property.name.as_str(),
                body,
            )
        {
            return Ok(None);
        }
        // On an ERASED receiver (`any`) a prototype-method name does not say
        // which operation runs — the receiver's runtime tag does. Every handler
        // below would have to guess a prototype, and each guess is wrong for
        // some values (string containment rejected `anyArray.includes(2)`; a
        // list map over `anyNumber` panicked where JavaScript throws
        // `TypeError`), while TypeScript types the result `any` anyway. So the
        // call is left to the erased member call, whose runtime resolves the
        // method on the value it meets (`smelt_get_unknown_field`, tables in
        // `smelt_stdlib::erased_prototype_methods`). A handler that already
        // dispatches on the runtime tag (the erased `slice`, which also covers
        // host byte buffers) is not a guess, and `is_erased_prototype_call`
        // leaves it alone. A utility namespace (`_.includes(xs, v)`) is an
        // import, not an erased value, and keeps its handler; so does a call
        // passing more arguments than the prototype method accepts
        // (`utils.reduce(xs, f, 0)`, `utils.keys(record)`), which is the
        // utility-function spelling of the name, not a prototype call.
        if let Expression::StaticMemberExpression(member) = &call.callee
            && smelt_stdlib::is_erased_prototype_call(
                member.property.name.as_str(),
                call.arguments.len(),
            )
            && !self.imported_utility_object(&member.object)
            && self.receiver_is_statically_erased(&member.object, body)
        {
            return Ok(None);
        }
        // A member a crate module's namespace exports (`P.map(key, value)` with
        // `import * as P from './patterns'`) is a call to that export, whatever
        // builtin its name also spells: no builtin handler may claim it.
        if let Expression::StaticMemberExpression(member) = &call.callee
            && self.crate_namespace_member_item(member).is_some()
        {
            return self.namespace_member_call(call, body);
        }
        self.receiver_frames.push(ReceiverFrame::of(call));
        let lowered = match self.dispatched_receiver_owns_member(call, body) {
            Ok(false) => Self::builtin_call_handlers()
                .into_iter()
                .find_map(|handler| handler(self, call, body).transpose())
                .transpose(),
            Ok(true) => Ok(None),
            Err(error) => Err(error),
        };
        self.receiver_frames.pop();
        lowered
    }

    /// Whether the value a member call dispatches on declares that member.
    ///
    /// [`Self::receiver_statically_declares_method`] answers the receivers whose
    /// type is readable from declarations alone. Every other shape
    /// (`routers[0].match(p)`, `make().replace(a)`) has to be lowered to be
    /// typed, and the answer has to be asked before any handler runs: JavaScript
    /// finds a declared `match` on the receiver and never reaches
    /// `String.prototype`, so a handler keyed on the name must not see the call.
    ///
    /// A name no lowered type declares cannot be owned, so such a call lowers
    /// nothing here. A type whose module has not lowered yet is no receiver type
    /// either, which is what makes the crate-wide name set sufficient. An
    /// identifier with no static type is a namespace, never a typed receiver.
    fn dispatched_receiver_owns_member(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if !self
            .ctx
            .declared_member_names
            .contains(member.property.name.as_str())
        {
            return Ok(false);
        }
        let method = self.intern_source_name(member.property.name.as_str());
        if let Some(receiver_ty) = self.static_receiver_type(&member.object, body) {
            return Ok(self.receiver_owns_member(receiver_ty, method));
        }
        if matches!(
            member.object.without_parentheses(),
            Expression::Identifier(_)
        ) {
            return Ok(false);
        }
        let mark = self.speculation_mark(body);
        let receiver = self.dispatched_receiver(&member.object, body)?;
        let owned = receiver.is_some_and(|receiver| {
            let receiver_ty = self.optional_receiver_inner_type(Self::expr_ty(body, receiver));
            self.receiver_owns_member(receiver_ty, method)
        });
        match receiver {
            Some(expr) => {
                let node = std::ptr::from_ref(&member.object).addr();
                if let Some(slot) = self.kept_receivers.last_mut() {
                    *slot = Some(KeptReceiver {
                        node,
                        expr,
                        exprs_after: body.exprs.len(),
                    });
                }
            }
            _ => {
                self.roll_back_to(mark, body);
            }
        }
        self.forget_dispatched_receiver();
        Ok(owned)
    }

    /// Whether a source declaration of the receiver's type names `member`.
    ///
    /// Wider than [`Self::receiver_declares_method`], which decides how a method
    /// call is dispatched: here any declared member wins over a builtin of the
    /// same name, whether it is a class method, an interface signature, or a
    /// function stored in a field.
    pub(in crate::lowering) fn receiver_owns_member(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        member: smelt_hir::Symbol,
    ) -> bool {
        if self.receiver_declares_method(receiver_ty, member) {
            return true;
        }
        match self.ctx.krate.types.get(receiver_ty).cloned() {
            Some(Type::Union(arms)) => {
                !arms.is_empty()
                    && arms
                        .iter()
                        .all(|arm| self.receiver_owns_member(*arm, member))
            }
            Some(Type::TypeParam { .. }) => self
                .type_param_member_bound(receiver_ty)
                .is_some_and(|bound| self.receiver_owns_member(bound, member)),
            Some(Type::Class { name, .. }) => {
                self.stdlib_class_of_type(receiver_ty).is_none()
                    && (self.find_interface(name).is_some_and(|interface| {
                        interface
                            .methods
                            .iter()
                            .any(|signature| signature.name == member)
                            || interface.fields.iter().any(|field| field.name == member)
                    }) || self
                        .class_field_type(receiver_ty, member)
                        .is_ok_and(|field_ty| {
                            matches!(self.ctx.krate.types.get(field_ty), Some(Type::Function(_)))
                        }))
            }
            _ => false,
        }
    }

    /// The receiver of the call being dispatched, lowered once for all handlers.
    ///
    /// Most handlers gate on the receiver's type, so the first one to ask lowers
    /// it and the rest read that answer. `None` is a receiver with no value of
    /// its own (a namespace, a class name), which later rules own. A missing
    /// host surface is a blocker whichever handler meets it, so it propagates.
    pub(in crate::lowering) fn dispatched_receiver(
        &mut self,
        object: &Expression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = object.span();
        if let Some(frame) = self.receiver_frames.last()
            && frame.object == Some(span)
        {
            match frame.lowered {
                DispatchedReceiver::Unasked => {}
                DispatchedReceiver::NoValue => return Ok(None),
                DispatchedReceiver::Value(receiver) => return Ok(Some(receiver)),
            }
        }
        let lowered = match self.expression(object, body) {
            Ok(receiver) => Some(receiver),
            Err(error) if error.category == DiagnosticCategory::MissingStdlib => {
                return Err(error);
            }
            Err(_) => None,
        };
        if let Some(frame) = self.receiver_frames.last_mut()
            && frame.object == Some(span)
        {
            frame.lowered = lowered.map_or(DispatchedReceiver::NoValue, DispatchedReceiver::Value);
        }
        Ok(lowered)
    }

    /// Forget the dispatched receiver after a handler rolled its lowering back.
    pub(in crate::lowering) fn forget_dispatched_receiver(&mut self) {
        if let Some(frame) = self.receiver_frames.last_mut() {
            frame.lowered = DispatchedReceiver::Unasked;
        }
    }

    /// Whether a receiver expression is statically ERASED (`any`), decided
    /// without lowering it.
    ///
    /// A binding (or a declared user-class field chain) answers through
    /// [`Self::static_receiver_type`]; a property read — static or computed —
    /// off an erased value is itself erased, exactly as TypeScript types
    /// `anyValue.items` as `any`. Anything whose type is not knowable here
    /// answers `false`, so the caller keeps its ordinary lowering.
    pub(in crate::lowering) fn receiver_is_statically_erased(
        &mut self,
        object: &Expression<'_>,
        body: &Body,
    ) -> bool {
        // A call's result type is decided by overload resolution, which
        // `static_receiver_type` does not perform: it answers the function
        // item's IMPLEMENTATION signature, and an overloaded function's
        // implementation is routinely `(...args: unknown[]): unknown` while the
        // overload the call selects returns a concrete array (remeda's
        // `sortBy(data, ...rules).map(f)`). Treating that as erased would drop
        // the concrete type the overload gives, so a call receiver is never
        // taken as erased here.
        if matches!(object, Expression::CallExpression(_)) {
            return false;
        }
        if let Some(ty) = self.static_receiver_type(object, body) {
            return self.ctx.krate.types.get(ty) == Some(&Type::Unknown);
        }
        match object {
            Expression::StaticMemberExpression(member) => {
                self.receiver_is_statically_erased(&member.object, body)
            }
            Expression::ComputedMemberExpression(member) => {
                self.receiver_is_statically_erased(&member.object, body)
            }
            Expression::ParenthesizedExpression(paren) => {
                self.receiver_is_statically_erased(&paren.expression, body)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.receiver_is_statically_erased(&non_null.expression, body)
            }
            // `(value as any)` erases whatever `value` was.
            Expression::TSAsExpression(cast) => {
                matches!(cast.type_annotation, oxc::ast::ast::TSType::TSAnyKeyword(_))
            }
            _ => false,
        }
    }

    /// Normalize a global-alias namespace call and re-dispatch it stripped.
    ///
    /// Implements the path-normalization layer from plan §5: a call whose callee
    /// is rooted at a recognized global alias is rewritten so the alias receiver
    /// is removed, then lowered through the *ordinary* call path. This guarantees
    /// `globalThis.Object.keys(x)` and `g.Array.isArray(value)` (for a known alias
    /// `g`) emit exactly what the bare `Object.keys(x)` / `Array.isArray(value)`
    /// spellings do, instead of producing a parallel global-object slot read.
    ///
    /// Two shapes are normalized:
    ///
    /// - `<alias>.<Namespace>.<method>(...)` -> `<Namespace>.<method>(...)`
    /// - `<alias>.<fn>(...)` -> `<fn>(...)`
    ///
    /// The rewrite only fires when the alias is *only* a namespace receiver here
    /// (a static, non-optional member path); a computed key, optional chain, or
    /// any deeper escape of the alias is left untouched so it keeps its honest
    /// blocker. The synthetic call is allocated in a local arena and its
    /// arguments are cloned into it, so no AST mutation of the original program
    /// occurs and the rewrite cannot recurse (the stripped callee no longer has a
    /// global-alias root).
    pub(in crate::lowering) fn global_alias_namespace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        use oxc::allocator::{Allocator, Box as ArenaBox, CloneIn};
        use oxc::ast::ast::{
            CallExpression, IdentifierName, StaticMemberExpression, TSTypeParameterInstantiation,
        };
        use oxc::ast::builder::AstBuilder;

        if call.optional {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.optional {
            return Ok(None);
        }

        // Decide the stripped callee shape without holding the arena yet.
        let stripped = if self.expr_is_global_alias(&member.object) {
            // `<alias>.<fn>(...)`.
            StrippedGlobalCallee::FreeFn(member.property.name.as_str())
        } else if let Expression::StaticMemberExpression(inner) = &member.object
            && !inner.optional
            && self.expr_is_global_alias(&inner.object)
        {
            // `<alias>.<Namespace>.<method>(...)`.
            StrippedGlobalCallee::Namespace {
                namespace: inner.property.name.as_str(),
                method: member.property.name.as_str(),
            }
        } else {
            return Ok(None);
        };

        let arena = Allocator::default();
        let builder = AstBuilder::new(&arena);
        let span = call.span;
        let no_type_args: Option<ArenaBox<'_, TSTypeParameterInstantiation<'_>>> = None;
        let callee = match stripped {
            StrippedGlobalCallee::FreeFn(name) => {
                Expression::new_identifier(member.property.span, name, &builder)
            }
            StrippedGlobalCallee::Namespace { namespace, method } => {
                let object = Expression::new_identifier(member.span, namespace, &builder);
                Expression::StaticMemberExpression(StaticMemberExpression::boxed(
                    member.span,
                    object,
                    IdentifierName::new(member.property.span, method, &builder),
                    false,
                    &builder,
                ))
            }
        };
        let arguments = call.arguments.clone_in(&arena);
        let synthetic = CallExpression::new(span, callee, no_type_args, arguments, false, &builder);
        self.call_expression(&synthetic, body).map(Some)
    }

    /// Lower `Array.prototype.<m>.call(receiver, ...args)` for a read-only `m`.
    ///
    /// The generic `Array.prototype` methods work on any array-like receiver
    /// (a typed-array view, `arguments`, a string, a `{ length }` record) by
    /// reading its `length` and indexed elements. For a method that never
    /// writes its receiver ([`smelt_stdlib::is_array_non_mutating_method`]),
    /// running it over `Array.from(receiver)` reads exactly the same elements
    /// and answers the same value, so the call is rewritten to
    /// `Array.from(receiver).<m>(...args)` and lowered through the ordinary
    /// array-method path. Hono's `utils/crypto.ts` hex-encodes a digest with
    /// `Array.prototype.map.call(new Uint8Array(buffer), (x) => ..)`.
    ///
    /// A mutating method is left alone: its writes would land on the copy.
    /// So is a spread first argument, and any receiver of `.call` other than
    /// the unshadowed `Array.prototype.<m>` spelling. Like
    /// [`Self::global_alias_namespace_call`], the synthetic call lives in a
    /// local arena and the original program is not mutated.
    pub(in crate::lowering) fn array_prototype_method_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        use oxc::allocator::{Allocator, Box as ArenaBox, CloneIn};
        use oxc::ast::ast::{
            CallExpression, IdentifierName, StaticMemberExpression, TSTypeParameterInstantiation,
        };
        use oxc::ast::builder::AstBuilder;

        if call.optional {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(call_member) = &call.callee else {
            return Ok(None);
        };
        if call_member.optional || call_member.property.name != "call" {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(method_member) = &call_member.object else {
            return Ok(None);
        };
        let method = method_member.property.name.as_str();
        if method_member.optional || !smelt_stdlib::is_array_non_mutating_method(method) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(prototype_member) = &method_member.object else {
            return Ok(None);
        };
        if prototype_member.optional || prototype_member.property.name != "prototype" {
            return Ok(None);
        }
        if !matches!(&prototype_member.object, Expression::Identifier(array) if self.names_host(array.name.as_str(), "Array"))
            || self.classes.contains("Array")
        {
            return Ok(None);
        }
        let Some((receiver, rest)) = call.arguments.split_first() else {
            return Ok(None);
        };
        let Some(receiver) = receiver.as_expression() else {
            return Ok(None);
        };

        let arena = Allocator::default();
        let builder = AstBuilder::new(&arena);
        let span = call.span;
        let from_callee = Expression::StaticMemberExpression(StaticMemberExpression::boxed(
            prototype_member.span,
            Expression::new_identifier(prototype_member.object.span(), "Array", &builder),
            IdentifierName::new(prototype_member.property.span, "from", &builder),
            false,
            &builder,
        ));
        let mut from_arguments = oxc::allocator::Vec::new_in(&&arena);
        from_arguments.push(receiver.clone_in(&arena).into());
        let no_type_args: Option<ArenaBox<'_, TSTypeParameterInstantiation<'_>>> = None;
        let copied = Expression::CallExpression(CallExpression::boxed(
            receiver.span(),
            from_callee,
            no_type_args,
            from_arguments,
            false,
            &builder,
        ));
        let method_callee = Expression::StaticMemberExpression(StaticMemberExpression::boxed(
            method_member.span,
            copied,
            IdentifierName::new(method_member.property.span, method, &builder),
            false,
            &builder,
        ));
        let mut arguments = oxc::allocator::Vec::new_in(&&arena);
        for argument in rest {
            arguments.push(argument.clone_in(&arena));
        }
        let method_type_args: Option<ArenaBox<'_, TSTypeParameterInstantiation<'_>>> = None;
        let synthetic = CallExpression::new(
            span,
            method_callee,
            method_type_args,
            arguments,
            false,
            &builder,
        );
        self.call_expression(&synthetic, body).map(Some)
    }

    /// Ordered builtin/stdlib call lowering handlers.
    ///
    /// The order is load-bearing: earlier handlers win when two could match the
    /// same callee, exactly as in the former sequential guard chain. Do not
    /// reorder entries without verifying overlapping callees still resolve to the
    /// same handler.
    pub(in crate::lowering) fn builtin_call_handlers()
    -> impl IntoIterator<Item = BuiltinCallHandler<'builder>> {
        [
            Self::type_test_call_entry,
            Self::typed_test_value_call_entry,
            Self::date_call,
            Self::regex_replace_call,
            Self::object_assign_call,
            Self::vitest_async_expect_call,
            Self::vitest_mock_function_call,
            Self::vitest_spy_on_call,
            Self::vitest_mock_handle_method_call,
            Self::vitest_fake_timer_call,
            Self::date_fns_timezone_context_call,
            Self::sinon_fake_timers_call,
            Self::structured_clone_call,
            Self::uri_encode_call,
            Self::base64_call,
            Self::object_prototype_to_string_call,
            Self::exact_stdlib_call,
            Self::array_constructor_call,
            Self::generator_next_call,
            Self::promise_continuation_call,
            Self::timer_call,
            Self::number_to_string_call,
            Self::number_to_fixed_call,
            Self::commonjs_require_call,
            Self::object_metadata_mutation_call,
            Self::object_get_own_property_symbols_call,
            Self::object_projection_call,
            Self::object_has_own_call,
            Self::reflect_has_call,
            Self::object_get_own_property_descriptor_call,
            // Before the collection and list dispatches: `slice`, `set` and `fill`
            // are names those own for other receivers, and the family's receiver
            // type is what separates them.
            Self::dispatch_typed_array_method,
            // Beside the family's methods and for the same reason: `getInt16` and
            // `setUint8` are ordinary user method names, so the receiver's own
            // `DataView` type is what admits the accessor.
            Self::dispatch_data_view_accessor,
            Self::dispatch_collection_method,
            Self::dispatch_headers_method,
            Self::dispatch_url_search_params_method,
            Self::dispatch_form_data_method,
            Self::dispatch_text_encoder_method,
            Self::dispatch_text_decoder_method,
            Self::dispatch_node_hash_method,
            Self::dispatch_blob_method,
            Self::dispatch_response_method,
            Self::dispatch_request_method,
            // Before the emitter dispatch: an `IncomingMessage` reaches the emitter
            // through it, and the two `node:http` receivers with methods of their
            // own must claim those names first.
            Self::http_create_server_call,
            Self::hono_node_serve_call,
            Self::dispatch_http_server_method,
            Self::dispatch_server_response_method,
            Self::dispatch_sqlite_method,
            Self::dispatch_event_emitter_method,
            Self::map_projection_call,
            Self::regexp_test_call,
            Self::regexp_exec_call,
            Self::string_match_call,
            Self::string_match_all_call,
            Self::url_to_string_call,
            Self::string_case_call,
            Self::string_well_formed_call,
            Self::string_normalize_call,
            Self::string_trim_call,
            Self::string_locale_compare_call,
            Self::string_affix_call,
            Self::list_callback_call,
            Self::list_reduce_call,
            Self::list_search_call,
            Self::collection_at_call,
            Self::list_entries_call,
            Self::string_search_call,
            Self::string_replace_call,
            Self::collection_slice_call,
            Self::list_push_call,
            Self::list_unshift_call,
            Self::list_reverse_call,
            Self::list_sort_call,
            Self::modern_array_call,
            Self::list_pop_call,
            Self::list_shift_call,
            Self::string_repeat_call,
            Self::string_pad_call,
            Self::string_char_at_call,
            Self::string_join_call,
            Self::list_concat_call,
            Self::list_contains_call,
            Self::string_contains_call,
            Self::string_split_call,
            Self::namespace_member_call,
            Self::unmodeled_host_namespace_call,
            Self::function_bind_call,
        ]
    }

    /// Return whether the module declares a callable with this name, at any depth.
    ///
    /// The caller uses this to resolve a name that is not in scope yet: a later
    /// declaration, or a nested function naming itself in its own body.
    pub(in crate::lowering) fn source_contains_forward_callable(&self, name: &str) -> bool {
        self.source_facts.declares_forward_callable(name)
    }

    /// Return whether the module source declares a class with this local name.
    pub(in crate::lowering) fn source_contains_class(&self, name: &str) -> bool {
        self.source_facts.declares_class(name)
    }

    /// Rewrites the receiver for `value[index]?.method()` to optional indexing.
    ///
    /// In JavaScript, an out-of-bounds element access produces `undefined`,
    /// and the following optional method call short-circuits. Keeping the
    /// receiver as a strict `Index` would make generated Rust panic before
    /// optional chaining can observe the missing value.
    pub(in crate::lowering) fn optionalize_index_receiver(
        &mut self,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let expr = body
            .expr_at(receiver)
            .expect("expr id should point to an existing expression")
            .clone();
        let ExprKind::Index {
            receiver: index_receiver,
            index: static_index,
        } = expr.kind
        else {
            return receiver;
        };
        let ty = self.optional_chain_result_type(expr.ty);
        body.push_expr(Expr {
            kind: ExprKind::OptionalIndex {
                receiver: index_receiver,
                index: static_index,
                from_end: false,
            },
            ty,
            span: expr.span,
        })
    }

    /// The stdlib rule a call denotes: the recognition table's answer, unless a
    /// program binding shadows the callee's root name.
    ///
    /// The one question every exact rule asks, so a recognized spelling is
    /// matched once and a shadowed global gives way however it is reached.
    pub(in crate::lowering) fn recognized_call(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<RuleId> {
        let rule = stdlib_dispatch::call_rule(call, &|name, global| self.names_host(name, global))?;
        let shadowed = stdlib_dispatch::callee_root(&call.callee)
            .is_some_and(|root| self.builtin_call_identifier_is_shadowed(root));
        (!shadowed).then_some(rule)
    }

    /// Route exact global and static namespace calls through shared recognition metadata.
    ///
    /// Receiver-, shape-, plugin-, and test-framework-dependent handlers remain
    /// in `call_expression` because they require semantic inspection and ordered
    /// overlap resolution rather than exact source-name recognition.
    pub(in crate::lowering) fn exact_stdlib_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(rule) = self.recognized_call(call) else {
            return Ok(None);
        };
        match rule {
            RuleId::TsStructuredClone => self.structured_clone_call(call, body),
            RuleId::TsObjectBox => self.object_box_call(call, body),
            RuleId::TsPromiseStatic => self.promise_static_call(call, body),
            RuleId::TsFetch => self.fetch_call(call, body),
            RuleId::TsPrimitiveCast => self.primitive_cast_call(call, body),
            RuleId::TsSymbol => self.symbol_call(call, body),
            RuleId::TsMathNumeric => self.exact_math_numeric_call(call, body),
            RuleId::TsMathRandom => self.math_random_call(call, body),
            RuleId::TsNumberPredicate => self.number_predicate_call(call, body),
            RuleId::TsNumberParseFloat => self.number_parse_float_call(call, body),
            RuleId::TsNumberParseInt => self.number_parse_int_call(call, body),
            RuleId::TsBigIntStatic => self.bigint_static_call(call, body),
            RuleId::TsStringFromCodes => self.string_from_codes_call(call, body),
            RuleId::TsObjectStatic => self.exact_object_static_call(call, body),
            RuleId::TsArrayStatic => self.exact_array_static_call(call, body),
            RuleId::TsBufferStatic => self.exact_buffer_static_call(call, body),
            RuleId::TsJsonStringify => self.json_stringify_call(call, body),
            RuleId::TsJsonParse => self.json_parse_call(call, body),
            RuleId::TsCryptoRandomUuid
            | RuleId::TsCryptoGetRandomValues
            | RuleId::TsCryptoDigest
            | RuleId::TsCryptoImportKey
            | RuleId::TsCryptoSign
            | RuleId::TsCryptoVerify => self.crypto_call(rule, call, body),
            RuleId::TsAbortSignalAbort | RuleId::TsAbortSignalTimeout => {
                self.abort_signal_static_call(rule, call, body)
            }
            _ => Ok(None),
        }
    }

    /// Preserve the established ordering among exact deterministic `Math.*` handlers.
    pub(in crate::lowering) fn exact_math_numeric_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.math_abs_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_round_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_extrema_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_hypot_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_unary_func_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_pow_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_imul_call(call, body)? {
            return Ok(Some(expr));
        }
        self.math_atan2_call(call, body)
    }

    /// Preserve the established ordering among exact static `Object.*` handlers.
    pub(in crate::lowering) fn exact_object_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.object_is_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.object_from_entries_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.object_create_call(call, body)? {
            return Ok(Some(expr));
        }
        self.object_get_prototype_of_call(call, body)
    }

    /// Preserve the established ordering among exact static `Array.*` handlers.
    pub(in crate::lowering) fn exact_array_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.array_is_array_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.arraybuffer_is_view_call(call, body)? {
            return Ok(Some(expr));
        }
        self.array_from_call(call, body)
    }

    /// Strip transparent wrappers around an asserted function-call callee.
    pub(in crate::lowering) fn transparent_asserted_callee<'a>(
        callee: &'a Expression<'a>,
    ) -> Option<&'a Expression<'a>> {
        match peel(callee, Wrappers::Parens) {
            Expression::TSAsExpression(as_expr) => Some(&as_expr.expression),
            Expression::TSSatisfiesExpression(satisfies) => Some(&satisfies.expression),
            Expression::TSNonNullExpression(non_null) => Some(&non_null.expression),
            _ => None,
        }
    }
}
