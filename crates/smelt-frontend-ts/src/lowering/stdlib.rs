//! Focused TypeScript standard-library lowering helpers.

use oxc::ast::ast::{Argument, CallExpression, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, ListCallbackOp, ListProjectionOp, RegexMatchOp, Type};
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

        if let Some(target_ty) = self.object_assign_callable_target_type(target_arg, body) {
            let target = self.argument(target_arg, body)?;
            let props = self.object_assign_callable_props(source_args, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::CallableObjectAssign {
                    callable: target,
                    props,
                },
                ty: target_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let mut sources = Vec::with_capacity(source_args.len());
        let mut record_ty = None;
        for source_arg in source_args {
            let source = self.argument_with_hint(source_arg, body, record_ty)?;
            let source_ty = Self::expr_ty(body, source);
            if !matches!(self.ctx.krate.types.get(source_ty), Some(Type::Dict(_, _))) {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Object.assign sources must be record values",
                ));
            }
            if let Some(expected_ty) = record_ty {
                if source_ty != expected_ty {
                    return Err(SmeltError::unsupported(
                        self.span(source_arg.span().start, source_arg.span().end),
                        "Object.assign sources must share the target record type",
                    ));
                }
            } else {
                record_ty = Some(source_ty);
            }
            sources.push(source);
        }

        let Some(record_ty) = record_ty else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires record-typed arguments",
            ));
        };
        let target = self.argument_with_hint(target_arg, body, Some(record_ty))?;
        if Self::expr_ty(body, target) != record_ty {
            return Err(SmeltError::unsupported(
                self.span(target_arg.span().start, target_arg.span().end),
                "Object.assign target must share the source record type",
            ));
        }

        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictAssign { target, sources },
            ty: record_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the existing callable target type for supported `Object.assign` targets.
    fn object_assign_callable_target_type(
        &self,
        target_arg: &Argument<'_>,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let Argument::Identifier(ident) = target_arg else {
            return None;
        };
        if let Some(local) = self.locals.get(ident.name.as_str()).copied() {
            let ty = Self::local_ty(body, local);
            let resolved = self.type_param_constraint_or_self(ty);
            if matches!(
                self.ctx.krate.types.get(resolved),
                Some(Type::Function(_) | Type::Class { .. })
            ) {
                return Some(ty);
            }
        }
        None
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
                    return_ty: unknown,
                    is_async: false,
                }))
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(smelt_hir::Literal::None),
            ty: function_ty,
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
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.stringify() currently supports exactly one value argument",
            ));
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
        let Some(type_args) = &call.type_arguments else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse requires an explicit type argument",
            ));
        };
        let [target_ty] = type_args.params.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse requires exactly one type argument",
            ));
        };
        let ty = self.ts_type_to_hir(target_ty)?;
        if !self.is_json_serializable_type(ty) {
            return Err(SmeltError::unsupported(
                self.span(target_ty.span().start, target_ty.span().end),
                "JSON.parse<T>() target type must be JSON-compatible",
            ));
        }
        let text = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, text)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "JSON.parse<T>() text argument must be a string",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse { text },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
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
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires exactly one string argument",
            ));
        };
        let Some(pattern) =
            self.regexp_pattern_expression(&member.object, body, is_known_regexp_test)?
        else {
            return Ok(None);
        };
        let haystack = self.argument(haystack_argument, body)?;
        let Some(haystack) = self.regexp_text_operand(haystack, body) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires a string haystack",
            ));
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
    fn regexp_pattern_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        require_regexp_receiver: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        match expression {
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                };
                if callee.name != "RegExp" {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                }
                let [pattern_argument] = new_expr.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp construction currently requires exactly one string pattern argument",
                    ));
                };
                let pattern = self.argument(pattern_argument, body)?;
                self.regexp_pattern_operand(pattern, body)
            }
            Expression::CallExpression(call_expr) => {
                let Expression::Identifier(callee) = &call_expr.callee else {
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                };
                if callee.name != "RegExp" {
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp.test() requires a RegExp receiver",
                    ));
                }
                let [pattern_argument] = call_expr.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call_expr.span.start, call_expr.span.end),
                        "RegExp construction currently requires exactly one string pattern argument",
                    ));
                };
                let pattern = self.argument(pattern_argument, body)?;
                self.regexp_pattern_operand(pattern, body)
            }
            Expression::RegExpLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(smelt_hir::Literal::String(
                        Self::regex_literal_pattern_text(literal),
                    )),
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
    fn is_json_serializable_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
                self.is_json_serializable_type(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.is_json_serializable_type(*item)),
            Some(Type::Dict(key, value)) => {
                matches!(self.ctx.krate.types.get(*key), Some(Type::String))
                    && self.is_json_serializable_type(*value)
            }
            _ => false,
        }
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
            let callback = self.arrow_callback(argument, &[element_ty, element_ty], body)?;
            let number_ty = self.ctx.krate.types.intern(Type::Float);
            self.require_callback_ty(callback.ty, number_ty, call, "array sort")?;
            Some(callback)
        } else {
            None
        };
        if comparator.is_some() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSort { list, comparator },
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
            kind: ExprKind::ListSort { list, comparator },
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
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push currently supports exactly one item argument",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.argument(item_argument, body)?;
        let item_ty = Self::expr_ty(body, item);
        let compatible = item_ty == element_ty
            || self.ctx.krate.types.get(element_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(item_ty)
            || self.type_contains_unknown(element_ty)
            || self.numeric_type_compatible(element_ty, item_ty)
            || matches!(
                (
                    self.ctx.krate.types.get(element_ty),
                    self.ctx.krate.types.get(item_ty)
                ),
                (Some(Type::TypeParam { .. }), _) | (_, Some(Type::TypeParam { .. }))
            );
        if !compatible {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListPush { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
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
                    let item = self.argument(argument, body)?;
                    if Self::expr_ty(body, item) != element_ty {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "array splice replacement items must match the array element type",
                        ));
                    }
                    items.push(item);
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
                let value = self.argument(value_arg, body)?;
                if Self::expr_ty(body, value) != element_ty {
                    return Err(SmeltError::unsupported(
                        self.span(value_arg.span().start, value_arg.span().end),
                        "array fill value must match the array element type",
                    ));
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
                        "array flat supports depth 0 or 1",
                    ));
                }
                let flat_item_ty = match self
                    .ctx
                    .krate
                    .types
                    .get(self.type_param_constraint_or_self(element_ty))
                {
                    Some(Type::List(flat_item_ty)) => *flat_item_ty,
                    Some(Type::Unknown | Type::TypeParam { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => return Ok(None),
                };
                let ty = self.ctx.krate.types.intern(Type::List(flat_item_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListFlat { list },
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
                    let callback =
                        self.arrow_callback(argument, &[element_ty, element_ty], body)?;
                    let number_ty = self.ctx.krate.types.intern(Type::Float);
                    self.require_callback_ty(callback.ty, number_ty, call, "array toSorted")?;
                    Some(callback)
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
                let callback = self.callback_argument(
                    callback_arg,
                    &[element_ty, index_ty, list_ty],
                    "array findLast/findLastIndex",
                    body,
                )?;
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                let context = if method == "findLast" {
                    "array findLast"
                } else {
                    "array findLastIndex"
                };
                self.require_callback_ty(callback.return_ty, bool_ty, call, context)?;
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
            if Self::expr_ty(body, item) != element_ty {
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

    /// Lower direct TypeScript `Array.prototype.slice`, `String.prototype.slice`, and
    /// positive-bound `String.prototype.substring` calls.
    pub(super) fn collection_slice_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "slice" | "substring") {
            return Ok(None);
        }
        if call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring currently support only omitted, start, and end arguments",
            ));
        }
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let effective_operand_ty = self.type_param_constraint_or_self(operand_ty);
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
        let end = call
            .arguments
            .get(1)
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;

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
                "slice/substring requires a string receiver, or an array receiver for slice",
            )),
        }
    }

    /// Lower and validate a slice index argument.
    fn slice_index_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index = self.argument(argument, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, index)),
            Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. })
        ) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "slice indexes must be numbers",
            ));
        }
        Ok(index)
    }
}
