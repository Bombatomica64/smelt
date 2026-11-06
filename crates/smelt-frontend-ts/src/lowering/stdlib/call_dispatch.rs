//! Call-expression lowering hub for the TypeScript frontend.
//!
//! Central dispatch for lowering call expressions: standard-library shims,
//! builtin-call registry routing, immediately-invoked function expressions, and
//! direct function/method invocation into typed HIR.

use crate::lowering::js_text::js_str_text;
use crate::lowering::{ModuleBuilder, RestParam, stdlib_dispatch};
use crate::{SmeltError, test_support};
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    AsyncOp, Body, CaptureMode, ClosureCapture, ErasureReason, Expr, ExprKind, FunctionType,
    GeneratorResumeKind, Literal, LocalDecl, Param, Pattern, Span, Stmt, Type, TypeParamDef,
};
use smelt_stdlib::{DiagnosticCategory, RuleId};
use std::collections::HashMap;

/// Render the opaque runtime string that stands for a *unique* `Symbol(...)`.
///
/// A unique symbol has fresh identity per evaluation site, so the spelling is
/// keyed by the call's source SITE — its module and offset, packed into one
/// decimal number (`file << 32 | offset`) so the spelling keeps the
/// `Symbol(d)@<digits>` shape the runtime key derivation parses. The module is
/// part of the identity because two modules routinely declare
/// `export const A: unique symbol = Symbol()` at the same offset, and those are
/// two symbols; module 0 keeps its bare offset. The description is carried along because
/// two spellings of the same symbol are compared as plain strings at runtime
/// (`js_strict_eq`), so a description-less variant would compare unequal to the
/// described one for the very same source symbol. Every path that materializes a
/// unique symbol — the ordinary call-lowering path and the const-initializer
/// folding path — must go through here so the two cannot drift apart.
pub(in crate::lowering) fn unique_symbol_spelling(description: &str, site: Span) -> String {
    let site = (u64::from(site.file.0) << 32_u32) | u64::from(site.start);
    format!("Symbol({description})@{site}")
}

/// Render the opaque runtime string that stands for a registry `Symbol.for(...)`.
///
/// Registry symbols are globally interned by description, so the spelling omits
/// the source offset and two `Symbol.for("k")` sites share one string.
pub(in crate::lowering) fn registry_symbol_spelling(description: &str) -> String {
    format!("Symbol.for({description})")
}

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

