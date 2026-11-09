//! Callback-surface classification: resolving callback function items,
//! arrow/function callback recognizers, factory predicates, and opaque
//! (local/member) callback values, plus building callbacks from param lists.

use crate::lowering::erasure::NotNewErasure;
use super::forwarding::{ForwardTarget, ForwardingCallback};
use crate::lowering::js_text::js_str_text;
use crate::lowering::{
    Argument, Body, Expr, ExprKind, Expression, FunctionType, HashMap, Item,
    LocalDecl, ModuleBuilder, Param, SmeltError, Span, Type,
};
use smelt_hir::ErasureReason;
use oxc::span::GetSpan;
impl ModuleBuilder<'_> {
    /// Resolve a callback function symbol back to its normal HIR item.
    pub(in crate::lowering) fn callback_function_item(
        &self,
        function: smelt_hir::Symbol,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.ctx
            .krate
            .items
            .iter()
            .enumerate()
            .find_map(|(index, item)| {
                if matches!(item, Item::Function(item_function) if item_function.name == function) {
                    Some(smelt_hir::ItemId(u32::try_from(index).ok()?))
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "callback function reference does not resolve to an item",
                )
            })
    }

    /// Wrap a function item in a first-class closure value.
    pub(in crate::lowering) fn callback_function_item_closure(
        &mut self,
        item: smelt_hir::ItemId,
        function_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned() else {
            return Err(SmeltError::unsupported(
                span,
                "callback function reference must have a function type",
            ));
        };
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut call_args = Vec::new();
        for (index, ty) in function.params.iter().copied().enumerate() {
            let name = self.ctx.krate.symbols.intern(&format!("arg{index}"));
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
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }
        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: function_ty,
            span,
        });
        let call = closure_body.push_expr(Expr {
            kind: ExprKind::Call {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        if let Some(block) = closure_body.blocks.first_mut() {
            block.tail = Some(call);
        }
        let body = self.ctx.krate.push_body(closure_body);
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: function.rest,
                required_params: function.required_params,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body,
                // Same bare function-item-as-value wrapper as
                // `item_function_closure_expression`, reached through the
                // callback-reference path. Tag it with the source item so all
                // references to this named function share one cached runtime
                // wrapper and compare equal under JavaScript `===`.
                function_item: Some(item),
                span,
            }),
            ty: function_ty,
            span,
        }))
    }

    /// Lower a supported arrow callback to a typed expression tree.
    pub(in crate::lowering) fn forwarding_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &mut Body,
    ) -> Result<ForwardingCallback, SmeltError> {
        if let Some(callback) = self.asserted_forwarding_callback(argument, expected_param_tys, body)? {
            return Ok(callback);
        }
        // A callback produced by a call (`xs.filter(negate(isEven))`) is not
        // modeled here: it is evaluated once, in the enclosing body, and then
        // called. See the callable surface branch of `Self::callback_argument`.
        if let Argument::Identifier(identifier) = argument
            && let Some(callback) = self.named_callback_reference(
                identifier.name.as_str(),
                identifier.span.start,
                identifier.span.end,
                expected_param_tys,
                body,
            )?
        {
            return Ok(callback);
        }
        if let Argument::StaticMemberExpression(_member) = argument {
            return Ok(self.opaque_member_callback(expected_param_tys));
        }
        // A callback chosen at runtime between callable values
        // (`xs.map(cond ? Object : identity)`) or coalesced from one
        // (`xs.map(maybeFn ?? identity)`). The selected callee is an opaque
        // callable surface here, so model the whole argument as an opaque
        // callback that forwards the receiver's element arguments, the same
        // way a member or imported-value callback is handled.
        if matches!(
            argument,
            Argument::ConditionalExpression(_) | Argument::LogicalExpression(_)
        ) {
            return Ok(self.opaque_member_callback(expected_param_tys));
        }
        if let Argument::ArrayExpression(array) = argument
            && let Some(callback) =
                self.matches_property_iteratee_callback(array, body)?
        {
            return Ok(callback);
        }
        Err(SmeltError::unsupported(
            self.span(argument.span().start, argument.span().end),
            "array callback methods currently require arrow function callbacks",
        ))
    }

    /// Resolve a bare identifier callback (`xs.map(fn)`) to a typed callback
    /// expression tree, if the name statically names a callable value.
    ///
    /// Handles, in resolution order:
    /// - a module-level function item, called by name;
    /// - a local/parameter binding whose static type is a `Type::Function`,
    ///   called through a capture;
    /// - an opaque callable value — an import or forward-declared function whose
    ///   body is not visible here — modeled as an [`Self::opaque_member_callback`].
    ///
    /// Returns `Ok(None)` when the name is not a resolvable callable so the
    /// caller can fall through to its remaining forms (or raise its own error).
    /// Typed function signatures and receiver element arguments are preserved so
    /// the generated call keeps its concrete parameter/return types.
    pub(in crate::lowering) fn named_callback_reference(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<Option<ForwardingCallback>, SmeltError> {
        if let Some(item) = self.items.get(name).copied() {
            let span = self.span(start, end);
            let Item::Function(function) = self.item_ref(item) else {
                return Err(SmeltError::unsupported(
                    span,
                    "array callback function references must resolve to functions",
                ));
            };
            let function_name = function.name;
            let function_params_len = function.params.len();
            let function_return_ty = function.return_ty;
            let function_ty = self.item_expr_type(item, span)?;
            // JavaScript ignores callback arguments beyond the declared
            // parameters, so a reference declaring fewer parameters than the
            // receiver supplies, down to zero (`values.filter(stubTrue)`), is
            // called with just its declared prefix.
            return Ok(Some(ForwardingCallback {
                target: ForwardTarget::Item { function: function_name, ty: function_ty },
                arity: function_params_len.min(expected_param_tys.len()),
                ty: function_return_ty,
            }));
        }
        if let Some(local) = self.scope.lookup(name) {
            let local_ty = Self::local_ty(body, local);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned() {
                return Ok(Some(ForwardingCallback {
                    target: ForwardTarget::Local { local, ty: local_ty },
                    arity: function.params.len().min(expected_param_tys.len()),
                    ty: function.return_ty,
                }));
            }
        }
        if self.is_opaque_callback_value(name) {
            self.reject_unresolved_callback_import(name, start, end)?;
            return Ok(Some(self.opaque_member_callback(expected_param_tys)));
        }
        Ok(None)
    }

    /// Report a callback that names an import no source file or host module
    /// provides, instead of standing a null callee in for it.
    pub(in crate::lowering) fn reject_unresolved_callback_import(
        &self,
        name: &str,
        span_start: u32,
        span_end: u32,
    ) -> Result<(), SmeltError> {
        self.imports.unresolved_value_import(name).map_or(Ok(()), |blocker| {
            Err(SmeltError::missing_stdlib(self.span(span_start, span_end), blocker.to_owned()))
        })
    }

    /// Lower the lodash array iteratee shorthand `[path, srcValue]` passed as a
    /// callback (`_.find(list, ['key', key])`).
    ///
    /// Lodash-style collection functions accept a two-element array in callback
    /// position as a `matchesProperty(path, srcValue)` predicate: it returns
    /// whether `element[path] === srcValue`. A callable is required in the same
    /// position for native array methods, so an array literal reaching callback
    /// classification can only be this iteratee form. Only a string-literal
    /// path is modeled; other shapes return `Ok(None)` so the caller keeps its
    /// unsupported-callback diagnostic.
    fn matches_property_iteratee_callback(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<ForwardingCallback>, SmeltError> {
        let [first, second] = array.elements.as_slice() else {
            return Ok(None);
        };
        let oxc::ast::ast::ArrayExpressionElement::StringLiteral(path) = first else {
            return Ok(None);
        };
        let Some(value_expression) = second.as_expression() else {
            return Ok(None);
        };
        // JavaScript builds the iteratee array once, at the call.
        let source_value = self.expression(value_expression, body)?;
        let value_ty = Self::expr_ty(body, source_value);
        let span = self.span(array.span.start, array.span.end);
        let symbol = self.ctx.krate.symbols.intern("__iteratee_value");
        let local = self.capture_bind_value(source_value, value_ty, symbol, span, body);
        let field = self.intern_source_name(&js_str_text(path.value));
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let field_ty = self.erase_with(ErasureReason::ByName, &js_str_text(path.value));
        Ok(Some(ForwardingCallback {
            target: ForwardTarget::MatchesProperty { field, field_ty, value: local, value_ty },
            arity: 1,
            ty: bool_ty,
        }))
    }

    /// Lower callbacks wrapped in erased TypeScript assertion syntax.
    pub(in crate::lowering) fn asserted_forwarding_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<Option<ForwardingCallback>, SmeltError> {
        match argument {
            Argument::TSAsExpression(as_expr) => Ok(Some(self.asserted_callee_callback(
                &as_expr.expression,
                expected_param_tys,
                body,
            )?)),
            Argument::TSTypeAssertion(assertion) => Ok(Some(self.asserted_callee_callback(
                &assertion.expression,
                expected_param_tys,
                body,
            )?)),
            Argument::TSSatisfiesExpression(satisfies) => Ok(Some(
                self.asserted_callee_callback(&satisfies.expression, expected_param_tys, body)?,
            )),
            Argument::TSNonNullExpression(non_null) => Ok(Some(self.asserted_callee_callback(
                &non_null.expression,
                expected_param_tys,
                body,
            )?)),
            _ => Ok(None),
        }
    }

    /// Lower the expression inside a TypeScript assertion when it is a callback.
    pub(in crate::lowering) fn asserted_callee_callback(
        &mut self,
        expression: &Expression<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<ForwardingCallback, SmeltError> {
        match expression {
            Expression::ParenthesizedExpression(parenthesized) => {
                self.asserted_callee_callback(&parenthesized.expression, expected_param_tys, body)
            }
            Expression::TSAsExpression(as_expr) => {
                self.asserted_callee_callback(&as_expr.expression, expected_param_tys, body)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.asserted_callee_callback(&satisfies.expression, expected_param_tys, body)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.asserted_callee_callback(&non_null.expression, expected_param_tys, body)
            }
            Expression::StaticMemberExpression(_member) => {
                Ok(self.opaque_member_callback(expected_param_tys))
            }
            Expression::ConditionalExpression(_) | Expression::LogicalExpression(_) => {
                Ok(self.opaque_member_callback(expected_param_tys))
            }
            // A bare identifier inside an assertion (`(fn as Fn)`, `fn!`) that
            // statically names a function item, a function-typed local, or an
            // opaque imported callable. Resolve it the same way the unwrapped
            // `xs.map(fn)` form does so the assertion is transparent.
            Expression::Identifier(identifier) => self
                .named_callback_reference(
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                    expected_param_tys,
                    body,
                )?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        "array callback methods currently require arrow function callbacks",
                    )
                }),
            _ => Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "array callback methods currently require arrow function callbacks",
            )),
        }
    }

    /// Return whether a local's type can hold a callable value handed to an
    /// array method as a named callback.
    ///
    /// `Type::Function` locals are lowered directly elsewhere; this covers the
    /// erased/generic surfaces — `unknown`/`any`, type parameters, and unions
    /// that include a function or erased branch — where the value is genuinely
    /// callable at runtime but lacks a clean static function type.
    pub(in crate::lowering) fn callback_local_value_is_callable_surface(&mut self, ty: smelt_hir::TypeId) -> bool {
        self.callback_local_value_is_callable_surface_depth(ty, 0)
    }

    /// Depth-guarded worker for [`Self::callback_local_value_is_callable_surface`].
    ///
    /// The depth bound stops runaway recursion when a type alias resolves to a
    /// type that references itself.
    fn callback_local_value_is_callable_surface_depth(
        &mut self,
        ty: smelt_hir::TypeId,
        depth: u32,
    ) -> bool {
        if depth > 8 {
            return false;
        }
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
            .cloned()
        {
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. }) => true,
            // An optional callable (`compareKeys?: (a, b) => number` used as
            // `keys.sort(compareKeys)`) is a callable surface too; the wrapper
            // closure observes the nullish payload at runtime.
            Some(Type::Optional(inner)) => {
                self.callback_local_value_is_callable_surface_depth(inner, depth + 1)
            }
            Some(Type::Union(items)) => items.iter().copied().any(|item| {
                matches!(
                    self.ctx.krate.types.get(item),
                    Some(Type::Function(_) | Type::Unknown)
                )
            }),
            // A named type reference (`ListIterateeCustom<T, boolean>`).
            //
            // When the alias definition is visible in this lowering unit, resolve
            // it and inspect the underlying type: a union alias like
            // `((v, i, c) => R) | PropertyKey | ...` still exposes a callable
            // (function) arm, so a local of that type is a callable surface.
            //
            // When the reference is *unresolved* — the alias is imported from
            // another module and its definition is not present in this lowering
            // unit (single-file lowering, e.g. `smelt probe`) — it is an erased
            // type boundary: its concrete shape is unknown here. A concrete
            // nominal type (a real `class`/`interface` declared in scope) is not
            // a callable surface, so keep rejecting those. But an opaque imported
            // reference is treated like `unknown` — a callable surface whose
            // wrapper closure observes the real value at runtime — matching how
            // the whole-crate build (where the alias resolves to its union) lowers
            // the same call.
            Some(Type::Class { name, args }) => {
                if self.types.is_callable_object_alias(name) {
                    return true;
                }
                if let Some(underlying) = self.resolve_type_alias_underlying(name, &args)
                    && underlying != ty
                {
                    return self
                        .callback_local_value_is_callable_surface_depth(underlying, depth + 1);
                }
                !self.symbol_names_concrete_nominal_type(name)
            }
            _ => false,
        }
    }

    /// Resolve a `Type::Class` alias reference to its underlying definition with
    /// the reference's type arguments substituted for the alias parameters.
    ///
    /// Returns `None` when the symbol does not name a type alias (e.g. a real
    /// class or interface), so callers can fall back to their opaque handling.
    fn resolve_type_alias_underlying(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<smelt_hir::TypeId> {
        let alias = self.find_type_alias(name).cloned()?;
        let mut substitutions = HashMap::new();
        for (index, param) in alias.type_params.iter().enumerate() {
            let actual = args
                .get(index)
                .copied()
                .or(param.default)
                .unwrap_or_else(|| {
                    self.ctx
                        .krate
                        .types
                        .intern(Type::TypeParam { name: param.name })
                });
            substitutions.insert(param.name, actual);
        }
        Some(self.substitute_type_params(alias.ty, &substitutions))
    }

    /// Return whether `name` refers to a concrete nominal type — a `class` or
    /// `interface` declared in this lowering unit — as opposed to an unresolved
    /// (imported, definition-not-present) type reference.
    fn symbol_names_concrete_nominal_type(&self, name: smelt_hir::Symbol) -> bool {
        if self.find_interface(name).is_some() {
            return true;
        }
        self.ctx
            .krate
            .items
            .iter()
            .any(|item| matches!(item, Item::Class(class) if class.name == name))
    }

    /// Build an opaque callback that calls a captured outer local value.
    ///
    /// Mirrors [`Self::opaque_member_callback`], but the callee is the named
    /// local itself (captured by id) instead of a `None` placeholder resolved by
    /// name. This lowers `xs.map(fn)` where `fn` is a local holding a callable
    /// value whose static type is erased (`any`/`unknown`), a type parameter, or
    /// a union that includes a function — cases where the local is genuinely
    /// callable but does not have a clean `Type::Function`, so the direct
    /// local-value branch above does not fire. The wrapper closure forwards the
    /// receiver's element arguments, matching how a direct `fn(...)` call lowers.
    pub(in crate::lowering) fn opaque_local_callback(
        &mut self,
        local: smelt_hir::LocalId,
        local_ty: smelt_hir::TypeId,
        expected_param_tys: &[smelt_hir::TypeId],
    ) -> ForwardingCallback {
        let unknown_ty = if self.ctx.krate.types.get(local_ty) == Some(&Type::Unknown) {
            // not a new erasure: the callee local is already erased
            self.unknown_type(NotNewErasure::ErasedUpstream)
        } else {
            // A type-parameter or function-union callee has no single return type.
            self.erase_with(ErasureReason::TypeLevel, "opaque local callback")
        };
        ForwardingCallback {
            target: ForwardTarget::Local { local, ty: local_ty },
            arity: expected_param_tys.len(),
            ty: unknown_ty,
        }
    }

    /// Build an opaque callback expression for imported predicate/function members.
    pub(in crate::lowering) fn opaque_member_callback(&mut self, expected_param_tys: &[smelt_hir::TypeId]) -> ForwardingCallback {
        let unknown_ty = self.erase_with(ErasureReason::UntypedPackage, "opaque member callback");
        let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: expected_param_tys.to_vec(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: false,
        }));
        ForwardingCallback {
            target: ForwardTarget::Opaque { ty: function_ty },
            arity: expected_param_tys.len(),
            ty: unknown_ty,
        }
    }

    /// Return whether a bare identifier names a callable value whose body is
    /// opaque to this module — an imported function value or a module-level
    /// function/const callable declared elsewhere in the source.
    ///
    /// Such names resolve like a direct call would (see `call_expression` in
    /// `stdlib/call_dispatch.rs`), so when one
    /// is handed to an array method as a named-local callback we can lower it to
    /// an opaque closure that calls the value, rather than rejecting it for not
    /// being an inline arrow. The name must not shadow a local binding, because a
    /// local with the same name is lexically nearer and handled separately.
    pub(in crate::lowering) fn is_opaque_callback_value(&self, name: &str) -> bool {
        !self.scope.is_bound(name)
            && !self.items.contains_key(name)
            && (self.imports.is_value(name) || self.source_contains_forward_callable(name))
    }

    /// Return whether an expression is an imported utility namespace/object.
    pub(in crate::lowering) fn imported_utility_object(&self, expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::Identifier(object)
                if self.imports.is_namespace(object.name.as_str())
                    || self.object_namespaces.contains_key(object.name.as_str())
        )
    }

    /// Return whether a name-matched global builtin call is shadowed by a real
    /// binding in scope, so the global interceptor must not fire.
    ///
    /// The builtin call interceptors (`isNaN(...)`, `parseInt(...)`, ...) are
    /// dispatched by identifier name *before* the callee is resolved against the
    /// module's bindings. That is only correct when the name still refers to the
    /// JavaScript global. A value import, module item, or local binding of the
    /// same name lexically shadows the global — for example es-toolkit's
    /// `import { isNaN } from './isNaN'` makes `isNaN(true)` a call to the
    /// imported function, not the numeric global predicate. In that case the
    /// interceptor bails and the ordinary call path lowers the real binding
    /// (mirroring how [`Self::imported_utility_object`] defers namespace member
    /// calls). A file that genuinely calls the global has no such binding, so the
    /// interceptor keeps firing unchanged.
    pub(in crate::lowering) fn builtin_call_identifier_is_shadowed(&self, name: &str) -> bool {
        self.scope.is_bound(name)
            || self.source_facts.declares_module_value(name)
            || (self.imports.is_value(name) && self.host_module_identifier_export_path(name).is_none())
    }

}
