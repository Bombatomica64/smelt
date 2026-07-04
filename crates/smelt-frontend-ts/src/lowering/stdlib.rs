//! Focused TypeScript standard-library lowering helpers.

mod buffer;
mod call_dispatch;
mod collections;
mod numbers_math;
mod objects;

use oxc::ast::ast::{Argument, CallExpression, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{
    BinOp, Body, Expr, ExprKind, ListCallbackOp, ListProjectionOp, ListSpliceItem, Literal,
    Pattern, RegexMatchOp, Stmt, Type, UnaryOp,
};
use smelt_stdlib::RuleId;

use super::{ModuleBuilder, SmeltError, stdlib_dispatch};

impl ModuleBuilder<'_> {
    /// Lower TypeScript `Object.assign(target, ...sources)` for homogeneous record values.
    pub(super) fn object_assign_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "assign" {
            return Ok(None);
        }
        let [target_arg, source_args @ ..] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires a target record and at least one source record",
            ));
        };
        if source_args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires at least one source record",
            ));
        }

        if self.object_assign_callable_target_candidate(target_arg, body)
            && Self::object_assign_callable_sources_static(source_args)
        {
            let target = self.argument(target_arg, body)?;
            let target_ty = Self::expr_ty(body, target);
            let resolved = self.type_param_constraint_or_self(target_ty);
            if !matches!(
                self.ctx.krate.types.get(resolved),
                Some(Type::Function(_) | Type::Class { .. })
            ) {
                return Err(SmeltError::unsupported(
                    self.span(target_arg.span().start, target_arg.span().end),
                    "Object.assign callable target must lower to a function or class value",
                ));
            }
            let props = self.object_assign_callable_props(source_args, body)?;
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::CallableObjectAssign {
                    callable: target,
                    props,
                },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let mut sources = Vec::with_capacity(source_args.len());
        let mut record_ty = None;
        for source_arg in source_args {
            let source = self.argument_with_hint(source_arg, body, record_ty)?;
            let source_ty = Self::expr_ty(body, source);
            if let Some(source_record_ty) = self.object_assign_record_source_type(source_ty) {
                if let Some(expected_ty) = record_ty {
                    if source_record_ty != expected_ty {
                        return Err(SmeltError::unsupported(
                            self.span(source_arg.span().start, source_arg.span().end),
                            "Object.assign sources must share the target record type",
                        ));
                    }
                } else {
                    record_ty = Some(source_record_ty);
                }
            } else if self.object_assign_object_like_source(source_ty) {
                if record_ty.is_none() {
                    record_ty = Some(self.object_assign_generic_record_type());
                }
            } else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Object.assign sources must be record or object-like values",
                ));
            }
            sources.push(source);
        }

        let Some(record_ty) = record_ty else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires record-typed arguments",
            ));
        };
        let mut target = self.argument_with_hint(target_arg, body, Some(record_ty))?;
        if Self::expr_ty(body, target) != record_ty
            && self.object_assign_castable_target(Self::expr_ty(body, target))
        {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }
        if Self::expr_ty(body, target) != record_ty
            && matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, target)),
                Some(Type::Dict(_, _))
            )
        {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }
        if Self::expr_ty(body, target) != record_ty {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }

        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictAssign { target, sources },
            ty: record_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the concrete record type represented by an `Object.assign` source.
    fn object_assign_record_source_type(
        &self,
        source_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) => Some(source_ty),
            Some(Type::Optional(inner)) => self.object_assign_record_source_type(*inner),
            _ => None,
        }
    }

    /// Return whether a source is a TypeScript object surface assignable by `Object.assign`.
    fn object_assign_object_like_source(&self, source_ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Class { .. } | Type::TypeParam { .. } | Type::Unknown) => true,
            Some(Type::Optional(inner)) => self.object_assign_object_like_source(*inner),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.object_assign_object_like_source(item)),
            _ => false,
        }
    }

    /// Return whether an `Object.assign` target can be coerced to the merged record type.
    fn object_assign_castable_target(&self, target_ty: smelt_hir::TypeId) -> bool {
        self.object_assign_object_like_source(target_ty)
            || matches!(self.ctx.krate.types.get(target_ty), Some(Type::None))
    }

    /// Build the fallback record type for object-like assign sources without a record source.
    fn object_assign_generic_record_type(&mut self) -> smelt_hir::TypeId {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Return whether an `Object.assign` target should use callable decoration lowering.
    fn object_assign_callable_target_candidate(
        &self,
        target_arg: &Argument<'_>,
        body: &Body,
    ) -> bool {
        match target_arg {
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
            Argument::Identifier(ident) => {
                if let Some(local) = self.locals.get(ident.name.as_str()).copied() {
                    let ty = Self::local_ty(body, local);
                    let resolved = self.type_param_constraint_or_self(ty);
                    return matches!(
                        self.ctx.krate.types.get(resolved),
                        Some(Type::Function(_) | Type::Class { .. })
                    );
                }
                false
            }
            _ => false,
        }
    }

    /// Lower static object-literal sources used to decorate callable values.
    fn object_assign_callable_props(
        &mut self,
        source_args: &[Argument<'_>],
        body: &mut Body,
    ) -> Result<Vec<(smelt_hir::Symbol, smelt_hir::ExprId)>, SmeltError> {
        let mut props = Vec::new();
        for source_arg in source_args {
            if let Argument::Identifier(ident) = source_arg
                && self.locals.contains_key(ident.name.as_str())
            {
                continue;
            }
            let Argument::ObjectExpression(object) = source_arg else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Object.assign callable sources must be static object literals",
                ));
            };
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                    return Err(SmeltError::unsupported(
                        self.span(property.span().start, property.span().end),
                        "Object.assign callable sources do not support spread properties yet",
                    ));
                };
                if object_property.computed || object_property.method {
                    return Err(SmeltError::unsupported(
                        self.span(object_property.span.start, object_property.span.end),
                        "Object.assign callable sources require static data properties",
                    ));
                }
                let key_text = match &object_property.key {
                    PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                    PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(
                                object_property.key.span().start,
                                object_property.key.span().end,
                            ),
                            "Object.assign callable property keys must be static string keys",
                        ));
                    }
                };
                let value = self.expression(&object_property.value, body)?;
                let name = self.intern_source_name(&key_text);
                props.push((name, value));
            }
        }
        Ok(props)
    }

    /// Return whether `Object.assign` sources are static enough for callable decoration.
    fn object_assign_callable_sources_static(source_args: &[Argument<'_>]) -> bool {
        source_args.iter().all(|source_arg| {
            matches!(
                source_arg,
                Argument::Identifier(_) | Argument::ObjectExpression(_)
            )
        })
    }

    /// Lower Vitest `vi.fn<T>()` mock factories as callable placeholders.
    pub(super) fn vitest_mock_function_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "vi" || member.property.name != "fn" {
            return Ok(None);
        }
        let function_ty = if let Some(type_args) = &call.type_arguments {
            let [target] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "vi.fn<T>() supports exactly one function type argument",
                ));
            };
            let ty = self.ts_type_to_hir(target)?;
            if !matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
                return Err(SmeltError::unsupported(
                    self.span(target.span().start, target.span().end),
                    "vi.fn<T>() type argument must be a function type",
                ));
            }
            ty
        } else {
            let unknown = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx
                .krate
                .types
                .intern(Type::Function(smelt_hir::FunctionType {
                    params: Vec::new(),
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: unknown,
                    is_async: false,
                    may_throw: false,
                }))
        };
        if let Some(implementation) = call.arguments.first() {
            let value = self.argument(implementation, body)?;
            // A `vi.fn<(...) => void>(impl)` annotation only constrains the mock's
            // declared shape for type-checking; at runtime JS still returns the
            // wrapped implementation's value through callers like `map`. Asserting
            // `impl` straight to a `=> void` (lowered to `Type::None`) function type
            // would erase that return value at value-consuming call sites, so when
            // the declared return is `void` we keep the declared params/arity (which
            // drive call tracking) but recover the implementation's real return type.
            let assert_ty = self.vitest_mock_assert_ty(function_ty, Self::expr_ty(body, value));
            if Self::expr_ty(body, value) == assert_ty {
                return Ok(Some(value));
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: assert_ty,
                span: self.span(implementation.span().start, implementation.span().end),
            })));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: function_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Pick the function type a `vi.fn<T>(impl)` mock should assert its
    /// implementation to.
    ///
    /// The declared annotation `T` fixes the mock's parameter shape (and thus its
    /// call-tracking arity), but a `=> void` return only means "callers ignore the
    /// result" at the type level — in JS the mock still forwards the wrapped
    /// implementation's runtime value. When the declared return is `void`
    /// (`Type::None`) and the implementation is itself a function with a concrete
    /// return type, we rebuild the declared signature with the implementation's
    /// return type so value-consuming call sites (e.g. `map`) keep that value
    /// instead of receiving `()`. In every other case the declared type is used
    /// unchanged.
    fn vitest_mock_assert_ty(
        &mut self,
        declared_ty: smelt_hir::TypeId,
        impl_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let Some(Type::Function(declared)) = self.ctx.krate.types.get(declared_ty) else {
            return declared_ty;
        };
        let declared = declared.clone();
        if self.ctx.krate.types.get(declared.return_ty) != Some(&Type::None) {
            return declared_ty;
        }
        let Some(Type::Function(implementation)) = self.ctx.krate.types.get(impl_ty) else {
            return declared_ty;
        };
        let impl_return_ty = implementation.return_ty;
        if self.ctx.krate.types.get(impl_return_ty) == Some(&Type::None) {
            return declared_ty;
        }
        let mut adjusted = declared;
        adjusted.return_ty = impl_return_ty;
        self.ctx.krate.types.intern(Type::Function(adjusted))
    }

    /// Lower Vitest `vi.spyOn(target, "method")` calls as mock handles.
    pub(super) fn vitest_spy_on_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "vi" || member.property.name != "spyOn" {
            return Ok(None);
        }
        let [target, method] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "vi.spyOn requires target object and method name arguments",
            ));
        };
        let _target = self.argument(target, body)?;
        let method_expr = self.argument(method, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, method_expr)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(method.span().start, method.span().end),
                "vi.spyOn method name must be a string",
            ));
        }
        let is_date_timezone_offset_spy = matches!(method, Argument::StringLiteral(method_name) if method_name.value == "getTimezoneOffset");
        Ok(Some(self.vitest_mock_handle_expr(
            self.span(call.span.start, call.span.end),
            body,
            is_date_timezone_offset_spy,
        )))
    }

    /// Lower Vitest mock-handle methods used to configure or restore mocks.
    pub(super) fn vitest_mock_handle_method_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(
            method,
            "mockImplementation" | "mockReturnValue" | "mockRestore" | "mockClear" | "mockReset"
        ) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let is_date_timezone_offset_spy =
            self.is_vitest_date_timezone_offset_spy(Self::expr_ty(body, receiver));
        if is_date_timezone_offset_spy && method == "mockReturnValue" {
            let [offset] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockReturnValue requires exactly one return value",
                ));
            };
            let offset = self.argument(offset, body)?;
            let ty = Self::expr_ty(body, receiver);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateSetTimezoneOffset { offset },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if is_date_timezone_offset_spy && matches!(method, "mockRestore" | "mockReset") {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{method} does not accept arguments"),
                ));
            }
            let ty = Self::expr_ty(body, receiver);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateResetTimezoneOffset,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "mockImplementation" {
            let [implementation] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockImplementation requires exactly one implementation callback",
                ));
            };
            let _implementation = self.argument(implementation, body)?;
            return Ok(Some(receiver));
        }
        // `mockReturnValue(value)` configures a non-date mock's return value.
        // The mock handle is a placeholder, so evaluate the configured value
        // for its side effects and yield the receiver, matching Vitest's
        // chainable mock API (`mock.mockReturnValue(x)` returns the mock).
        if method == "mockReturnValue" {
            let [value] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockReturnValue requires exactly one return value",
                ));
            };
            let _value = self.argument(value, body)?;
            return Ok(Some(receiver));
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{method} does not accept arguments"),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Build the opaque value Smelt uses for Vitest mock handles.
    fn vitest_mock_handle_expr(
        &mut self,
        span: smelt_hir::Span,
        body: &mut Body,
        date_timezone_offset_spy: bool,
    ) -> smelt_hir::ExprId {
        let ty = if date_timezone_offset_spy {
            let name = self.intern_source_name("__SmeltVitestDateTimezoneOffsetSpy");
            self.ctx.krate.types.intern(Type::Class {
                name,
                args: Vec::new(),
            })
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        };
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span,
        })
    }

    /// Return whether a mock handle controls `Date.prototype.getTimezoneOffset`.
    fn is_vitest_date_timezone_offset_spy(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(*name)
                    == Some("__smelt_vitest_date_timezone_offset_spy")
        )
    }

    /// Lower Vitest fake-clock APIs to the mutable timestamp observed by `Date.now()`.
    pub(super) fn vitest_fake_timer_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "vi" {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        match member.property.name.as_str() {
            "setSystemTime" => {
                let [timestamp] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.setSystemTime requires exactly one Date-compatible value",
                    ));
                };
                let timestamp = self.argument(timestamp, body)?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DateSetNow { timestamp },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "useRealTimers" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.useRealTimers does not accept arguments",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DateResetNow,
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "useFakeTimers" => {
                let Some(argument) = call.arguments.first() else {
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                };
                if call.arguments.len() != 1 {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.useFakeTimers accepts at most one options object",
                    ));
                }
                let Argument::ObjectExpression(options) = argument else {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "vi.useFakeTimers currently expects an options object",
                    ));
                };
                for property in &options.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let key = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => continue,
                    };
                    if key == "now" {
                        let timestamp = self.expression(&property.value, body)?;
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::DateSetNow { timestamp },
                            ty,
                            span: self.span(call.span.start, call.span.end),
                        })));
                    }
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower `tz(zone)` from `@date-fns/tz` to its date-context function value.
    pub(super) fn date_fns_timezone_context_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if !self
            .date_fns_timezone_factories
            .contains(callee.name.as_str())
        {
            return Ok(None);
        }
        let [timezone] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "@date-fns/tz tz(...) requires exactly one time zone string",
            ));
        };
        let timezone = self.argument(timezone, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, timezone)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "@date-fns/tz tz(...) requires a time zone string",
            ));
        }
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self
            .ctx
            .krate
            .types
            .intern(Type::Function(smelt_hir::FunctionType {
                params: vec![unknown],
                rest: None,
                required_params: Some(1),
                mutable_params: Vec::new(),
                return_ty: unknown,
                is_async: false,
                may_throw: false,
            }));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateTimezoneContext { timezone },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower async Vitest matcher chains as `Promise<void>` test-side effects.
    pub(super) fn vitest_async_expect_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(matcher) = &call.callee else {
            return Ok(None);
        };
        let Expression::StaticMemberExpression(modifier) = &matcher.object else {
            return Ok(None);
        };
        if !matches!(modifier.property.name.as_str(), "resolves" | "rejects") {
            return Ok(None);
        }
        let Expression::CallExpression(expect_call) = &modifier.object else {
            return Ok(None);
        };
        let Expression::Identifier(expect_ident) = &expect_call.callee else {
            return Ok(None);
        };
        if expect_ident.name != "expect" || !self.test_builtins.contains("expect") {
            return Ok(None);
        }
        let Some(actual_arg) = expect_call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(expect_call.span.start, expect_call.span.end),
                "expect(...).resolves/rejects matcher requires an actual promise",
            ));
        };
        if modifier.property.name == "rejects"
            && matches!(
                matcher.property.name.as_str(),
                "toThrow" | "toThrowErrorMatchingInlineSnapshot"
            )
        {
            return self.vitest_rejects_to_throw_call(call, actual_arg, body);
        }

        let actual = self.argument(actual_arg, body)?;
        if self
            .awaitable_inner_type(Self::expr_ty(body, actual))
            .is_none()
        {
            return Err(SmeltError::unsupported(
                self.span(actual_arg.span().start, actual_arg.span().end),
                "expect(...).resolves/rejects actual value must be a Promise<T>",
            ));
        }
        for argument in &call.arguments {
            self.argument(argument, body)?;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `await expect(promise).rejects.toThrow(...)` to native exception flow.
    ///
    /// The actual must be awaited, so it has to lower to a real `Future<T>`:
    /// imported `async` helpers resolve to `Type::Future` across the project
    /// graph and drive the full reject-and-message assertion below. When the
    /// actual is only an erased `Unknown` (single-file lowering before
    /// cross-module resolution determines the future), awaiting it would be
    /// invalid HIR, so — mirroring the ordinary erased-`await` fallback — the
    /// actual is evaluated for its effects and a benign `Promise<void>`
    /// placeholder is returned instead of rejecting the matcher outright.
    fn vitest_rejects_to_throw_call(
        &mut self,
        call: &CallExpression<'_>,
        actual_arg: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(call.span.start, call.span.end);

        // Evaluate the actual once and classify it. Only a real `Future<T>`
        // supports the awaited assertion; an erased actual takes the benign
        // placeholder path, and anything non-awaitable is a genuine blocker.
        let actual = self.argument(actual_arg, body)?;
        let actual_ty = Self::expr_ty(body, actual);
        if self.awaitable_inner_type(actual_ty).is_none() {
            return Err(SmeltError::unsupported(
                self.span(actual_arg.span().start, actual_arg.span().end),
                "expect(...).rejects.toThrow(...) actual value must be a Promise<T>",
            ));
        }
        let Some(item_ty) = self.future_inner_type(actual_ty) else {
            for argument in &call.arguments {
                self.argument(argument, body)?;
            }
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span,
            })));
        };

        let did_throw_name = self.intern_source_name("did_throw");
        let did_throw = body.push_local(smelt_hir::LocalDecl {
            name: Some(did_throw_name),
            ty: bool_ty,
            mutable: true,
            span,
        });
        let did_throw_pat = body.push_pattern(Pattern::Binding(did_throw));
        let false_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        body.push_stmt(Stmt::Let {
            pat: did_throw_pat,
            ty: bool_ty,
            value: Some(false_expr),
        });

        let try_block = body.push_block(span);
        let awaited = body.push_expr(Expr {
            kind: ExprKind::Await(actual),
            ty: item_ty,
            span,
        });
        body.push_stmt_to_block(try_block, Stmt::Expr(awaited));

        let catch_message = body.push_local(smelt_hir::LocalDecl {
            name: Some(self.intern_source_name("error")),
            ty: string_ty,
            mutable: false,
            span,
        });
        let catch_block = body.push_block(span);
        let did_throw_target = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let true_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            catch_block,
            Stmt::Assign {
                target: did_throw_target,
                value: true_expr,
            },
        );
        if let Some(expected_arg) = call.arguments.first() {
            let expected = self.argument(expected_arg, body)?;
            if Self::expr_ty(body, expected) == string_ty {
                let error_expr = body.push_expr(Expr {
                    kind: ExprKind::Local(catch_message),
                    ty: string_ty,
                    span,
                });
                let contains = self.contains_expr(error_expr, expected, call.span, body)?;
                let failed = self.unary_bool_expr(UnaryOp::Not, contains, call.span, body);
                let failure_block = body.push_block(span);
                let message = self.string_literal_expr(
                    "expect(...).rejects.toThrow(...) message failed",
                    call.span,
                    body,
                );
                body.push_stmt_to_block(failure_block, Stmt::Throw(message));
                body.push_stmt_to_block(
                    catch_block,
                    Stmt::If {
                        cond: failed,
                        then_block: failure_block,
                        else_block: None,
                    },
                );
            }
        }
        body.push_stmt(Stmt::TryCatch {
            body: try_block,
            catch_binding: Some(catch_message),
            catch_body: Some(catch_block),
            finally_body: None,
        });

        let did_throw_check = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let failed = self.unary_bool_expr(UnaryOp::Not, did_throw_check, call.span, body);
        self.push_test_failure_if(
            failed,
            "expect(...).rejects.toThrow(...) failed",
            call.span,
            body,
        );

        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span,
        })))
    }

    /// Lower Sinon fake timer construction and clock methods used by test helpers.
    pub(super) fn sinon_fake_timers_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if let Expression::Identifier(object) = &member.object
            && object.name == "sinon"
            && member.property.name == "useFakeTimers"
        {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "sinon.useFakeTimers supports at most one timestamp argument",
                ));
            }
            if let Some(argument) = call.arguments.first() {
                let timestamp = self.argument(argument, body)?;
                if !self.is_date_constructor_arg_type(Self::expr_ty(body, timestamp)) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "sinon.useFakeTimers timestamp must be DateArg-compatible",
                    ));
                }
            }
            let name = self.intern_type_name("SinonFakeTimers");
            let ty = self.ctx.krate.types.intern(Type::Class {
                name,
                args: Vec::new(),
            });
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            member.property.name.as_str(),
            "restore"
                | "reset"
                | "tick"
                | "tickAsync"
                | "next"
                | "nextAsync"
                | "runAll"
                | "runAllAsync"
                | "runToLast"
                | "runToLastAsync"
                | "setSystemTime"
        ) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = self.optional_receiver_inner_type(Self::expr_ty(body, receiver));
        if !matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Class { .. } | Type::Unknown)
        ) {
            return Ok(None);
        }
        for argument in &call.arguments {
            self.argument(argument, body)?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `JSON.stringify(value)` calls for JSON-compatible values.
    pub(super) fn json_stringify_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsJsonStringify) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "JSON" || member.property.name != "stringify" {
            return Ok(None);
        }
        if call.arguments.is_empty() || call.arguments.len() > 3 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.stringify() currently supports value, replacer, and space arguments",
            ));
        }
        for argument in call.arguments.iter().skip(1) {
            let _ = self.argument(argument, body)?;
        }
        let Some(argument) = call.arguments.first() else {
            return Ok(None);
        };
        let value = self.argument(argument, body)?;
        if !self.is_json_serializable_type(Self::expr_ty(body, value)) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "JSON.stringify() value must be JSON-serializable",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonStringify { value },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `JSON.parse<T>(text)` calls for JSON-compatible targets.
    pub(super) fn json_parse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsJsonParse) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "JSON" || member.property.name != "parse" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse<T>() currently supports exactly one text argument",
            ));
        };
        let ty = if let Some(type_args) = &call.type_arguments {
            let [target_ty] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "JSON.parse requires exactly one type argument",
                ));
            };
            self.ts_type_to_hir(target_ty)?
        } else {
            self.json_parse_fallback_type()
        };
        if !self.is_json_serializable_type(ty) && !self.is_erased_json_class_target(ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse<T>() target type must be JSON-compatible",
            ));
        }
        let text = self.json_parse_text_argument(argument, body, "JSON.parse<T>()")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse { text },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the fallback type for untyped `JSON.parse(text)`.
    fn json_parse_fallback_type(&mut self) -> smelt_hir::TypeId {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Lower `JSON.parse(text) as T` using the assertion type as parse target.
    pub(super) fn json_parse_call_with_target(
        &mut self,
        expression: &Expression<'_>,
        target: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::CallExpression(call) = expression else {
            return Ok(None);
        };
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsJsonParse) {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse() currently supports exactly one text argument",
            ));
        };
        if call.type_arguments.is_some() {
            return Ok(None);
        }
        if !self.is_json_serializable_type(target) && !self.is_erased_json_class_target(target) {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "JSON.parse assertion target type must be JSON-compatible",
            ));
        }
        let text = self.json_parse_text_argument(argument, body, "JSON.parse()")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse { text },
            ty: target,
            span: self.span(span.start, span.end),
        })))
    }

    /// Lower and coerce the text operand accepted by `JSON.parse`.
    fn json_parse_text_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        source_name: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let text = self.argument(argument, body)?;
        let text_ty = Self::expr_ty(body, text);
        if self.ctx.krate.types.get(text_ty) == Some(&Type::String) {
            return Ok(text);
        }
        if self.is_string_compatible_type(text_ty) || self.type_contains_unknown(text_ty) {
            let target = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: text,
                    target,
                },
                ty: target,
                span: self.span(argument.span().start, argument.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(argument.span().start, argument.span().end),
            format!("{source_name} text argument must be a string"),
        ))
    }

    /// Lower TypeScript `new RegExp(pattern).test(text)` to a regex boolean match.
    pub(super) fn regexp_test_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "test" {
            return Ok(None);
        }
        let is_known_regexp_test = stdlib_dispatch::call_rule(call) == Some(RuleId::TsRegExpTest);
        if !is_known_regexp_test && !self.could_be_regexp_test_receiver(&member.object, body) {
            return Ok(None);
        }
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires exactly one string argument",
            ));
        };
        let haystack = self.argument(haystack_argument, body)?;
        let Some(haystack) = self.regexp_text_operand(haystack, body) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires a string haystack",
            ));
        };
        let receiver = self.expression(&member.object, body)?;
        if matches!(self.ctx.krate.types.get(Self::expr_ty(body, receiver)), Some(Type::Class { name, .. }) if self.ctx.krate.symbols.get(*name).is_some_and(|name| name == "RegExp"))
        {
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let optional_unknown_ty = self.ctx.krate.types.intern(Type::Optional(unknown_ty));
            let exec = body.push_expr(Expr {
                kind: ExprKind::RegexExec {
                    regex: receiver,
                    haystack,
                },
                ty: optional_unknown_ty,
                span: self.span(call.span.start, call.span.end),
            });
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::None),
                span: self.span(call.span.start, call.span.end),
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: exec,
                    rhs: none,
                },
                ty: bool_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let Some(pattern) =
            self.regexp_pattern_expression(&member.object, body, is_known_regexp_test)?
        else {
            return Ok(None);
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexIsMatch {
                op: RegexMatchOp::Search,
                pattern,
                haystack,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether an untagged `.test(...)` receiver is plausibly a `RegExp` value.
    ///
    /// Many validation libraries expose their own `.test(...)` APIs. Smelt only
    /// routes untagged calls into regex lowering when the receiver is a direct
    /// regex-producing expression or a local/module value with a regex-like type
    /// surface.
    fn could_be_regexp_test_receiver(&self, expression: &Expression<'_>, body: &Body) -> bool {
        match expression {
            Expression::NewExpression(new_expr) => {
                matches!(&new_expr.callee, Expression::Identifier(callee) if callee.name == "RegExp")
            }
            Expression::CallExpression(call_expr) => {
                matches!(&call_expr.callee, Expression::Identifier(callee) if callee.name == "RegExp")
            }
            Expression::RegExpLiteral(_) => true,
            Expression::Identifier(identifier) => self
                .locals
                .get(identifier.name.as_str())
                .and_then(|local| {
                    usize::try_from(local.0)
                        .ok()
                        .and_then(|index| body.locals.get(index))
                        .map(|decl| decl.ty)
                })
                .or_else(|| self.module_globals.get(identifier.name.as_str()).copied())
                .is_some_and(|ty| self.regexp_receiver_type(ty)),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return false;
                };
                self.const_objects
                    .get(object.name.as_str())
                    .or_else(|| self.ctx.object_consts.get(object.name.as_str()))
                    .and_then(|object_const| {
                        object_const
                            .entries
                            .iter()
                            .find(|entry| entry.key == member.property.name.as_str())
                    })
                    .is_some_and(|entry| self.regexp_receiver_type(entry.value_ty))
            }
            _ => false,
        }
    }

    /// Return whether a HIR type can represent a regex value in TS lowering.
    fn regexp_receiver_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String) => true,
            Some(Type::Class { name, .. }) => {
                self.ctx
                    .krate
                    .symbols
                    .get(*name)
                    .or_else(|| self.ctx.krate.names.get(*name))
                    == Some("RegExp")
            }
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.regexp_receiver_type(item)),
            _ => false,
        }
    }

    /// Lower TypeScript `pattern.exec(text)` to an optional match array.
    pub(super) fn regexp_exec_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "exec" {
            return Ok(None);
        }
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.exec() requires exactly one string argument",
            ));
        };
        let regex = self.expression(&member.object, body)?;
        let haystack = self.argument(haystack_argument, body)?;
        let Some(haystack) = self.regexp_text_operand(haystack, body) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.exec() requires a string haystack",
            ));
        };
        // The result is a concrete match value (or `null`); consumer reads
        // (`m[0]`, `m.index`, `m.input`, `m.groups.name`) resolve to typed
        // accessors on the generated `SmeltMatch` type instead of the erased
        // `SmeltUnknown` boundary.
        let match_ty = self.match_result_type();
        let ty = self.ctx.krate.types.intern(Type::Optional(match_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexExec { regex, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `text.match(pattern)` to an optional match array.
    pub(super) fn string_match_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "match" {
            return Ok(None);
        }
        let [pattern_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "String.match() requires exactly one RegExp argument",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        if !self.is_string_compatible_type(Self::expr_ty(body, haystack)) {
            return Ok(None);
        }
        let pattern = self.argument(pattern_argument, body)?;
        let pattern_ty = Self::expr_ty(body, pattern);
        let pattern = if self.is_string_compatible_type(pattern_ty) {
            pattern
        } else if self.ctx.krate.types.get(pattern_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(pattern_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: pattern,
                    target: string_ty,
                },
                ty: string_ty,
                span: self.span(pattern_argument.span().start, pattern_argument.span().end),
            })
        } else {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: pattern },
                ty: string_ty,
                span: self.span(pattern_argument.span().start, pattern_argument.span().end),
            })
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
        let ty = self.ctx.krate.types.intern(Type::Optional(list_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexFind { pattern, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Extract a pattern string from a supported TypeScript RegExp-producing expression.
    pub(super) fn regexp_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let ([pattern_argument] | [pattern_argument, _]) = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "RegExp construction requires a string pattern and optional flags",
            ));
        };
        let pattern = self.argument(pattern_argument, body)?;
        let pattern = self.regexp_pattern_operand(pattern, body)?.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "RegExp construction requires a string pattern",
            )
        })?;
        let flags = if let Some(flags_argument) = new_expr.arguments.get(1) {
            let flags = self.argument(flags_argument, body)?;
            self.regexp_text_operand(flags, body).ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(flags_argument.span().start, flags_argument.span().end),
                    "RegExp flags must be string-compatible",
                )
            })?
        } else {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: self.ctx.krate.types.intern(Type::String),
                span: self.span(new_expr.span.start, new_expr.span.end),
            })
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::New {
                class: self.intern_type_name("RegExp"),
                args: vec![pattern, flags],
            },
            ty: self.regexp_type(),
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Extract a pattern string from a supported TypeScript RegExp-producing expression.
    fn regexp_pattern_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        require_regexp_receiver: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        match expression {
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    if !require_regexp_receiver {
                        return Ok(None);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                };
                if callee.name != "RegExp" {
                    if !require_regexp_receiver {
                        return Ok(None);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                }
                let ([pattern_argument] | [pattern_argument, _]) = new_expr.arguments.as_slice()
                else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp construction requires a string pattern and optional flags",
                    ));
                };
                let pattern = self.argument(pattern_argument, body)?;
                self.regexp_pattern_operand(pattern, body)
            }
            Expression::CallExpression(call_expr) => {
                let Expression::Identifier(callee) = &call_expr.callee else {
                    if !require_regexp_receiver {
                        return Ok(None);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                };
                if callee.name != "RegExp" {
                    if !require_regexp_receiver {
                        return Ok(None);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                }
                let ([pattern_argument] | [pattern_argument, _]) = call_expr.arguments.as_slice()
                else {
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp construction requires a string pattern and optional flags",
                    ));
                };
                let pattern = self.argument(pattern_argument, body)?;
                self.regexp_pattern_operand(pattern, body)
            }
            Expression::RegExpLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(Self::regex_literal_pattern_text(
                        literal,
                    ))),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                })))
            }
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_) => {
                let pattern = self.expression(expression, body)?;
                Ok(self.regexp_text_operand(pattern, body))
            }
            _ if require_regexp_receiver => Err(SmeltError::unsupported(
                self.expression_span(expression),
                "RegExp.test() requires a RegExp receiver",
            )),
            _ => Ok(None),
        }
    }

    /// Coerce a regex text expression to the string representation used by Rust regex APIs.
    fn regexp_pattern_operand(
        &mut self,
        pattern: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.regexp_text_operand(pattern, body)
            .map(Some)
            .ok_or_else(|| {
                let index = usize::try_from(pattern.0).expect("expr id should fit into usize");
                let span = body
                    .exprs
                    .get(index)
                    .expect("expr id should point to an existing expression")
                    .span;
                SmeltError::unsupported(span, "RegExp.test() requires a string pattern")
            })
    }

    /// Coerce a regex text expression to the string representation used by Rust regex APIs.
    fn regexp_text_operand(
        &mut self,
        pattern: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let pattern_ty = Self::expr_ty(body, pattern);
        if self.is_string_compatible_type(pattern_ty) {
            return Some(pattern);
        }
        if self.ctx.krate.types.get(pattern_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(pattern_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let index = usize::try_from(pattern.0).ok()?;
            let span = body.exprs.get(index)?.span;
            return Some(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: pattern,
                    target: string_ty,
                },
                ty: string_ty,
                span,
            }));
        }
        None
    }

    /// Return whether a HIR type can be serialized by the JSON mapping.
    fn is_json_serializable_type(&mut self, ty: smelt_hir::TypeId) -> bool {
        self.is_json_serializable_type_inner(ty, &mut Vec::new())
    }

    /// Return whether a HIR type can be serialized without recursing forever.
    fn is_json_serializable_type_inner(
        &mut self,
        ty: smelt_hir::TypeId,
        seen: &mut Vec<smelt_hir::Symbol>,
    ) -> bool {
        let Some(ty_kind) = self.ctx.krate.types.get(ty).cloned() else {
            return false;
        };
        match ty_kind {
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::TypeParam { .. } => true,
            Type::List(item) | Type::Set(item) | Type::Optional(item) => {
                self.is_json_serializable_type_inner(item, seen)
            }
            Type::Tuple(items) => items
                .iter()
                .all(|item| self.is_json_serializable_type_inner(*item, seen)),
            Type::Union(items) => items
                .iter()
                .all(|item| self.is_json_serializable_type_inner(*item, seen)),
            Type::Dict(key, value) => {
                matches!(self.ctx.krate.types.get(key), Some(Type::String))
                    && self.is_json_serializable_type_inner(value, seen)
            }
            Type::Class { name, args } => {
                self.json_class_fields(name, &args).is_some_and(|fields| {
                    if seen.contains(&name) {
                        return true;
                    }
                    seen.push(name);
                    let serializable = fields
                        .iter()
                        .all(|field| self.is_json_serializable_type_inner(field.ty, seen));
                    seen.pop();
                    serializable
                })
            }
            Type::None => true,
            Type::Never | Type::Function(_) | Type::Future(_) => false,
        }
    }

    /// Return fields for class-like TypeScript types that can map to JSON objects.
    fn json_class_fields(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<Vec<smelt_hir::Field>> {
        if let Some(class) = self.class_by_symbol(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&class.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&class.fields, &substitutions));
        }
        if let Some(interface) = self.find_interface(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&interface.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&interface.fields, &substitutions));
        }
        self.type_alias_fields.get(&name).cloned()
    }

    /// Return whether a JSON parse target is a class-like shape whose fields are erased.
    fn is_erased_json_class_target(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
    }

    /// Lower direct TypeScript `Array.prototype.shift` calls.
    pub(super) fn list_shift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "shift" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array shift requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let ty = self.ctx.krate.types.intern(Type::Optional(element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListShift { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.pop` calls.
    pub(super) fn list_pop_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "pop" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array pop requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let ty = self.ctx.krate.types.intern(Type::Optional(*element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListPop { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.reverse` calls.
    pub(super) fn list_reverse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reverse" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reverse requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReverse { list },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.sort` calls with an optional comparator.
    pub(super) fn list_sort_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "sort" {
            return Ok(None);
        }
        let comparator_argument = match call.arguments.as_slice() {
            [] => None,
            [argument] => Some(argument),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array sort requires at most one comparator argument",
                ));
            }
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let comparator = if let Some(argument) = comparator_argument {
            let number_ty = self.ctx.krate.types.intern(Type::Float);
            // Use the body-fallback variant so a comparator whose body uses
            // statement forms the side-effect-free callback IR cannot represent
            // (e.g. a `for` loop scanning sort criteria) retries through full
            // closure-body lowering instead of failing the whole file.
            let callback = self.callback_argument_with_body_fallback(
                argument,
                &[element_ty, element_ty],
                number_ty,
                "array sort",
                body,
            )?;
            // A named/optional comparator lowered through an erased wrapper
            // returns `unknown`; the emitter coerces the comparison result
            // numerically, so only reject genuinely non-numeric returns.
            if callback.return_ty != number_ty
                && !matches!(
                    self.ctx.krate.types.get(callback.return_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                )
                && !self.erased_or_union_surface(callback.return_ty)
            {
                self.require_callback_ty(callback.return_ty, number_ty, call, "array sort")?;
            }
            Some(callback.expr)
        } else {
            None
        };
        if comparator.is_some() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSort {
                    list,
                    comparator,
                    key: None,
                    reverse: false,
                },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            self.ctx.krate.types.get(element_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array sort supports boolean, number, and string arrays for now",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSort {
                list,
                comparator,
                key: None,
                reverse: false,
            },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.push` calls.
    pub(super) fn list_push_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "push" {
            return Ok(None);
        }
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push currently requires at least one item argument",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        // `Array.prototype.push` accepts any number of arguments and appends each
        // in order, returning the new length. Each spread argument extends the
        // list, each scalar pushes one element; the value of the call is the last
        // mutation's `Float` length result.
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let mut result = None;
        for item_argument in &call.arguments {
            // Every push before the last is a sequenced side effect; the call's
            // value is the final push's `Float` length result.
            if let Some(previous) = result.take() {
                body.push_stmt(Stmt::Expr(previous));
            }
            let pushed = if let Argument::SpreadElement(spread) = item_argument {
                let other = self.expression(&spread.argument, body)?;
                let other_ty = Self::expr_ty(body, other);
                let other = if matches!(self.ctx.krate.types.get(other_ty), Some(Type::List(_))) {
                    other
                } else {
                    let list_value_ty = self.ctx.krate.types.intern(Type::List(element_ty));
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: other },
                        ty: list_value_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                };
                body.push_expr(Expr {
                    kind: ExprKind::ListExtend { list, other },
                    ty: number_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            } else {
                let mut item = self.argument(item_argument, body)?;
                let item_ty = Self::expr_ty(body, item);
                let compatible = self.array_item_type_compatible(item_ty, element_ty)
                    || self.ctx.krate.types.get(element_ty) == Some(&Type::Unknown)
                    || self.type_contains_unknown(item_ty)
                    || self.type_contains_unknown(element_ty)
                    || self.numeric_type_compatible(element_ty, item_ty);
                if !compatible {
                    if self.erased_or_union_surface(item_ty)
                        || self.erased_or_union_surface(element_ty)
                    {
                        item = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: item },
                            ty: element_ty,
                            span: self.span(item_argument.span().start, item_argument.span().end),
                        });
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            "array push argument must match the array element type",
                        ));
                    }
                }
                body.push_expr(Expr {
                    kind: ExprKind::ListPush { list, item },
                    ty: number_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            };
            result = Some(pushed);
        }
        Ok(result)
    }

    /// Lower modern TypeScript array APIs that materialize lists directly.
    pub(super) fn modern_array_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(
            method,
            "splice"
                | "toSpliced"
                | "fill"
                | "copyWithin"
                | "with"
                | "flat"
                | "flatMap"
                | "toSorted"
                | "toReversed"
                | "findLast"
                | "findLastIndex"
                | "keys"
                | "values"
                | "entries"
        ) {
            return Ok(None);
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let span = self.span(call.span.start, call.span.end);
        match method {
            "splice" | "toSpliced" => {
                let [start_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array splice requires a start argument",
                    ));
                };
                let start = self.slice_index_argument(start_arg, body)?;
                let delete_count = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                let item_args = if delete_count.is_some() {
                    rest.get(1..).unwrap_or(&[])
                } else {
                    rest
                };
                let mut items = Vec::with_capacity(item_args.len());
                for argument in item_args {
                    let (item, spread) = match argument {
                        Argument::SpreadElement(spread) => {
                            (self.expression(&spread.argument, body)?, true)
                        }
                        _ => (self.argument(argument, body)?, false),
                    };
                    let item_ty = Self::expr_ty(body, item);
                    let expected_ty = if spread { list_ty } else { element_ty };
                    if item_ty != expected_ty {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "array splice replacement items must match the array element type",
                        ));
                    }
                    items.push(ListSpliceItem {
                        value: item,
                        spread,
                    });
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListSplice {
                        list,
                        start,
                        delete_count,
                        items,
                        mutate: method == "splice",
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "fill" => {
                let [value_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array fill requires a value argument",
                    ));
                };
                if rest.len() > 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array fill supports value, start, and end arguments",
                    ));
                }
                let mut value = self.argument(value_arg, body)?;
                let value_ty = Self::expr_ty(body, value);
                if value_ty != element_ty {
                    // Coerce a fill value that is assignment-compatible with the
                    // element type (numeric widening, erased/union surfaces, type
                    // params) via a type assertion, instead of requiring an exact
                    // type match.
                    let compatible = self.array_item_type_compatible(value_ty, element_ty)
                        || self.ctx.krate.types.get(element_ty) == Some(&Type::Unknown)
                        || self.type_contains_unknown(value_ty)
                        || self.type_contains_unknown(element_ty)
                        || self.numeric_type_compatible(element_ty, value_ty)
                        || self.erased_or_union_surface(value_ty)
                        || self.erased_or_union_surface(element_ty)
                        || matches!(
                            (
                                self.ctx.krate.types.get(element_ty),
                                self.ctx.krate.types.get(value_ty)
                            ),
                            (Some(Type::TypeParam { .. }), _) | (_, Some(Type::TypeParam { .. }))
                        );
                    if compatible {
                        value = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value },
                            ty: element_ty,
                            span: self.span(value_arg.span().start, value_arg.span().end),
                        });
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(value_arg.span().start, value_arg.span().end),
                            "array fill value must match the array element type",
                        ));
                    }
                }
                let start = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                let end = rest
                    .get(1)
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListFill {
                        list,
                        value,
                        start,
                        end,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "copyWithin" => {
                let [target_arg, start_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array copyWithin requires target and start arguments",
                    ));
                };
                if rest.len() > 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array copyWithin supports target, start, and end arguments",
                    ));
                }
                let target = self.slice_index_argument(target_arg, body)?;
                let start = self.slice_index_argument(start_arg, body)?;
                let end = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCopyWithin {
                        list,
                        target,
                        start,
                        end,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "with" => {
                let [index_arg, value_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array with requires index and value arguments",
                    ));
                };
                let index = self.slice_index_argument(index_arg, body)?;
                let value = self.argument(value_arg, body)?;
                if Self::expr_ty(body, value) != element_ty {
                    return Err(SmeltError::unsupported(
                        self.span(value_arg.span().start, value_arg.span().end),
                        "array with value must match the array element type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListWith { list, index, value },
                    ty: list_ty,
                    span,
                })))
            }
            "flat" => {
                if call.arguments.len() > 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array flat accepts at most one depth argument",
                    ));
                }
                let depth = call
                    .arguments
                    .first()
                    .map(|argument| self.argument(argument, body))
                    .transpose()?;
                let flat_item_ty = match self
                    .ctx
                    .krate
                    .types
                    .get(self.type_param_constraint_or_self(element_ty))
                {
                    Some(Type::List(flat_item_ty)) => *flat_item_ty,
                    Some(Type::Tuple(items)) => self.flattened_tuple_item_type(items.clone()),
                    Some(Type::Unknown | Type::TypeParam { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => return Ok(None),
                };
                let ty = self.ctx.krate.types.intern(Type::List(flat_item_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListFlat { list, depth },
                    ty,
                    span,
                })))
            }
            "flatMap" => {
                let [callback_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array flatMap requires one callback argument",
                    ));
                };
                let index_ty = self.ctx.krate.types.intern(Type::Float);
                let callback = self.callback_argument(
                    callback_arg,
                    &[element_ty, index_ty, list_ty],
                    "array flatMap",
                    body,
                )?;
                let flat_item_ty = self
                    .flat_map_callback_item_type(callback.return_ty)
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(callback_arg.span().start, callback_arg.span().end),
                            "array flatMap callback must return an array or flattened item",
                        )
                    })?;
                let ty = self.ctx.krate.types.intern(Type::List(flat_item_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCallback {
                        op: ListCallbackOp::FlatMap,
                        list,
                        callback: callback.expr,
                    },
                    ty,
                    span,
                })))
            }
            "toSorted" => {
                let comparator_argument = match call.arguments.as_slice() {
                    [] => None,
                    [argument] => Some(argument),
                    _ => {
                        return Err(SmeltError::unsupported(
                            span,
                            "array toSorted requires at most one comparator argument",
                        ));
                    }
                };
                let comparator = if let Some(argument) = comparator_argument {
                    let callback = self.callback_argument(
                        argument,
                        &[element_ty, element_ty],
                        "array toSorted",
                        body,
                    )?;
                    let number_ty = self.ctx.krate.types.intern(Type::Float);
                    self.require_callback_ty(
                        callback.return_ty,
                        number_ty,
                        call,
                        "array toSorted",
                    )?;
                    Some(callback.expr)
                } else {
                    None
                };
                let sorted = body.push_expr(Expr {
                    kind: ExprKind::ListCopy { list },
                    ty: list_ty,
                    span,
                });
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListSort {
                        list: sorted,
                        comparator,
                        key: None,
                        reverse: false,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "toReversed" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        "array toReversed requires no arguments",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListReversed { list },
                    ty: list_ty,
                    span,
                })))
            }
            "findLast" | "findLastIndex" => {
                let [callback_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array findLast/findLastIndex requires one callback argument",
                    ));
                };
                let index_ty = self.ctx.krate.types.intern(Type::Float);
                // findLast/findLastIndex are truthiness predicates like
                // find/filter: a named callback returning an erased/`any`
                // value is coerced to bool by the truthy wrapper instead of
                // being rejected for not returning `boolean` literally.
                let callback = self.truthy_callback_argument_with_body_fallback(
                    callback_arg,
                    &[element_ty, index_ty, list_ty],
                    "array findLast/findLastIndex",
                    body,
                )?;
                let op = if method == "findLast" {
                    ListCallbackOp::FindLast
                } else {
                    ListCallbackOp::FindLastIndex
                };
                let ty = if method == "findLast" {
                    self.ctx.krate.types.intern(Type::Optional(element_ty))
                } else {
                    index_ty
                };
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCallback {
                        op,
                        list,
                        callback: callback.expr,
                    },
                    ty,
                    span,
                })))
            }
            "keys" | "values" | "entries" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        "array projection methods require no arguments",
                    ));
                }
                let op = match method {
                    "keys" => ListProjectionOp::Keys,
                    "values" => ListProjectionOp::Values,
                    "entries" => ListProjectionOp::Entries,
                    _ => return Ok(None),
                };
                let ty = match op {
                    ListProjectionOp::Keys => {
                        let int_ty = self.ctx.krate.types.intern(Type::Int);
                        self.ctx.krate.types.intern(Type::List(int_ty))
                    }
                    ListProjectionOp::Values => list_ty,
                    ListProjectionOp::Entries => {
                        let int_ty = self.ctx.krate.types.intern(Type::Int);
                        let tuple_ty = self
                            .ctx
                            .krate
                            .types
                            .intern(Type::Tuple(vec![int_ty, element_ty]));
                        self.ctx.krate.types.intern(Type::List(tuple_ty))
                    }
                };
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListProjection { op, list },
                    ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Infer the output item type for JavaScript `Array.prototype.flatMap`.
    fn flat_map_callback_item_type(
        &mut self,
        return_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(return_ty))
            .cloned()
        {
            Some(Type::List(item_ty)) => Some(item_ty),
            Some(Type::Tuple(items)) => Some(self.flattened_tuple_item_type(items)),
            Some(Type::Union(items)) => {
                let mut item_tys = Vec::new();
                for item in items {
                    match self
                        .ctx
                        .krate
                        .types
                        .get(self.type_param_constraint_or_self(item))
                    {
                        Some(Type::List(list_item)) if !item_tys.contains(list_item) => {
                            item_tys.push(*list_item);
                        }
                        Some(Type::Tuple(tuple_items)) => {
                            let tuple_item = self.flattened_tuple_item_type(tuple_items.clone());
                            if !item_tys.contains(&tuple_item) {
                                item_tys.push(tuple_item);
                            }
                        }
                        Some(Type::List(_) | Type::Never) => {}
                        _ if !item_tys.contains(&item) => item_tys.push(item),
                        _ => {}
                    }
                }
                match item_tys.as_slice() {
                    [] => None,
                    [single] => Some(*single),
                    _ => Some(self.ctx.krate.types.intern(Type::Union(item_tys))),
                }
            }
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                Some(self.ctx.krate.types.intern(Type::Unknown))
            }
            Some(_) | None => None,
        }
    }

    /// Collapse a tuple used as an array element into the item type produced by flattening it.
    pub(super) fn flattened_tuple_item_type(
        &mut self,
        items: Vec<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        match items.as_slice() {
            [] => self.ctx.krate.types.intern(Type::Never),
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(items)),
        }
    }

    /// Lower direct TypeScript `Array.prototype.unshift` calls.
    pub(super) fn list_unshift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "unshift" {
            return Ok(None);
        }
        let Expression::Identifier(_) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array unshift currently requires a local array receiver",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let mut items = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let item = self.argument(argument, body)?;
            let item_ty = Self::expr_ty(body, item);
            if !self.array_item_type_compatible(item_ty, element_ty)
                && !self.numeric_type_compatible(element_ty, item_ty)
            {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "array unshift arguments must match the array element type",
                ));
            }
            items.push(item);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListUnshift { list, items },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript collection and string slicing calls.
    pub(super) fn collection_slice_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "slice" | "substring" | "substr") {
            return Ok(None);
        }
        if call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring/substr currently support only omitted, start, and end arguments",
            ));
        }
        let mut operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let mut effective_operand_ty = self.type_param_constraint_or_self(operand_ty);
        if matches!(
            self.ctx.krate.types.get(effective_operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) || self.type_contains_unknown(effective_operand_ty)
            || matches!(
                self.ctx.krate.types.get(effective_operand_ty),
                Some(Type::Optional(inner)) if self.is_string_compatible_type(*inner)
            )
            || matches!(
                self.ctx.krate.types.get(effective_operand_ty),
                Some(Type::Union(items)) if items.iter().any(|item| self.is_string_compatible_type(*item))
            )
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            operand = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: operand },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
            effective_operand_ty = string_ty;
        }
        if method == "substring"
            && self.ctx.krate.types.get(effective_operand_ty) != Some(&Type::String)
        {
            return Ok(None);
        }
        let start = call
            .arguments
            .first()
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let mut end = call
            .arguments
            .get(1)
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        if method == "substr"
            && let (Some(start_expr), Some(length_expr)) = (start, end)
        {
            let ty = self.ctx.krate.types.intern(Type::Float);
            end = Some(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: start_expr,
                    rhs: length_expr,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }

        match self.ctx.krate.types.get(effective_operand_ty) {
            Some(Type::String) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringSlice {
                        operand,
                        start,
                        end,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            Some(Type::List(_)) if method == "slice" => Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSlice {
                    list: operand,
                    start,
                    end,
                },
                ty: operand_ty,
                span: self.span(call.span.start, call.span.end),
            }))),
            _ => Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring/substr requires a string receiver, or an array receiver for slice",
            )),
        }
    }

    /// Lower and validate a slice index argument.
    pub(super) fn slice_index_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index = self.argument(argument, body)?;
        if !self.slice_index_type_is_number(Self::expr_ty(body, index)) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "slice indexes must be numbers",
            ));
        }
        Ok(index)
    }

    /// Return whether a type can be used as a JavaScript slice index.
    pub(super) fn slice_index_type_is_number(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Optional(item)) => self.slice_index_type_is_number(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.slice_index_type_is_number(item)),
            _ => false,
        }
    }

    /// Return whether a value type is compatible with an array element type.
    pub(super) fn array_item_type_compatible(
        &mut self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, expected) {
            return true;
        }
        if self.erased_or_union_surface(actual) || self.erased_or_union_surface(expected) {
            return true;
        }
        if matches!(
            (
                self.ctx.krate.types.get(actual),
                self.ctx.krate.types.get(expected)
            ),
            (Some(Type::Union(_)), Some(Type::Union(_)))
        ) {
            return true;
        }
        let members = self.non_nullish_member_types(actual);
        !members.is_empty()
            && if matches!(self.ctx.krate.types.get(expected), Some(Type::Union(_))) {
                members
                    .into_iter()
                    .any(|item| self.type_assignable_to(item, expected))
            } else {
                members
                    .into_iter()
                    .all(|item| self.type_assignable_to(item, expected))
            }
    }

    /// Return union members after removing nullish wrappers from each member.
    fn non_nullish_member_types(&mut self, ty: smelt_hir::TypeId) -> Vec<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Union(items)) => items
                .into_iter()
                .flat_map(|item| self.non_nullish_member_types(item))
                .collect(),
            Some(Type::Optional(item)) => vec![item],
            Some(Type::None) => Vec::new(),
            _ => vec![ty],
        }
    }
}