impl ReceiverFrame {
    /// The empty frame of `call`.
    fn of(call: &oxc::ast::ast::CallExpression<'_>) -> Self {
        let object = match &call.callee {
            Expression::StaticMemberExpression(member) => Some(member.object.span()),
            Expression::ComputedMemberExpression(member) => Some(member.object.span()),
            _ => None,
        };
        Self { object, lowered: DispatchedReceiver::Unasked }
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
            return Ok(None);
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
        if !matches!(
            self.ctx.krate.types.get(inner),
            Some(Type::String | Type::List(_) | Type::Tuple(_))
        ) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        self.scope.push_narrowing_fact(name.to_owned(), inner);
        let lowered = self.call_expression(call, body);
        self.scope.pop_narrowing_scope();
        let op = lowered?;
        Ok(Some(self.wrap_optional_receiver_method(receiver, op, call.span, body)))
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
        let callee_ty = Self::expr_ty(body, callee);
        // The call result is decided by the run-time callee; a callee that is
        // already erased was recorded where it was erased.
        let unknown_ty = self.erase_unless_unknown(callee_ty, ErasureReason::ByName);
        let callee = if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown) {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: unknown_ty,
                span: self.span(callee_span.start, callee_span.end),
            })
        };
        if self.call_has_spread_arguments_or_source_spread(call) {
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

    fn call_expression_lowering(
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
            && object.name == "console"
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
    /// generated trait (bounded-generics plan), where the untyped literal would
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
                || interface.methods.iter().any(|method| named(method.name, key))
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
            && smelt_stdlib::is_erased_prototype_call(member.property.name.as_str(), call.arguments.len())
            && !self.imported_utility_object(&member.object)
            && self.receiver_is_statically_erased(&member.object, body)
        {
            return Ok(None);
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
        if !self.ctx.declared_member_names.contains(member.property.name.as_str()) {
            return Ok(false);
        }
        let method = self.intern_source_name(member.property.name.as_str());
        if let Some(receiver_ty) = self.static_receiver_type(&member.object, body) {
            return Ok(self.receiver_owns_member(receiver_ty, method));
        }
        if matches!(member.object.without_parentheses(), Expression::Identifier(_)) {
            return Ok(false);
        }
        let mark = self.speculation_mark(body);
        let receiver = self.dispatched_receiver(&member.object, body)?;
        let receiver_ty = receiver.map(|receiver| Self::expr_ty(body, receiver));
        self.roll_back_to(mark, body);
        self.forget_dispatched_receiver();
        let Some(receiver_ty) = receiver_ty else {
            return Ok(false);
        };
        let receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        Ok(self.receiver_owns_member(receiver_ty, method))
    }

    /// Whether a source declaration of the receiver's type names `member`.
    ///
    /// Wider than [`Self::receiver_declares_method`], which decides how a method
    /// call is dispatched: here any declared member wins over a builtin of the
    /// same name, whether it is a class method, an interface signature, or a
    /// function stored in a field.
    pub(in crate::lowering) fn receiver_owns_member(&mut self, receiver_ty: smelt_hir::TypeId, member: smelt_hir::Symbol) -> bool {
        if self.receiver_declares_method(receiver_ty, member) {
            return true;
        }
        match self.ctx.krate.types.get(receiver_ty).cloned() {
            Some(Type::Union(arms)) => {
                !arms.is_empty() && arms.iter().all(|arm| self.receiver_owns_member(*arm, member))
            }
            Some(Type::TypeParam { .. }) => self
                .type_param_member_bound(receiver_ty)
                .is_some_and(|bound| self.receiver_owns_member(bound, member)),
            Some(Type::Class { name, .. }) => {
                self.stdlib_class_of_type(receiver_ty).is_none()
                    && (self.find_interface(name).is_some_and(|interface| {
                        interface.methods.iter().any(|signature| signature.name == member)
                            || interface.fields.iter().any(|field| field.name == member)
                    }) || self.class_field_type(receiver_ty, member).is_ok_and(|field_ty| {
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
    pub(in crate::lowering) fn receiver_is_statically_erased(&mut self, object: &Expression<'_>, body: &Body) -> bool {
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
        use oxc::ast::builder::AstBuilder;
        use oxc::ast::ast::{
            CallExpression, IdentifierName, StaticMemberExpression, TSTypeParameterInstantiation,
        };

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
        let synthetic =
            CallExpression::new(span, callee, no_type_args, arguments, false, &builder);
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
    /// `Array.prototype.map.call(new Uint8Array(buffer), (x) => ..)`, which
    /// used to go through an erased `prototype` read and a dynamic `call`
    /// that answered an empty list.
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
        if !matches!(&prototype_member.object, Expression::Identifier(array) if array.name == "Array")
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
        let synthetic =
            CallExpression::new(span, method_callee, method_type_args, arguments, false, &builder);
        self.call_expression(&synthetic, body).map(Some)
    }

    /// Adapter so an infallible `Option`-returning handler fits the registry shape.
    ///
    /// Preserves the original guard `if let Some(expr) = self.typed_test_value_call(...)`
    /// which never produced an error.
    pub(in crate::lowering) fn typed_test_value_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        Ok(self.typed_test_value_call(call, body))
    }

    /// Adapter so the type-test handler fits the builtin registry shape.
    pub(in crate::lowering) fn type_test_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.type_test_call(call, body)
    }

    /// Ordered builtin/stdlib call lowering handlers.
    ///
    /// The order is load-bearing: earlier handlers win when two could match the
    /// same callee, exactly as in the former sequential guard chain. Do not
    /// reorder entries without verifying overlapping callees still resolve to the
    /// same handler.
    pub(in crate::lowering) fn builtin_call_handlers() -> impl IntoIterator<Item = BuiltinCallHandler<'builder>> {
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
        let index = usize::try_from(receiver.0).expect("expr id should fit into usize");
        let expr = body
            .exprs
            .get(index)
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
            },
            ty,
            span: expr.span,
        })
    }

    /// Lower `CommonJS` `require(path)` as an opaque JSON-like module object.
    pub(in crate::lowering) fn commonjs_require_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "require")
        {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "require() requires exactly one module path argument",
            ));
        };
        let _ = self.argument(argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.erase_with(ErasureReason::ByName, "require");
        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `structuredClone(value)` as a typed value copy.
    ///
    /// HIR values are immutable unless explicitly modeled otherwise, so the
    /// lowering keeps the source value and preserves its type instead of
    /// modeling JavaScript's object graph cloning behavior.
    pub(in crate::lowering) fn structured_clone_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "structuredClone" {
            return Ok(None);
        }
        let [value] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "structuredClone requires exactly one value",
            ));
        };
        let operand = self.argument(value, body)?;
        // Typed values are immutable in HIR, so a pass-through copy is faithful and
        // keeps their concrete type. Erased (`unknown`) values may be mutable host
        // objects (Date/Map/Set/Error) whose graph must be deep-copied with fresh
        // identities to honor `structuredClone`'s reference semantics; route those
        // through the `smelt_structured_clone` runtime helper.
        // not a new erasure: only compared against the operand's type
        let unknown_ty = self.unknown_type();
        let operand_ty = body
            .exprs
            .get(usize::try_from(operand.0).unwrap_or(usize::MAX))
            .map(|expr| expr.ty);
        if operand_ty != Some(unknown_ty) {
            return Ok(Some(operand));
        }
        let span = self.span(call.span.start, call.span.end);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StructuredClone { operand },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower `Object.prototype.toString.call(value)` to the `"[object Tag]"` probe.
    ///
    /// This is the classic JavaScript type-probing idiom (isPlainObject,
    /// lodash `getTag`): the result string distinguishes primitives, arrays,
    /// functions, and host objects by their `@@toStringTag`-style tag. The
    /// lowering resolves the tag at runtime from the erased value's variant
    /// and host identity markers (see the `smelt_object_to_string_tag` runtime
    /// helper), instead of mis-reading `toString`/`call` as fields of the
    /// prototype sentinel. Only fires for the exact unshadowed
    /// `Object.prototype.toString.call(x)` spelling so every other member call
    /// keeps its existing path.
    pub(in crate::lowering) fn object_prototype_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(call_member) = &call.callee else {
            return Ok(None);
        };
        if call_member.property.name != "call" {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(to_string_member) = &call_member.object else {
            return Ok(None);
        };
        if to_string_member.property.name != "toString" {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(prototype_member) = &to_string_member.object else {
            return Ok(None);
        };
        if prototype_member.property.name != "prototype" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &prototype_member.object else {
            return Ok(None);
        };
        // A local binding or user class named `Object` shadows the builtin.
        if object.name != "Object"
            || self.scope.is_bound("Object")
            || self.classes.contains("Object")
        {
            return Ok(None);
        }
        let [value_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.prototype.toString.call requires exactly one value argument",
            ));
        };
        let operand = self.argument(value_arg, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ObjectToStringTag { operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Which direction of the base64 codec `name` names, if any.
    ///
    /// Asked of the name alone, so the direct-call lowering and the bare-value
    /// form (`values.map(atob)`) cannot disagree about which global is which
    /// direction.
    pub(in crate::lowering) const fn base64_global(name: &str) -> Option<smelt_hir::Base64Op> {
        match name.as_bytes() {
            b"btoa" => Some(smelt_hir::Base64Op::Encode),
            b"atob" => Some(smelt_hir::Base64Op::Decode),
            _ => None,
        }
    }

    /// Lower `btoa(text)` / `atob(text)` to the base64 IR op.
    ///
    /// The operand rule is the URI transcoders': a concrete `string` passes
    /// through and an erased or string-convertible operand is asserted to
    /// `string` first, because `btoa(1)` is `btoa("1")` in JavaScript. A source
    /// binding of either name shadows the global and declines here.
    pub(in crate::lowering) fn base64_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        let name = callee.name.as_str();
        let Some(op) = Self::base64_global(name) else {
            return Ok(None);
        };
        if self.scope.is_bound(name) || self.items.contains_key(name) {
            return Ok(None);
        }
        let operand = self.single_string_operand(name, call, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Base64Transcode { op, operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower the only argument of a one-string builtin call (`btoa(s)`,
    /// `encodeURI(s)`) to a `string` operand.
    ///
    /// A `string` passes through; a string-compatible or erased operand is
    /// asserted to `string`; anything else, or any arity but one, is rejected
    /// under the builtin's `name`.
    fn single_string_operand(
        &mut self,
        name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [value_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{name} requires exactly one string argument"),
            ));
        };
        let value = self.argument(value_arg, body)?;
        let value_ty = Self::expr_ty(body, value);
        if self.ctx.krate.types.get(value_ty) == Some(&Type::String) {
            return Ok(value);
        }
        if self.is_string_compatible_type(value_ty) || self.type_contains_unknown(value_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span: self.span(value_arg.span().start, value_arg.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(value_arg.span().start, value_arg.span().end),
            format!("{name} argument must be a string"),
        ))
    }

    /// Which of the four ECMA-262 URI transcoding globals `name` is, if any.
    ///
    /// The four share one grammar and one operand rule and differ only in the
    /// character set they treat as structure, so they share one lowering (see
    /// [`smelt_hir::UriTranscodeOp`]) rather than four near-identical ones.
    pub(in crate::lowering) const fn uri_transcode_global(
        name: &str,
    ) -> Option<smelt_hir::UriTranscodeOp> {
        match name.as_bytes() {
            b"encodeURI" => Some(smelt_hir::UriTranscodeOp::Encode),
            b"encodeURIComponent" => Some(smelt_hir::UriTranscodeOp::EncodeComponent),
            b"decodeURI" => Some(smelt_hir::UriTranscodeOp::Decode),
            b"decodeURIComponent" => Some(smelt_hir::UriTranscodeOp::DecodeComponent),
            _ => None,
        }
    }

    /// Lower `encodeURI`/`encodeURIComponent`/`decodeURI`/`decodeURIComponent`
    /// to the URI transcoding IR op.
    ///
    /// The operand must be string-compatible: a concrete `string` passes
    /// through, and an erased or string-convertible operand is asserted to
    /// `string` first, mirroring how other string builtins accept erased
    /// operands. The bare-value form of each is handled by
    /// `builtin_function_value_expression`, which builds a closure over the
    /// same op.
    pub(in crate::lowering) fn uri_encode_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        let Some(op) = Self::uri_transcode_global(callee.name.as_str()) else {
            return Ok(None);
        };
        if self.scope.is_bound(callee.name.as_str()) {
            return Ok(None);
        }
        let operand = self.single_string_operand(callee.name.as_str(), call, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UriTranscode { op, operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a generator `next`, `return`, or `throw` call, resuming the generator.
    pub(in crate::lowering) fn generator_next_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "next" | "return" | "throw") || call.arguments.len() > 1 {
            return Ok(None);
        }
        let generator = self.expression(&member.object, body)?;
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return Ok(None);
        };
        let result_ty = self.ctx.krate.types.intern(Type::GeneratorResult {
            yield_ty,
            return_ty,
        });
        let ty = if is_async {
            self.ctx.krate.types.intern(Type::Future(result_ty))
        } else {
            result_ty
        };
        let value = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .map(|argument| self.expression(argument, body))
            .transpose()?;
        let kind = match method {
            "next" => GeneratorResumeKind::Next,
            "return" => GeneratorResumeKind::Return,
            "throw" => GeneratorResumeKind::Throw,
            _ => unreachable!(),
        };
        let expected_ty = match kind {
            GeneratorResumeKind::Next => Some(next_ty),
            GeneratorResumeKind::Return => Some(return_ty),
            GeneratorResumeKind::Throw => None,
        };
        if let (Some(value), Some(expected_ty)) = (value, expected_ty)
            && !self.type_assignable_to(Self::expr_ty(body, value), expected_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "generator protocol value is not assignable to its declared type",
            ));
        }
        if matches!(kind, GeneratorResumeKind::Throw) && value.is_none() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "generator throw requires an error value",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::GeneratorNext {
                generator,
                value,
                kind,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Promise/Future `.then(...)`, `.catch(...)` and `.finally(...)`
    /// continuation calls.
    pub(in crate::lowering) fn promise_continuation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "then" | "catch" | "finally") {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let Some(Type::Future(inner_ty)) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let [callback_arg, ..] = call.arguments.as_slice() else {
            return Ok(Some(receiver));
        };
        // not a new erasure: only the contextual return hint for the callback,
        // whose real result is inferred from its body below.
        let unknown_ty = self.unknown_type();
        // The continuation parameter type is contextual, but its return type is
        // inferred from the callback body. `Promise.then` is generic in that
        // return (`U | PromiseLike<U>`); forcing `void` here erased values such
        // as a returned `Generator<Y, R, N>` before the surrounding `await` and
        // made typed `yield* await promise.then(...)` impossible.
        // `onFinally`'s result is discarded (only a returned promise is
        // awaited, and an `async` callback is a future by its own `async`), so
        // its contextual return is `void`, not an erased value.
        let callback_return_ty = if method == "finally" {
            self.ctx.krate.types.intern(Type::None)
        } else {
            unknown_ty
        };
        // `onFinally` is called with no arguments (it cannot observe the
        // outcome); `onRejected` gets the erased reason, `onFulfilled` the value.
        let callback_params = match method {
            "finally" => Vec::new(),
            // The rejection reason is `any` in TypeScript's own lib: any value
            // may be thrown anywhere in the awaited call graph.
            "catch" => vec![self.erase_with(ErasureReason::ExternalType, "rejection reason")],
            _ => vec![inner_ty],
        };
        let callback_required = callback_params.len();
        let callback_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: callback_params,
            rest: None,
            required_params: Some(callback_required),
            mutable_params: Vec::new(),
            return_ty: callback_return_ty,
            is_async: false,
            may_throw: false,
        }));
        let callback = self.argument_with_hint(callback_arg, body, Some(callback_ty))?;
        let declared_callback_result = body
            .exprs
            .get(usize::try_from(callback.0).unwrap_or(usize::MAX))
            .and_then(|expr| match &expr.kind {
                ExprKind::Closure(closure) => Some(closure.return_ty),
                _ => None,
            })
            .or_else(|| match self.ctx.krate.types.get(Self::expr_ty(body, callback)) {
                Some(Type::Function(function)) => Some(function.return_ty),
                _ => None,
            });
        let callback_result_ty = match declared_callback_result {
            Some(return_ty) => match self.ctx.krate.types.get(return_ty) {
                // Promise callbacks flatten a returned promise rather than
                // producing `Promise<Promise<T>>`.
                Some(Type::Future(item)) => *item,
                _ => return_ty,
            },
            // No closure or function type to read the callback's result from.
            _ => self.erase_with(ErasureReason::MissingAnnotation, "promise callback result"),
        };
        let (op, output_ty) = if method == "catch" {
            (AsyncOp::Catch, receiver_ty)
        } else if method == "finally" {
            // The outcome passes through unchanged, so the type does too.
            (AsyncOp::Finally, receiver_ty)
        } else {
            (
                AsyncOp::Then,
                self.ctx.krate.types.intern(Type::Future(callback_result_ty)),
            )
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op,
                args: vec![receiver, callback],
            },
            ty: output_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `callable.bind(this_arg, ...bound)` into a closure with captured arguments.
    ///
    /// JavaScript `Function.prototype.bind` produces a new callable value. Smelt
    /// models the callable value directly: the generated closure captures the
    /// original callee plus each bound argument, then forwards the remaining
    /// parameters when the closure is called.
    pub(in crate::lowering) fn function_bind_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        outer_body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "bind" {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, outer_body)?;
        let receiver_ty = Self::expr_ty(outer_body, receiver);
        let Some(function_ty) = self.function_member_type(receiver_ty) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned() else {
            return Ok(None);
        };
        let bound_source_args = call.arguments.iter().skip(1).collect::<Vec<_>>();
        if bound_source_args.len() > function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Function.prototype.bind received more bound arguments than callable parameters",
            ));
        }

        let span = self.span(call.span.start, call.span.end);
        // `bind`'s LEADING operand is the receiver, not a bound argument (the
        // bound arguments start at index 1, which is why they are skipped above).
        // It used to be read by nobody at all, so `fn.bind(context)` produced a
        // callable that forwarded the bound arguments correctly and lost the
        // context entirely -- es-toolkit's `spread` spec binds a greeting object
        // and then reads `this.greeting` inside the callee. Bind it as the
        // receiver installed for every call of the resulting callable, which is
        // exactly what `Function.prototype.bind` specifies.
        let receiver = match call.arguments.first() {
            Some(argument) => {
                let this_arg = self.argument(argument, outer_body)?;
                self.bind_this_receiver(receiver, this_arg, outer_body, span)
            }
            None => receiver,
        };
        let callee_symbol = self.intern_source_name("__smelt_bind_callee");
        let callee_local =
            self.capture_bind_value(receiver, function_ty, callee_symbol, span, outer_body);
        let mut bound_captures = Vec::new();
        for (index, argument) in bound_source_args.iter().enumerate() {
            let param_ty = function.params.get(index).copied();
            let value = self.argument_with_hint(argument, outer_body, param_ty)?;
            let value_ty = Self::expr_ty(outer_body, value);
            let symbol = self.intern_source_name(&format!("__smelt_bind_arg_{index}"));
            let local = self.capture_bind_value(value, value_ty, symbol, span, outer_body);
            bound_captures.push((local, symbol, value_ty));
        }

        let remaining_params = function
            .params
            .iter()
            .copied()
            .skip(bound_captures.len())
            .collect::<Vec<_>>();
        let mut closure_body = Body::new(None, span);
        let mut captures = Vec::new();
        let callee_body_local = Self::push_bind_capture_local(
            callee_local,
            callee_symbol,
            function_ty,
            span,
            &mut closure_body,
            &mut captures,
        );
        let mut call_args = Vec::new();
        for (source_local, symbol, ty) in bound_captures {
            let body_local = Self::push_bind_capture_local(
                source_local,
                symbol,
                ty,
                span,
                &mut closure_body,
                &mut captures,
            );
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(body_local),
                ty,
                span,
            }));
        }

        let mut closure_params = Vec::new();
        for (index, ty) in remaining_params.iter().copied().enumerate() {
            let symbol = self.synthetic_param_symbol(index);
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }

        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Local(callee_body_local),
            ty: function_ty,
            span,
        });
        let call_expr = closure_body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(call_expr)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: remaining_params,
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: function.is_async,
            may_throw: function.may_throw,
        }));
        Ok(Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty: function.return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        })))
    }

    /// Store a bind component in an outer local so a generated closure can capture it.
    pub(in crate::lowering) fn capture_bind_value(
        &self,
        value: smelt_hir::ExprId,
        ty: smelt_hir::TypeId,
        symbol: smelt_hir::Symbol,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        let stmt = Stmt::Let {
            pat,
            ty,
            value: Some(value),
        };
        self.push_stmt_to_sink(body, stmt);
        local
    }

    /// Add one captured bind component to a generated closure body.
    pub(in crate::lowering) fn push_bind_capture_local(
        source_local: smelt_hir::LocalId,
        symbol: smelt_hir::Symbol,
        ty: smelt_hir::TypeId,
        span: Span,
        closure_body: &mut Body,
        captures: &mut Vec<ClosureCapture>,
    ) -> smelt_hir::LocalId {
        let body_local = closure_body.push_local(LocalDecl {
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
            mode: CaptureMode::ByRef,
        });
        body_local
    }

    /// Lower JavaScript `Symbol(description)` branding calls as opaque strings.
    ///
    /// Smelt does not model symbol identity yet. Type-level branding libraries
    /// only need a stable opaque value here so the containing module can lower.
    pub(in crate::lowering) fn symbol_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && object.name == "Symbol"
            && member.property.name == "for"
        {
            return self.symbol_like_call(call, body, true);
        }
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Symbol" {
            return Ok(None);
        }
        self.symbol_like_call(call, body, false)
    }

    /// Lower a supported symbol-producing call as a stable opaque string.
    pub(in crate::lowering) fn symbol_like_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        global: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) supports at most one description argument",
            ));
        }
        let description_arg = call.arguments.first();
        let checked_description = description_arg
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        let ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
        if let Some(description) = checked_description
            && !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, description)),
                Some(Type::String)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) description must be a string",
            ));
        }
        let description = description_arg
            .and_then(|argument| match argument {
                Argument::StringLiteral(literal) => Some(js_str_text(literal.value).into_owned()),
                _ => None,
            })
            .unwrap_or_default();
        let value = if global {
            registry_symbol_spelling(&description)
        } else {
            unique_symbol_spelling(&description, self.span(call.span.start, call.span.end))
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Symbol(value)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
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
        let rule = stdlib_dispatch::call_rule(call)?;
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

    /// Lower a call whose callee is a local closure or function-typed local.
    pub(in crate::lowering) fn local_callable_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee_ident) = &call.callee else {
            return Ok(None);
        };
        if self.items.contains_key(callee_ident.name.as_str())
            && !self.scope.is_bound(callee_ident.name.as_str())
        {
            return Ok(None);
        }
        let callee = self.identifier_expression(
            callee_ident.name.as_str(),
            callee_ident.span.start,
            callee_ident.span.end,
            body,
        )?;
        let callee_ty = Self::expr_ty(body, callee);
        // A `typeof callee === "function"` guard narrows the *expression* type to
        // a concrete function, but the backing local keeps its declared type. When
        // that declared type is a dynamic surface (union/unknown/type parameter/
        // erased callable), codegen still dispatches the call through the runtime
        // `SmeltUnknown` ABI. Track the declared type so a spread call is packed
        // into a single flattened argument vector even after narrowing hides the
        // dynamic surface behind a function type.
        let callee_dispatches_dynamically = self.type_is_dynamic_call_surface(callee_ty)
            || self
                .scope
                .lookup(callee_ident.name.as_str())
                .is_some_and(|local| self.type_is_dynamic_call_surface(Self::local_ty(body, local)));
        let optional_function_ty = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Optional(inner))
                if call.optional
                    && matches!(self.ctx.krate.types.get(*inner), Some(Type::Function(_))) =>
            {
                Some(*inner)
            }
            _ => None,
        };
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        let function_member_ty = optional_function_ty.or_else(|| {
            self.function_member_type_for_args(
                callee_ty,
                Some(call.arguments.len()),
                &probed_arg_tys,
            )
        });
        let (function_ty, function, optional_call) = match function_member_ty {
            Some(function_ty) => {
                let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
                else {
                    return Ok(None);
                };
                (function_ty, function, optional_function_ty.is_some())
            }
            _ if matches!(
                self.ctx.krate.types.get(callee_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || self.is_callable_object_erased_class(callee_ty) =>
            {
                let ty = if self.is_callable_object_erased_class(callee_ty) {
                    self.erase(ErasureReason::UnmodeledType)
                } else {
                    // `unknown` propagates without a record; a type-parameter
                    // callee's result is resolved by name at runtime.
                    self.erase_unless_unknown(callee_ty, ErasureReason::ByName)
                };
                let callee = if self.is_callable_object_erased_class(callee_ty) {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: callee },
                        ty,
                        span: self.span(callee_ident.span.start, callee_ident.span.end),
                    })
                } else {
                    callee
                };
                // Dynamically dispatched callees (unknown/type-parameter/erased
                // callable surfaces) are invoked through the runtime
                // `SmeltUnknown` call ABI, which flattens the entire argument
                // list. A spread call must therefore pack its flattened
                // arguments into a single runtime list operand and lower as
                // `ClosureCallSpread`. Emitting a dynamic `ClosureCall` here
                // would lose the spread, leaving the call-site flatten
                // heuristic to misread the individual operands.
                if self.call_has_spread_arguments_or_source_spread(call) {
                    let args = self.packed_spread_call_arguments(ty, call, body)?;
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread { callee, args },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            _ => {
                return Ok(None);
            }
        };
        // For an optional call `f?.(..)` keep the callee at its declared
        // `Option<Function>` type. Asserting it down to the bare function type
        // would coerce an absent callee into a null-returning default callback
        // that is then called unconditionally, so `f?.(..)` would wrongly yield
        // `null` instead of `undefined`. The `Option<Function>` callee lets the
        // emitter render the short-circuiting `callee.map(|f| f(..))`.
        let callee = if optional_call || callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(callee_ident.span.start, callee_ident.span.end),
            })
        };
        let call_return_ty = if optional_call {
            self.optional_chain_result_type(function.return_ty)
        } else {
            function.return_ty
        };
        let supplied_arg_count = call.arguments.len();
        // Dynamically dispatched callees (union/unknown/type-parameter/erased
        // callable surfaces) are invoked through the runtime `SmeltUnknown` call
        // ABI, which flattens the entire argument list. A spread must therefore
        // be packed into a single runtime argument vector even when the callee
        // carries local-callback metadata: the typed-shape `[fixed.., rest_list]`
        // lowering below is only correct for a concrete function whose Rust rest
        // parameter absorbs the trailing list, not for a dynamic dispatch that
        // would otherwise wrap the rest list in a single extra `Array` argument.
        if self.call_has_spread_arguments_or_source_spread(call) && callee_dispatches_dynamically {
            // not a new erasure: names the tagged item type of the runtime
            // call ABI's packed argument list; the call keeps its typed result
            let unknown_ty = self.unknown_type();
            let packed = self.packed_spread_call_arguments(unknown_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let variadic_item_ty = function.params.first().and_then(|param| {
            if function.rest.is_some() {
                return None;
            }
            if function.params.len() != 1 {
                return None;
            }
            match self.ctx.krate.types.get(*param) {
                Some(Type::List(item_ty)) => Some(*item_ty),
                Some(Type::Never) => Some(self.erase_with(ErasureReason::TypeLevel, "never parameter")),
                _ => None,
            }
        });
        if self.call_has_spread_arguments_or_source_spread(call)
            && let Some(item_ty) = variadic_item_ty
        {
            let packed = self.packed_spread_call_arguments(item_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let rest = function.rest.and_then(|index| {
            let param = self.type_param_constraint_or_self(*function.params.get(index)?);
            match self.ctx.krate.types.get(param) {
                Some(Type::List(item_ty)) => Some(RestParam {
                    index,
                    item_ty: *item_ty,
                }),
                _ => None,
            }
        });
        let fixed_param_count = rest.map_or(function.params.len(), |rest| rest.index);
        if rest.is_some() && self.call_has_spread_arguments_or_source_spread(call) {
            let args = self.spread_closure_call_arguments(&function, rest, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // Spreads are resolved BEFORE the arity checks below, because
        // `supplied_arg_count` counts a spread as a single argument: a
        // `sink(...triple)` looked like one argument for three parameters and
        // took the shortfall branch, which returned an EMPTY argument list that
        // the emitter then padded with `default_value(..)` per parameter. So
        // `sink(...triple)` became `sink(String::new(), String::new(), 0.0)` --
        // every argument a default, the tuple never read, and it COMPILED. A
        // silent wrong answer is the worst outcome available here.
        //
        // A tuple spread has a statically known width, so it distributes
        // positionally and the call's real arity is known after expansion.
        // Anything else has no static arity and is a named blocker rather than
        // a guess.
        if rest.is_none() && call.arguments.iter().any(Argument::is_spread) {
            let expanded = self.expanded_call_arguments(&call.arguments, body)?;
            let fixed_params = function.params.get(..fixed_param_count).unwrap_or_default();
            let expanded = self.distribute_trailing_list_spread(expanded, fixed_params, body)?;
            let mut args = Vec::with_capacity(fixed_param_count);
            for argument in expanded.into_iter().take(fixed_param_count) {
                args.push(self.lower_call_arg(argument, None, body)?);
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // An OPTIONAL parameter (`b?: string`, `required_params` on the
        // function type) is omittable exactly like a defaulted one; it is
        // padded with `undefined` below. Counting it as required sent
        // `f('p')` against `(a: string, b?: string)` down the shortfall branch,
        // which dropped EVERY argument (`f()`), so `getPattern(':id')` in Hono
        // ran with an empty label.
        let required_arg_count =
            fixed_param_count.min(function.required_params.unwrap_or(fixed_param_count));
        if supplied_arg_count < required_arg_count
            || (rest.is_none() && supplied_arg_count > function.params.len())
        {
            if rest.is_none() {
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: Vec::new(),
                    },
                    ty: call_return_ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "closure call argument count does not match closure parameters",
            ));
        }

        // Carry the callee's parameter type into each argument, the way the
        // named-function and `new`-expression paths already do
        // (`new_expr.rs`'s `params.get(index)`, and the IIFE path above). This
        // loop lowered its arguments with NO hint, so an object literal written
        // inline against a closure parameter -- `showRequired({ status: 8 })`
        // where `showRequired` is a const-bound arrow taking `Opts` -- was
        // built as a bare record of erased values and only then converted to
        // the struct, erasing every value on the way in. The parameter type is
        // right here in `function.params`; a hand-written Rust port would
        // construct `Opts { .. }` directly, so the hint has to reach the
        // literal.
        let mut args = call
            .arguments
            .iter()
            .take(fixed_param_count)
            .enumerate()
            .map(|(index, arg)| {
                let hint = function
                    .params
                    .get(index)
                    .copied()
                    .filter(|param| self.hint_preserves_argument_shape(*param));
                self.argument_with_hint(arg, body, hint)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..fixed_param_count {
            // An omitted optional parameter receives `undefined`.
            let Some(param_ty) = function
                .params
                .get(index)
                .copied()
                .filter(|_| index >= function.required_params.unwrap_or(fixed_param_count))
            else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "closure call argument count does not match closure parameters",
                ));
            };
            args.push(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: param_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Some(rest) = rest {
            let rest_args = call
                .arguments
                .iter()
                .skip(rest.index)
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_args),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: call_return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Whether a parameter type is worth hinting an argument with, i.e. whether
    /// it PRESERVES the argument's own shape rather than erasing it.
    ///
    /// A hint makes the argument lower AT the hinted type. That is what turns
    /// an inline object literal into its struct, but it cuts the other way when
    /// the parameter itself is an erased surface: hinting `unknown[]` at
    /// `spreadFn([1, 2])` builds `Vec<SmeltUnknown>` directly, erasing each
    /// element at construction, where the unhinted lowering builds `Vec<f64>`
    /// and erases at the call through the boundary adapter the emitter already
    /// generates. Same runtime value, but the erasure moves from an explicit
    /// boundary into ordinary data flow, which is what the `SmeltUnknown`
    /// policy forbids — measured as +14 avoidable erasure in es-toolkit's
    /// `spread.spec.ts` when the hint was passed unconditionally.
    ///
    /// So an `unknown`, or a container whose elements/values are `unknown`, is
    /// not a shape to lower into. Peels `Optional` first, because
    /// `Optional<Opts>` is a real hint (fixture 66 covers it) while
    /// `Optional<unknown>` is not.
    pub(in crate::lowering) fn hint_preserves_argument_shape(&self, ty: smelt_hir::TypeId) -> bool {
        let peeled = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        match self.ctx.krate.types.get(peeled) {
            Some(Type::Unknown) => false,
            Some(Type::List(item) | Type::Set(item)) => {
                !matches!(self.ctx.krate.types.get(*item), Some(Type::Unknown))
            }
            Some(Type::Dict(_, value) | Type::JsMap(_, value)) => {
                !matches!(self.ctx.krate.types.get(*value), Some(Type::Unknown))
            }
            _ => true,
        }
    }

    /// Return whether a local's structural class type is an erased callable object.
    pub(in crate::lowering) fn is_callable_object_erased_class(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { name, .. }) => self.types.is_callable_object_alias(*name),
            _ => false,
        }
    }

    /// Return whether a callee of this type is invoked through the runtime
    /// `SmeltUnknown` call ABI rather than a concrete Rust function.
    ///
    /// Such callees (unknown, type parameters, unions, and erased callable
    /// objects) flatten their entire argument list at the call site, so spread
    /// arguments must be packed into a single runtime vector rather than passed
    /// as a trailing rest list.
    pub(in crate::lowering) fn type_is_dynamic_call_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_callable_object_erased_class(ty)
    }

    /// Return whether a `Type::Class` value erases to `SmeltUnknown` at codegen.
    ///
    /// This mirrors the Rust emitter's `is_erased_class_type` rule: a class-shaped
    /// type is emitted as `SmeltUnknown` when its name resolves to neither a
    /// declared class nor a declared interface. Generic structural type aliases
    /// such as `Funnel<Args>` (a `type` literal, not an `interface`) therefore
    /// erase, while `RegExp` keeps its dedicated runtime type. Keeping the
    /// predicate aligned with codegen lets the lowering decide, in the frontend,
    /// whether a receiver value will be a runtime `SmeltUnknown` at the call site.
    pub(in crate::lowering) fn class_type_erases_to_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        let Some(type_name) = self.ctx.krate.symbols.get(*name) else {
            return false;
        };
        if Self::is_ts_stdlib_class_name(type_name, smelt_stdlib::StdlibClass::RegExp) {
            return false;
        }
        !self.classes.contains(type_name) && !self.interfaces.contains(type_name)
    }

    /// Return whether a value of this type dispatches member calls through the
    /// runtime `SmeltUnknown` ABI rather than as a typed Rust value.
    ///
    /// A receiver dispatches dynamically when it is itself `unknown`, a type
    /// parameter, a union, or a class-shaped type that erases to `SmeltUnknown`
    /// (see [`Self::class_type_erases_to_unknown`]). Optional receivers unwrap to
    /// their inner type first so `Funnel<Args> | undefined` is recognized too.
    pub(in crate::lowering) fn receiver_type_dispatches_dynamically(&self, ty: smelt_hir::TypeId) -> bool {
        let inner = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        matches!(
            self.ctx.krate.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.class_type_erases_to_unknown(inner)
    }

    /// Return whether the receiving object of a static member call erases to
    /// `SmeltUnknown` at codegen.
    ///
    /// The receiver is lowered to recover its type; the produced expression is
    /// otherwise unused here because the member-read callee already lowered the
    /// receiver. When the receiver erases, the member read yields a runtime
    /// `SmeltUnknown`, so a spread call through it must be packed as a
    /// `ClosureCallSpread` regardless of the field's declared function type.
    pub(in crate::lowering) fn static_member_receiver_dispatches_dynamically(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> bool {
        let Ok(receiver) = self.expression(&member.object, body) else {
            return false;
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        self.receiver_type_dispatches_dynamically(receiver_ty)
    }

    /// Expand a JavaScript spread call into fixed function parameters plus rest.
    pub(in crate::lowering) fn spread_closure_call_arguments(
        &mut self,
        function: &FunctionType,
        rest: Option<RestParam>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let rest = rest.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure calls require a rest-like list parameter",
            )
        })?;
        let fixed_param_count = rest.index;
        let mut args = Vec::new();
        let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
        let mut rest_items = Vec::new();
        let mut rest_list = None;
        let mut fixed_index = 0;
        for argument in &call.arguments {
            if fixed_index < fixed_param_count {
                let Argument::SpreadElement(spread) = argument else {
                    args.push(self.argument(argument, body)?);
                    fixed_index += 1;
                    continue;
                };
                // The fixed parameters read the list as written: only what lands in
                // the rest takes the rest's element type, and `predicate` of
                // `f(...args)` is not an element of `(n, predicate, ...rest: number[])`.
                let written = self.expression(&spread.argument, body)?;
                let spread_list = if matches!(self.ctx.krate.types.get(Self::expr_ty(body, written)), Some(Type::List(_))) {
                    written
                } else {
                    self.rest_spread_list_value(written, rest_ty, spread.span, body)
                };
                let consumed = fixed_param_count - fixed_index;
                for offset in 0..consumed {
                    let index = self.usize_float_literal(
                        offset,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    let ty = function
                        .params
                        .get(fixed_index + offset)
                        .copied()
                        .unwrap_or_else(|| {
                            self.index_type(Self::expr_ty(body, spread_list))
                                .unwrap_or(rest.item_ty)
                        });
                    let kind = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                        ExprKind::OptionalIndex {
                            receiver: spread_list,
                            index,
                        }
                    } else {
                        ExprKind::Index {
                            receiver: spread_list,
                            index,
                        }
                    };
                    args.push(body.push_expr(Expr {
                        kind,
                        ty,
                        span: self.span(spread.span.start, spread.span.end),
                    }));
                }
                fixed_index = fixed_param_count;
                let spread_rest = if consumed == 0 {
                    self.rest_spread_list_value(spread_list, rest_ty, spread.span, body)
                } else {
                    let start = self.usize_float_literal(
                        consumed,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    let tail = body.push_expr(Expr {
                        kind: ExprKind::ListSlice {
                            list: spread_list,
                            start: Some(start),
                            end: None,
                        },
                        ty: Self::expr_ty(body, spread_list),
                        span: self.span(spread.span.start, spread.span.end),
                    });
                    self.rest_spread_list_value(tail, rest_ty, spread.span, body)
                };
                rest_list = Some(rest_list.map_or(spread_rest, |left| {
                    body.push_expr(Expr {
                        kind: ExprKind::ListConcat {
                            left,
                            right: spread_rest,
                        },
                        ty: rest_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                }));
                continue;
            }
            match argument {
                Argument::SpreadElement(spread) => {
                    Self::flush_rest_items(
                        &mut rest_items,
                        &mut rest_list,
                        rest_ty,
                        body,
                        self.span(spread.span.start, spread.span.end),
                    );
                    let spread_list =
                        self.rest_spread_list_expression(&spread.argument, rest_ty, spread.span, body)?;
                    rest_list = Some(rest_list.map_or(spread_list, |left| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left,
                                right: spread_list,
                            },
                            ty: rest_ty,
                            span: self.span(spread.span.start, spread.span.end),
                        })
                    }));
                }
                _ => rest_items.push(self.argument(argument, body)?),
            }
        }
        if fixed_index < fixed_param_count {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure call does not provide enough fixed parameters",
            ));
        }
        Self::flush_rest_items(
            &mut rest_items,
            &mut rest_list,
            rest_ty,
            body,
            self.span(call.span.start, call.span.end),
        );
        args.push(rest_list.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            })
        }));
        Ok(args)
    }

    /// Convert an array spread argument to the statically required rest-list type.
    ///
    /// Generic array parameters are represented as erased runtime values even
    /// when their source constraint is an array. A rest call still needs their
    /// array payload for concatenation, so extract that payload before building
    /// the packed argument vector.
    pub(in crate::lowering) fn rest_spread_list_expression(
        &mut self,
        expression: &Expression<'_>,
        rest_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        Ok(self.rest_spread_list_value(value, rest_ty, span, body))
    }

    /// [`Self::rest_spread_list_expression`] for an already lowered `value`.
    fn rest_spread_list_value(
        &self,
        value: smelt_hir::ExprId,
        rest_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        if value_ty == rest_ty {
            return value;
        }
        if self.type_contains_unknown(value_ty)
            || matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::TypeParam { .. } | Type::Union(_))
            )
        {
            return body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: rest_ty,
                },
                ty: rest_ty,
                span: self.span(span.start, span.end),
            });
        }
        // A concrete list converts element by element: `g(...numbers)` into
        // `(...args: any[])` is a `List<Float>` for a `List<Unknown>` slot, and
        // the emitter reads a lone operand of another type as one element of the
        // rest, not all of it.
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: rest_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Lower a small positional index as the numeric literal used by JS indexing.
    pub(in crate::lowering) fn usize_float_literal(
        &mut self,
        value: usize,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = value.to_string().parse::<f64>().map_err(|err| {
            SmeltError::unsupported(span, format!("spread index cannot be represented: {err}"))
        })?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span,
        }))
    }

    /// Append queued rest items to the accumulated rest list expression.
    pub(in crate::lowering) fn flush_rest_items(
        rest_items: &mut Vec<smelt_hir::ExprId>,
        rest_list: &mut Option<smelt_hir::ExprId>,
        rest_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) {
        if rest_items.is_empty() {
            return;
        }
        let right = body.push_expr(Expr {
            kind: ExprKind::ListLit(std::mem::take(rest_items)),
            ty: rest_ty,
            span,
        });
        *rest_list = Some(rest_list.map_or(right, |left| {
            body.push_expr(Expr {
                kind: ExprKind::ListConcat { left, right },
                ty: rest_ty,
                span,
            })
        }));
    }

    /// Pack JavaScript spread call arguments into one variadic list argument.
    pub(in crate::lowering) fn packed_spread_call_arguments(
        &mut self,
        item_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        self.packed_spread_arguments(item_ty, &call.arguments, span, body)
    }

    /// Pack an argument slice (call or `new` expression) into one variadic list.
    pub(in crate::lowering) fn packed_spread_arguments(
        &mut self,
        item_ty: smelt_hir::TypeId,
        arguments: &[Argument<'_>],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut current_items = Vec::new();
        let mut packed = None;
        for arg in arguments {
            match arg {
                Argument::SpreadElement(spread) => {
                    if !current_items.is_empty() {
                        let left = body.push_expr(Expr {
                            kind: ExprKind::ListLit(std::mem::take(&mut current_items)),
                            ty: list_ty,
                            span,
                        });
                        packed = Some(packed.map_or(left, |existing| {
                            body.push_expr(Expr {
                                kind: ExprKind::ListConcat {
                                    left: existing,
                                    right: left,
                                },
                                ty: list_ty,
                                span,
                            })
                        }));
                    }
                    let spread_expr = self.expression(&spread.argument, body)?;
                    packed = Some(packed.map_or(spread_expr, |existing| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left: existing,
                                right: spread_expr,
                            },
                            ty: list_ty,
                            span,
                        })
                    }));
                }
                other => current_items.push(self.argument(other, body)?),
            }
        }
        if !current_items.is_empty() {
            let right = body.push_expr(Expr {
                kind: ExprKind::ListLit(current_items),
                ty: list_ty,
                span,
            });
            packed = Some(packed.map_or(right, |existing| {
                body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: existing,
                        right,
                    },
                    ty: list_ty,
                    span,
                })
            }));
        }
        packed.ok_or_else(|| {
            SmeltError::unsupported(
                span,
                "spread call requires at least one argument",
            )
        })
    }

    /// Lower type-test-only assertion calls to a no-op expression.
    ///
    /// APIs such as Vitest's `expectTypeOf` and `expect-type` assertions exist
    /// to make TypeScript's checker verify source types. They do not represent
    /// runtime behavior, so Smelt lowers the value under test to keep ordinary
    /// expression/type errors visible and erases the assertion call itself.
    pub(in crate::lowering) fn type_test_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(root_call) = self.type_test_root_call(call) else {
            return Ok(None);
        };
        if let Some(value) = root_call.arguments.first() {
            self.with_type_test_argument_mode(|this| this.argument(value, body))?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a call uses JavaScript spread arguments.
    pub(in crate::lowering) fn call_has_spread_arguments(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        call.arguments
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)))
    }

    /// Return whether a call contains JavaScript spread syntax.
    ///
    /// Some parser surfaces can preserve the argument expression while hiding
    /// the spread wrapper after TypeScript-specific normalization. The source
    /// span probe keeps spread-call lowering tied to source syntax instead of
    /// treating the argument as an ordinary array value.
    pub(in crate::lowering) fn call_has_spread_arguments_or_source_spread(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> bool {
        Self::call_has_spread_arguments(call)
            || self
                .source
                .get(
                    usize::try_from(call.span.start).unwrap_or(0)
                        ..usize::try_from(call.span.end).unwrap_or(0),
                )
                .is_some_and(|text| text.contains("..."))
            || call.arguments.iter().any(|argument| {
                let span = argument.span();
                let start = usize::try_from(call.span.start).unwrap_or(0);
                let argument_start = usize::try_from(span.start).unwrap_or(start);
                if self
                    .source
                    .get(argument_start..)
                    .is_some_and(|suffix| suffix.starts_with("..."))
                {
                    return true;
                }
                let end = argument_start;
                self.source
                    .get(start..end)
                    .is_some_and(|prefix| prefix.trim_end().ends_with("..."))
            })
    }

    /// Lower Remeda's `$typed<T>()` declaration-test helper to an opaque value.
    pub(in crate::lowering) fn typed_test_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        if callee.name != "$typed" || !self.allows_unknown_index_access() {
            return None;
        }
        let ty = self.erase_with(ErasureReason::TestHarness, "$typed");
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Lower call arguments for a statically typed, possibly under-applied
    /// closure call, padding missing optional parameters with typed `None`.
    ///
    /// A curried or partially applied call site may supply fewer arguments than
    /// the selected [`FunctionType`] declares. When the omitted parameters are
    /// optional (`Optional<T>` ABI) or absorbed by a trailing rest slot, the
    /// call is still statically typed: rather than erasing the callee, this
    /// synthesizes a `None` literal for each missing fixed parameter (typed with
    /// that parameter's declared `Optional<T>`) and an empty `List` for an
    /// unfilled rest slot. Supplied arguments beyond the fixed parameters are
    /// packed into the rest list, or evaluated and dropped when no rest slot
    /// exists, matching JavaScript's excess-argument semantics.
    pub(in crate::lowering) fn typed_under_applied_closure_arguments(
        &mut self,
        function: &FunctionType,
        arguments: &[Argument<'_>],
        call_span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call_span.start, call_span.end);
        // Fixed parameters are those before a rest slot; a rest signature keeps
        // its trailing `List` parameter out of the fixed range.
        let fixed_param_count = function.rest.unwrap_or(function.params.len());
        // Lower every supplied argument FIRST, expanding a tuple spread into one
        // argument per element. The loop below is positional -- it pairs
        // argument `index` with parameter `index` -- so a spread has to have
        // become its elements before it runs. Treating the spread as a single
        // argument is what made `sink(...triple)` pair the whole tuple with
        // parameter 0 and synthesize typed `None` for the other two.
        let expanded = self.expanded_call_arguments(arguments, body)?;
        let mut supplied = Vec::with_capacity(expanded.len());
        for argument in expanded {
            supplied.push(self.lower_call_arg(argument, None, body)?);
        }
        let mut args = Vec::with_capacity(function.params.len());
        for index in 0..fixed_param_count {
            if let Some(argument) = supplied.get(index).copied() {
                args.push(argument);
            } else {
                // Missing optional parameter: synthesize a typed `None` carrying
                // the parameter's declared type so the closure call stays typed.
                let param_ty = function
                    .params
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| self.erase(ErasureReason::Unclassified));
                args.push(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: param_ty,
                    span,
                }));
            }
        }
        if let Some(rest_index) = function.rest {
            let rest_args = supplied.get(rest_index..).unwrap_or_default().to_vec();
            let rest_param_ty = function
                .params
                .get(rest_index)
                .copied()
                .unwrap_or_else(|| {
                    let item = self.erase(ErasureReason::Unclassified);
                    self.ctx.krate.types.intern(Type::List(item))
                });
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_args),
                ty: rest_param_ty,
                span,
            }));
        } else {
            // No rest slot: surplus arguments are evaluated for their side
            // effects and discarded, exactly as JavaScript ignores extra call
            // arguments.
            for argument in arguments.iter().skip(fixed_param_count) {
                let _ = self.argument(argument, body)?;
            }
        }
        Ok(args)
    }

    /// Fold Remeda partial-bind nested calls into direct closure calls.
    pub(in crate::lowering) fn partial_bind_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        let append_partial_last = match callee.name.as_str() {
            "partialBind" => false,
            "partialLastBind" => true,
            _ => return Ok(None),
        };
        let Some((function_arg, prefix_args)) = callee_call.arguments.split_first() else {
            return Err(SmeltError::unsupported(
                self.span(callee_call.span.start, callee_call.span.end),
                "partialBind requires a function argument",
            ));
        };
        let callee_expr = self.argument(function_arg, body)?;
        let function_ty = self
            .ctx
            .krate
            .types
            .get(Self::expr_ty(body, callee_expr))
            .cloned();
        let Some(Type::Function(function)) = function_ty else {
            return Err(SmeltError::unsupported(
                self.span(function_arg.span().start, function_arg.span().end),
                "partialBind function argument must be callable",
            ));
        };
        let mut args = Vec::new();
        let ordered_args: Box<dyn Iterator<Item = &Argument<'_>>> = if append_partial_last {
            Box::new(outer_call.arguments.iter().chain(prefix_args.iter()))
        } else {
            Box::new(prefix_args.iter().chain(outer_call.arguments.iter()))
        };
        for arg in ordered_args {
            args.push(self.argument(arg, body)?);
        }
        if args.len() != function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "partialBind folded call argument count does not match function",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_expr,
                args,
            },
            ty: function.return_ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }

    /// Fold Remeda `sliceString(start, end?)(data)` into a string slice.
    pub(in crate::lowering) fn slice_string_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        if callee.name != "sliceString" {
            return Ok(None);
        }
        let [data_arg] = outer_call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "sliceString data-last call requires one data argument",
            ));
        };
        let (start_arg, end_arg) = match callee_call.arguments.as_slice() {
            [start] => (start, None),
            [start, end] => (start, Some(end)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "sliceString data-last call requires start and optional end",
                ));
            }
        };
        let operand = self.argument(data_arg, body)?;
        let start = Some(self.slice_index_argument(start_arg, body)?);
        let end = end_arg
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSlice {
                operand,
                start,
                end,
            },
            ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }

    /// Return the root `expectTypeOf(...)`-style call for a type-test chain.
    pub(in crate::lowering) fn type_test_root_call<'a>(
        &self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        if self.is_type_test_root_callee(&call.callee) {
            return Some(call);
        }
        match &call.callee {
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            _ => None,
        }
    }

    /// Return the root type-test call for an expression in a member chain.
    pub(in crate::lowering) fn type_test_root_expression<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        match expression {
            Expression::CallExpression(call) => self.type_test_root_call(call),
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.type_test_root_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.type_test_root_expression(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.type_test_root_expression(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.type_test_root_expression(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Return whether `callee` starts a supported type-test assertion chain.
    pub(in crate::lowering) fn is_type_test_root_callee(&self, callee: &Expression<'_>) -> bool {
        matches!(
            callee,
            Expression::Identifier(ident)
                if self.imports.is_test_builtin(ident.name.as_str())
                    && test_support::is_type_test_builtin_name(ident.name.as_str())
        )
    }

    /// Strip transparent wrappers around an asserted function-call callee.
    pub(in crate::lowering) fn transparent_asserted_callee<'a>(callee: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
        match callee {
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::transparent_asserted_callee(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => Some(&as_expr.expression),
            Expression::TSSatisfiesExpression(satisfies) => Some(&satisfies.expression),
            Expression::TSNonNullExpression(non_null) => Some(&non_null.expression),
            _ => None,
        }
    }
}
