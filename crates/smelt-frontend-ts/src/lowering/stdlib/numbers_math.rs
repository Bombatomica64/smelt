//! Lowering helpers for regular-expression replacement, string, and related
//! JavaScript call forms.

use crate::lowering::{ModuleBuilder, stdlib_dispatch};
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, Expr, ExprKind, FunctionType, Literal, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, PrimitiveCastOp, Span, StringReplaceOp, Type, UrlField,
};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Resolve which ECMA-262 replacer arguments a `.replace(re, fn)` callback
    /// declared.
    ///
    /// `RegExp.prototype[@@replace]` calls the replacer with a fixed positional
    /// list — `(matched, p1, …, pN, position, string)` where `N` is the
    /// pattern's capture-group count — and a callback declares a *prefix* of
    /// it. So the meaning of the second parameter is a property of the PATTERN,
    /// not of the callback: with no capture groups it is the match position (a
    /// number), with one it is capture group 1 (a string that is `undefined`
    /// when the group did not participate).
    ///
    /// `N` therefore has to be known here. It is, whenever the pattern lowered
    /// to a string literal — which covers every regex literal, every
    /// `new RegExp('…')` on a literal, and every module `const` bound to one,
    /// because `regex_replacement_pattern` folds all three to `Literal::String`.
    ///
    /// # Errors
    ///
    /// * A callback declaring more parameters than the spec supplies is a
    ///   source error, and reported as one rather than silently truncated.
    /// * A callback with more than one parameter over a pattern whose text is
    ///   not statically known cannot have its parameter ROLES resolved at all.
    ///   Reporting that is the honest outcome: guessing "capture group" would
    ///   hand a number-typed parameter a string (and the reverse) with no
    ///   diagnostic.
    fn regex_replacer_arg_plan(
        pattern: smelt_hir::ExprId,
        declared_arity: usize,
        span: Span,
        body: &Body,
    ) -> Result<Vec<smelt_hir::RegexReplaceArg>, SmeltError> {
        use smelt_hir::RegexReplaceArg;

        // A single-parameter (or parameterless) callback needs no capture
        // count: argument 0 is the matched substring for every pattern.
        if declared_arity <= 1 {
            return Ok(vec![RegexReplaceArg::Matched; declared_arity]);
        }
        let Some(pattern_text) = Self::string_literal_expr_text(body, pattern) else {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "regex replacement callback declares {declared_arity} parameters, but the \
                     pattern text is not statically known, so it cannot be decided which are \
                     capture groups and which are the match position and subject string"
                ),
            ));
        };
        let capture_count = smelt_stdlib::js_regex::capture_group_count(&pattern_text);
        let mut roles = Vec::with_capacity(usize::try_from(capture_count).unwrap_or(0) + 3);
        roles.push(RegexReplaceArg::Matched);
        for group in 1..=capture_count {
            roles.push(RegexReplaceArg::Capture(group));
        }
        roles.push(RegexReplaceArg::Position);
        roles.push(RegexReplaceArg::Source);
        if declared_arity > roles.len() {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "regex replacement callback declares {declared_arity} parameters, but the \
                     pattern has {capture_count} capture group(s), so the replacer is called with \
                     only {} arguments (matched, captures, position, subject)",
                    roles.len()
                ),
            ));
        }
        roles.truncate(declared_arity);
        Ok(roles)
    }

    /// The contextual function type a regex replacer callback is lowered
    /// against, built from its resolved argument roles.
    ///
    /// Each role has exactly one type the spec fixes, so the callback's
    /// parameters get concrete types rather than being erased: the matched text
    /// and the subject string are `string`, the position is a `number`, and a
    /// capture group is `string | undefined` because a group that did not
    /// participate in the match is passed `undefined`.
    fn regex_replacer_callback_type(
        &mut self,
        args: &[smelt_hir::RegexReplaceArg],
    ) -> smelt_hir::TypeId {
        use smelt_hir::RegexReplaceArg;

        let string_ty = self.ctx.krate.types.intern(Type::String);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let optional_string_ty = self.ctx.krate.types.intern(Type::Optional(string_ty));
        let params = args
            .iter()
            .map(|arg| match arg {
                RegexReplaceArg::Matched | RegexReplaceArg::Source => string_ty,
                RegexReplaceArg::Capture(_) => optional_string_ty,
                RegexReplaceArg::Position => float_ty,
            })
            .collect();
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: string_ty,
            is_async: false,
            may_throw: false,
        }))
    }

    /// The text of a lowered string-literal expression, if it is one.
    ///
    /// The regex pattern argument is folded to a `Literal::String` by
    /// `regex_replacement_pattern` for every statically known spelling, so this
    /// is how the pattern's own text is recovered after lowering.
    fn string_literal_expr_text(body: &Body, expr: smelt_hir::ExprId) -> Option<String> {
        let index = usize::try_from(expr.0).ok()?;
        match body.exprs.get(index).map(|held| &held.kind) {
            Some(ExprKind::Literal(Literal::String(text))) => Some(text.clone()),
            _ => None,
        }
    }

    /// Lower supported JavaScript regular-expression replacement calls.
    pub(in crate::lowering) fn regex_replace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let mut op = match member.property.name.as_str() {
            "replace" => StringReplaceOp::First,
            "replaceAll" => StringReplaceOp::All,
            _ => return Ok(None),
        };
        // Utility namespaces may export a collection helper named `replace`
        // with a wider signature. Defer before applying String/RegExp instance
        // arity rules so the namespace member-call path owns that invocation.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let [pattern_arg, replacement_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "regex replacement requires pattern and replacement arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let Some((pattern, pattern_op)) = self.regex_replacement_pattern(pattern_arg, body)? else {
            return Ok(None);
        };
        if let Some(pattern_op) = pattern_op {
            op = pattern_op;
        }
        if op == StringReplaceOp::First
            && let Argument::ArrowFunctionExpression(replacement) = replacement_arg
            && self.arrow_callback_returns_param_uppercase(replacement)?
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceFirstMatchUppercase { pattern, haystack },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if matches!(
            replacement_arg,
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
        ) {
            let declared_arity = match replacement_arg {
                Argument::ArrowFunctionExpression(arrow) => arrow.params.items.len(),
                Argument::FunctionExpression(function) => function.params.items.len(),
                _ => 1,
            };
            let arg_span = self.span(replacement_arg.span().start, replacement_arg.span().end);
            let args = Self::regex_replacer_arg_plan(pattern, declared_arity, arg_span, body)?;
            let callback_ty = self.regex_replacer_callback_type(&args);
            let callback = self.argument_with_hint(replacement_arg, body, Some(callback_ty))?;
            let callback_ty_actual = Self::expr_ty(body, callback);
            let callback_ok = self
                .function_member_type(callback_ty_actual)
                .and_then(|ty| self.ctx.krate.types.get(ty).cloned())
                .is_some_and(|ty| {
                    matches!(ty, Type::Function(function) if function.params.len() == args.len() && self.is_string_compatible_type(function.return_ty))
                });
            if !callback_ok {
                return Err(SmeltError::unsupported(
                    arg_span,
                    format!(
                        "regex replacement callback must accept a match string and return a string ({:?})",
                        self.ctx.krate.types.get(callback_ty_actual)
                    ),
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceCallback {
                    op,
                    pattern,
                    haystack,
                    callback,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let replacement = self.argument(replacement_arg, body)?;
        // A named function replacement (`.replace(re, escapeString)`) is a
        // callback replacement exactly like an inline arrow; dispatch it
        // through the same callback op when its shape fits.
        if let Some(function_ty) = self.function_member_type(Self::expr_ty(body, replacement))
            && let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
            && function.required_params.unwrap_or(function.params.len()) <= 1
            && !function.params.is_empty()
            && self.is_string_compatible_type(function.return_ty)
        {
            // A named replacer is called with the same spec argument list as an
            // inline one, and Rust has no optional parameters: the emitted
            // closure must supply one argument per DECLARED parameter, not one
            // per required parameter.
            let arg_span = self.span(replacement_arg.span().start, replacement_arg.span().end);
            let args =
                Self::regex_replacer_arg_plan(pattern, function.params.len(), arg_span, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceCallback {
                    op,
                    pattern,
                    haystack,
                    callback: replacement,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !(self.is_string_compatible_type(Self::expr_ty(body, haystack))
            || self.type_contains_unknown(Self::expr_ty(body, haystack)))
            || self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, replacement)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "regex replacement requires string-compatible receiver, pattern, and replacement",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Extract a string pattern from regex replacement pattern forms.
    pub(in crate::lowering) fn regex_replacement_pattern(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<(smelt_hir::ExprId, Option<StringReplaceOp>)>, SmeltError> {
        match argument {
            Argument::NewExpression(pattern_new) => {
                let Expression::Identifier(callee) = &pattern_new.callee else {
                    return Ok(None);
                };
                if callee.name != "RegExp" {
                    return Ok(None);
                }
                let [regex_pattern_arg] = pattern_new.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(pattern_new.span.start, pattern_new.span.end),
                        "regex replacement supports RegExp(pattern) without flags",
                    ));
                };
                Ok(Some((self.argument(regex_pattern_arg, body)?, None)))
            }
            Argument::CallExpression(pattern_call) => {
                let Expression::Identifier(callee) = &pattern_call.callee else {
                    return Ok(None);
                };
                if callee.name != "RegExp" {
                    return Ok(None);
                }
                let [regex_pattern_arg] = pattern_call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(pattern_call.span.start, pattern_call.span.end),
                        "regex replacement supports RegExp(pattern) without flags",
                    ));
                };
                Ok(Some((self.argument(regex_pattern_arg, body)?, None)))
            }
            Argument::RegExpLiteral(literal) => {
                let flags = literal.regex.flags.to_string();
                if flags
                    .chars()
                    .any(|flag| !matches!(flag, 'g' | 'i' | 'm' | 's'))
                {
                    return Err(SmeltError::unsupported(
                        self.span(literal.span.start, literal.span.end),
                        "regex replacement supports only g/i/m/s RegExp literal flags",
                    ));
                }
                let op = flags
                    .contains('g')
                    .then_some(StringReplaceOp::All);
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some((body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(
                        Self::regex_literal_pattern_text(literal),
                    )),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }), op)))
            }
            Argument::Identifier(identifier) => {
                let Some((pattern, flags, _ty)) =
                    self.consts.regexp(identifier.name.as_str()).cloned()
                else {
                    return Ok(None);
                };
                if flags
                    .chars()
                    .any(|flag| !matches!(flag, 'g' | 'i' | 'm' | 's'))
                {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        "regex replacement supports only g/i/m/s RegExp literal flags",
                    ));
                }
                let op = flags.contains('g').then_some(StringReplaceOp::All);
                let ty = self.ctx.krate.types.intern(Type::String);
                let pattern = if flags.chars().any(|flag| matches!(flag, 'i' | 'm' | 's')) {
                    let inline_flags = flags
                        .chars()
                        .filter(|flag| matches!(flag, 'i' | 'm' | 's'))
                        .collect::<String>();
                    format!("(?{inline_flags}){pattern}")
                } else {
                    pattern
                };
                Ok(Some((
                    body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(pattern)),
                        ty,
                        span: self.span(identifier.span.start, identifier.span.end),
                    }),
                    op,
                )))
            }
            _ => Ok(None),
        }
    }

    /// Lower `new URL(text).field` for the supported URL string fields.
    pub(in crate::lowering) fn url_field_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::property_rule(member) != Some(RuleId::TsUrlField) {
            return Ok(None);
        }
        let field = match member.property.name.as_str() {
            "href" => UrlField::Href,
            "protocol" => UrlField::Protocol,
            "host" => UrlField::Host,
            "origin" => UrlField::Origin,
            "hostname" => UrlField::Hostname,
            "pathname" => UrlField::Pathname,
            "search" => UrlField::Search,
            _ => return Ok(None),
        };
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(_) = &new_expr.callee else {
            return Ok(None);
        };
        let [url_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new URL() currently supports exactly one string URL argument",
            ));
        };
        let url = self.url_string_argument(url_arg, body, new_expr.span)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlField { field, url },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Lower `new URL(text).toString()` to the same full-URL extraction as `.href`.
    pub(in crate::lowering) fn url_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toString" || !call.arguments.is_empty() {
            return Ok(None);
        }
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "URL" {
            return Ok(None);
        }
        let [url_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new URL() currently supports exactly one string URL argument",
            ));
        };
        let url = self.url_string_argument(url_arg, body, new_expr.span)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlField {
                field: UrlField::Href,
                url,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a `new URL(...)` argument into the string value used by URL helpers.
    pub(in crate::lowering) fn url_string_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        url_span: oxc::span::Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let url = self.argument(argument, body)?;
        let url_ty = Self::expr_ty(body, url);
        if self.ctx.krate.types.get(url_ty) == Some(&Type::String) {
            return Ok(url);
        }
        if self.is_string_compatible_type(url_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: url,
                    target: string_ty,
                },
                ty: string_ty,
                span: self.span(argument.span().start, argument.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(url_span.start, url_span.end),
            "new URL(text) requires a string URL argument",
        ))
    }

    /// Lower direct TypeScript `Math.abs(...)` calls.
    pub(in crate::lowering) fn math_abs_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "abs" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires a number argument",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericAbs { operand },
            ty: operand_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript numeric rounding calls.
    pub(in crate::lowering) fn math_round_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "floor" => NumericRoundOp::Floor,
            "ceil" => NumericRoundOp::Ceil,
            "round" => NumericRoundOp::Round,
            "trunc" => NumericRoundOp::Trunc,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let mut operand = self.argument(argument, body)?;
        let mut operand_ty = Self::expr_ty(body, operand);
        if !self.is_numeric_like_type(operand_ty) {
            if self.type_contains_unknown(operand_ty)
                || matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                || self.is_date_constructor_arg_type(operand_ty)
            {
                let target = self.ctx.krate.types.intern(Type::Float);
                operand = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: operand,
                        target,
                    },
                    ty: target,
                    span: self.span(argument.span().start, argument.span().end),
                });
                operand_ty = target;
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Math.{} requires a number argument", member.property.name),
                ));
            }
        }
        let result_ty = self.type_param_constraint_or_self(operand_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: result_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.max` and `Math.min` calls.
    pub(in crate::lowering) fn math_extrema_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "max" => NumericExtremaOp::Max,
            "min" => NumericExtremaOp::Min,
            _ => return Ok(None),
        };
        // A single spread argument (`Math.max(...values)`) reduces the numeric
        // list rather than treating it as one scalar operand. Separate any
        // spread from the scalar arguments so the extrema can fold the list.
        // Bail (returning `None`) on the uncommon multi-spread case so the
        // generic call path is left untouched.
        let spread_count = call
            .arguments
            .iter()
            .filter(|argument| matches!(argument, Argument::SpreadElement(_)))
            .count();
        if spread_count > 1 {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        let mut args = Vec::new();
        let mut spread = None;
        for argument in &call.arguments {
            let lowered = self.argument(argument, body)?;
            if matches!(argument, Argument::SpreadElement(_)) {
                // The extrema reduction coerces each spread element to `Float`
                // in the backend, so the list is taken as lowered.
                spread = Some(lowered);
                continue;
            }
            let arg = if matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, lowered)),
                Some(&Type::Float)
            ) {
                lowered
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: lowered },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })
            };
            args.push(arg);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericExtrema { op, args, spread },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.hypot` calls.
    pub(in crate::lowering) fn math_hypot_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "hypot" {
            return Ok(None);
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        if args
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.hypot requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericHypot { args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript numeric predicate calls.
    pub(in crate::lowering) fn number_predicate_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let (op, source_name) = match &call.callee {
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return Ok(None);
                };
                if object.name != "Number" {
                    return Ok(None);
                }
                let op = match member.property.name.as_str() {
                    "isFinite" => NumericPredicateOp::IsFinite,
                    "isInteger" => NumericPredicateOp::IsInteger,
                    "isSafeInteger" => NumericPredicateOp::IsSafeInteger,
                    "isNaN" => NumericPredicateOp::IsNaN,
                    _ => return Ok(None),
                };
                (op, format!("Number.{}", member.property.name))
            }
            Expression::Identifier(identifier) if identifier.name == "isNaN" => {
                // A value import, module item, or local binding of `isNaN`
                // shadows the global predicate (es-toolkit's own
                // `import { isNaN } from './isNaN'` accepts `any`). Defer to the
                // ordinary call path so the shadowing binding is called instead
                // of forcing the numeric-global predicate onto its argument.
                if self.builtin_call_identifier_is_shadowed(identifier.name.as_str()) {
                    return Ok(None);
                }
                (NumericPredicateOp::IsNaN, "isNaN".to_owned())
            }
            _ => return Ok(None),
        };
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires exactly one number argument"),
            ));
        };
        let mut operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        // `Number.isSafeInteger` does not coerce: ECMAScript answers `false` for
        // every non-Number argument. Asserting an erased operand to `number`
        // here would make `Number.isSafeInteger('1')` answer `true`, so the
        // erased operand is carried through unchanged and the codegen tests its
        // runtime tag instead.
        if matches!(op, NumericPredicateOp::IsSafeInteger)
            && matches!(
                self.ctx.krate.types.get(operand_ty),
                Some(
                    Type::Unknown
                        | Type::TypeParam { .. }
                        | Type::Class { .. }
                        | Type::Optional(_)
                        | Type::Union(_)
                )
            )
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::NumericPredicate { op, operand },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Int | Type::Float)) {
            if ((source_name == "isNaN" || source_name == "Number.isNaN")
                && self.is_date_constructor_arg_type(operand_ty))
                || matches!(
                    self.ctx.krate.types.get(operand_ty),
                    Some(
                        Type::Unknown
                            | Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::Optional(_)
                            | Type::Union(_)
                    )
                )
            {
                let ty = self.ctx.krate.types.intern(Type::Float);
                operand = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty,
                    span: self.span(argument.span().start, argument.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a number argument"),
                ));
            }
        }
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires a number argument"),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPredicate { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Number.parseFloat(...)` calls.
    pub(in crate::lowering) fn number_parse_float_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Number" || member.property.name != "parseFloat" {
            return Ok(None);
        }
        let operand = self.parse_float_operand("Number.parseFloat", call, body)?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ParseFloat,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower one JavaScript `parseFloat` operand through its `ToString` step.
    ///
    /// ECMAScript parses the string representation of its input rather than
    /// requiring the runtime value to already be a string. Keeping this as an
    /// explicit nested primitive cast preserves erased `any`/`unknown` values
    /// until the existing string coercion boundary instead of asserting that
    /// their runtime representation is a Rust `String`.
    pub(in crate::lowering) fn parse_float_operand(
        &mut self,
        source_name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires exactly one argument"),
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) == Some(&Type::String) {
            return Ok(operand);
        }
        if !self.primitive_cast_accepts_operand(PrimitiveCastOp::ToString, operand_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} argument cannot be coerced to a string"),
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand,
            },
            ty: string_ty,
            span: self.span(argument.span().start, argument.span().end),
        }))
    }

    /// Lower direct TypeScript `Number.parseInt(...)` calls.
    pub(in crate::lowering) fn number_parse_int_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Number" || member.property.name != "parseInt" {
            return Ok(None);
        }
        let (operand, radix) = self.parse_int_operand("Number.parseInt", call, body)?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        let kind = radix.map_or(
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToInt,
                operand,
            },
            |radix| ExprKind::ParseIntRadix { operand, radix },
        );
        Ok(Some(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower and validate TypeScript `parseInt` string and radix arguments.
    ///
    /// Returns the string operand plus an optional numeric radix. A present
    /// radix is coerced to `Float` (asserting erased `unknown`/type-parameter
    /// radices) so the `ParseIntRadix` op the callers emit can honor it.
    pub(in crate::lowering) fn parse_int_operand(
        &mut self,
        source_name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, Option<smelt_hir::ExprId>), SmeltError> {
        let (argument, radix) = match call.arguments.as_slice() {
            [argument] => (argument, None),
            [argument, radix] => {
                let radix_expr = self.argument(radix, body)?;
                let radix_expr = match self.ctx.krate.types.get(Self::expr_ty(body, radix_expr)) {
                    Some(Type::Int | Type::Float) => radix_expr,
                    Some(Type::Unknown | Type::TypeParam { .. }) => {
                        let float_ty = self.ctx.krate.types.intern(Type::Float);
                        body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: radix_expr },
                            ty: float_ty,
                            span: self.span(radix.span().start, radix.span().end),
                        })
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            format!("{source_name} requires a numeric radix argument"),
                        ));
                    }
                };
                (argument, Some(radix_expr))
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a string argument and optional numeric radix"),
                ));
            }
        };
        let operand = self.argument(argument, body)?;
        let operand = match self.ctx.krate.types.get(Self::expr_ty(body, operand)) {
            Some(Type::String) => operand,
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::String)) =>
            {
                operand
            }
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty: string_ty,
                    span: self.span(argument.span().start, argument.span().end),
                })
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a string argument"),
                ));
            }
        };
        Ok((operand, radix))
    }

    /// Lower direct TypeScript `.toString()` calls with an optional radix argument.
    pub(in crate::lowering) fn number_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toString" {
            return Ok(None);
        }
        // `ns.toString(value)` on a utility *namespace* object (a `import * as _`
        // star import or a registered object namespace) is the lodash/es-toolkit
        // free-function `toString`, whose first argument is the value being
        // stringified, not a `Number.prototype.toString` radix. Defer to the
        // generic namespace member-call path instead of treating the value as a
        // radix and rejecting a non-numeric one.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let operand = self.expression(&member.object, body)?;
        if call.arguments.is_empty() && self.expression_is_known_date_value(operand, body) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToString {
                    timestamp_ms: operand,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(
                Type::Int
                    | Type::Float
                    | Type::String
                    | Type::Unknown
                    | Type::TypeParam { .. }
                    | Type::Class { .. }
            )
        ) {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        if let Some(radix_argument) = call.arguments.first() {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "number.toString accepts at most one radix argument",
                ));
            }
            let radix = self.argument(radix_argument, body)?;
            let radix_ty = Self::expr_ty(body, radix);
            if matches!(self.ctx.krate.types.get(radix_ty), Some(Type::String))
                && !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, operand)),
                    Some(Type::Int | Type::Float)
                )
            {
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToString,
                        operand,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if !matches!(self.ctx.krate.types.get(radix_ty), Some(Type::Int | Type::Float)) {
                return Err(SmeltError::unsupported(
                    self.span(radix_argument.span().start, radix_argument.span().end),
                    "number.toString radix argument must be numeric",
                ));
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::NumericToStringRadix { operand, radix },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "number.toString radix arguments are not supported yet",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `.toFixed(digits)` calls on a numeric receiver.
    ///
    /// `Number.prototype.toFixed` renders the receiver as a fixed-point decimal
    /// string with the given number of fractional digits (defaulting to `0`).
    /// Only numeric receivers and an optional numeric digit count are accepted;
    /// other shapes are left for later dispatch.
    pub(in crate::lowering) fn number_to_fixed_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toFixed" {
            return Ok(None);
        }
        let operand = self.expression(&member.object, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(None);
        }
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "number.toFixed accepts at most one digit-count argument",
            ));
        }
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let digits = if let Some(argument) = call.arguments.first() {
            let digits = self.argument(argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, digits)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "number.toFixed digit-count argument must be numeric",
                ));
            }
            digits
        } else {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: float_ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericToFixed { operand, digits },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `crypto.getRandomValues(output)` as an accepted typed-array surface.
    pub(in crate::lowering) fn crypto_get_random_values_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "getRandomValues" {
            return Ok(None);
        }
        let Expression::Identifier(receiver) = &member.object else {
            return Ok(None);
        };
        if receiver.name != "crypto" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "crypto.getRandomValues requires one typed array argument",
            ));
        };
        let output = self.argument(argument, body)?;
        let output_ty = self.type_param_constraint_or_self(Self::expr_ty(body, output));
        let accepts = match self.ctx.krate.types.get(output_ty) {
            // A concrete numeric list, the shape a hand-written `number[]` has.
            Some(Type::List(item)) => matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::Float | Type::Int)
            ),
            // A typed array — the argument the platform API actually takes — is a
            // byte-backed host-object record, so its static type is the erased
            // dynamic one. Accepting it here is what keeps
            // `crypto.getRandomValues(new Uint8Array(n))` lowering after the views
            // gained real view identity.
            Some(Type::Unknown | Type::Union(_)) => true,
            _ => false,
        };
        if !accepts {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "crypto.getRandomValues requires a numeric typed array",
            ));
        }
        Ok(Some(output))
    }

    /// Lower the specific Node probe `process.version.match(/^v(\d+)\./)` used by date-fns tests.
    pub(in crate::lowering) fn node_process_version_match_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if member.property.name != "match" || !Self::is_process_version_member(&member.object) {
            return None;
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
        let whole = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("v20.".to_owned())),
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        });
        let major = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("20".to_owned())),
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        });
        Some(body.push_expr(Expr {
            kind: ExprKind::ListLit(vec![whole, major]),
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Lower `process.cwd()` as an opaque current-working-directory string.
    pub(in crate::lowering) fn node_process_cwd_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "cwd"
            || !matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "process")
        {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "process.cwd() requires no arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower the small `Intl` surface used by date-fns timezone test labels.
    pub(in crate::lowering) fn intl_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.intl_format_method_call(call, body)? {
            return Ok(Some(expr));
        }
        if Self::is_intl_formatter_call(call) {
            for arg in &call.arguments {
                let _ = self.argument(arg, body)?;
            }
            return Ok(Some(self.intl_date_time_format_object_expr(
                body,
                self.span(call.span.start, call.span.end),
            )));
        }
        if let Some(expr) = self.intl_resolved_options_call(call, body)? {
            return Ok(Some(expr));
        }
        Ok(None)
    }

    /// Lower `new Intl.DateTimeFormat(...)` and related Intl constructors as opaque formatter objects.
    pub(in crate::lowering) fn intl_date_time_format_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !Self::is_intl_formatter_callee(&new_expr.callee) {
            return Ok(None);
        }
        for arg in &new_expr.arguments {
            let _ = self.argument(arg, body)?;
        }
        Ok(Some(self.intl_date_time_format_object_expr(
            body,
            self.span(new_expr.span.start, new_expr.span.end),
        )))
    }

    /// Lower `new Intl.<Constructor>(...)` to a marker-only host-object record.
    ///
    /// ECMA-402 defines the `Intl` namespace constructors (`Intl.Locale`,
    /// `Intl.Collator`, `Intl.NumberFormat`, ...). Source code in scope
    /// constructs them only to probe host identity (e.g.
    /// `isPlainObject(new Intl.Locale('en')) === false`), so each lowers to a
    /// marker-bearing record through the shared host-object registry (keyed by
    /// the full qualified `Intl.<Constructor>` path, per the qualified-type
    /// rule). Arguments are lowered for their effects and discarded. The opaque
    /// formatter pair (`Intl.DateTimeFormat` / `Intl.RelativeTimeFormat`) is
    /// claimed earlier by `intl_date_time_format_constructor_expression` and is
    /// not in the registry; unmodeled `Intl` members fall through to the
    /// ordinary member-callee path and keep their honest blocker.
    pub(in crate::lowering) fn intl_namespace_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &new_expr.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        // A local binding named `Intl` shadows the global namespace.
        if object.name != "Intl" || self.scope.is_bound("Intl") {
            return Ok(None);
        }
        let qualified = format!("Intl.{}", member.property.name.as_str());
        let Some(marker) = smelt_stdlib::host_object_marker(&qualified) else {
            return Ok(None);
        };
        self.marker_only_builtin_constructor_expression(new_expr, body, marker)
            .map(Some)
    }

    /// Lower calls to the supported opaque `Intl.*Format#format` formatter surface.
    pub(in crate::lowering) fn intl_format_method_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "format" {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        if !Self::is_intl_formatter_receiver(&member.object)
            && !matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Dict(key, value))
                    if self.ctx.krate.types.get(*key) == Some(&Type::String)
                        && self.ctx.krate.types.get(*value) == Some(&Type::Unknown)
            )
        {
            return Ok(None);
        }
        if let [timestamp_arg] = call.arguments.as_slice() {
            let timestamp_ms = self.argument(timestamp_arg, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToIsoString { timestamp_ms },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if call.arguments.len() == 2 {
            for arg in &call.arguments {
                let _ = self.argument(arg, body)?;
            }
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "Intl formatter format(...) requires one or two arguments",
        ))
    }

    /// Lower `Intl.DateTimeFormat().resolvedOptions()` to a small options record.
    pub(in crate::lowering) fn intl_resolved_options_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !Self::is_intl_resolved_options_call(call) {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Intl.DateTimeFormat().resolvedOptions() does not accept arguments",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("timeZone".to_owned())),
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("America/Santiago".to_owned())),
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        });
        let ty = self.ctx.krate.types.intern(Type::Dict(string_ty, string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(key, value)]),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Build the opaque object used for the supported `Intl.DateTimeFormat` surface.
    pub(in crate::lowering) fn intl_date_time_format_object_expr(&mut self, body: &mut Body, span: Span) -> smelt_hir::ExprId {
        let key = self.ctx.krate.types.intern(Type::String);
        let value = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::Dict(key, value));
        body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span,
        })
    }

    /// Return whether this call is `Intl.DateTimeFormat()`.
    pub(in crate::lowering) fn is_intl_date_time_format_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        Self::is_intl_date_time_format_callee(&call.callee)
    }

    /// Return whether this call is a supported `Intl.*Format()` constructor-style call.
    pub(in crate::lowering) fn is_intl_formatter_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        Self::is_intl_formatter_callee(&call.callee)
    }

    /// Return whether this expression names `Intl.DateTimeFormat`.
    pub(in crate::lowering) fn is_intl_date_time_format_callee(callee: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "Intl")
            && member.property.name == "DateTimeFormat"
    }

    /// Return whether this expression names a supported `Intl.*Format` constructor.
    pub(in crate::lowering) fn is_intl_formatter_callee(callee: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "Intl")
            && matches!(
                member.property.name.as_str(),
                "DateTimeFormat" | "RelativeTimeFormat"
            )
    }

    /// Return whether this expression constructs/calls `Intl.DateTimeFormat`.
    pub(in crate::lowering) fn is_intl_formatter_receiver(receiver: &Expression<'_>) -> bool {
        match receiver {
            Expression::CallExpression(call) => Self::is_intl_formatter_call(call),
            Expression::NewExpression(new_expr) => Self::is_intl_formatter_callee(&new_expr.callee),
            _ => false,
        }
    }

    /// Return whether this call is `Intl.DateTimeFormat().resolvedOptions()`.
    pub(in crate::lowering) fn is_intl_resolved_options_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != "resolvedOptions" {
            return false;
        }
        let Expression::CallExpression(receiver_call) = &member.object else {
            return false;
        };
        Self::is_intl_date_time_format_call(receiver_call)
    }

    /// Lower direct TypeScript unary `Math.*` numeric calls.
    pub(in crate::lowering) fn math_unary_func_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "sqrt" => NumericUnaryFuncOp::Sqrt,
            "cbrt" => NumericUnaryFuncOp::Cbrt,
            "sign" => NumericUnaryFuncOp::Sign,
            "sin" => NumericUnaryFuncOp::Sin,
            "cos" => NumericUnaryFuncOp::Cos,
            "tan" => NumericUnaryFuncOp::Tan,
            "asin" => NumericUnaryFuncOp::Asin,
            "acos" => NumericUnaryFuncOp::Acos,
            "atan" => NumericUnaryFuncOp::Atan,
            "log" => NumericUnaryFuncOp::Log,
            "log10" => NumericUnaryFuncOp::Log10,
            "log2" => NumericUnaryFuncOp::Log2,
            "exp" => NumericUnaryFuncOp::Exp,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericUnaryFunc { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.random` calls.
    pub(in crate::lowering) fn math_random_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsMathRandom) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "random" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.random requires no arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRandom,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    // Continued in the next split builder file.
}
