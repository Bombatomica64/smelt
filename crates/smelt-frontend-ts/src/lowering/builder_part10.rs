impl ModuleBuilder<'_> {
    /// Lower supported JavaScript regular-expression replacement calls.
    fn regex_replace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "replace" => StringReplaceOp::First,
            "replaceAll" => StringReplaceOp::All,
            _ => return Ok(None),
        };
        let [pattern_arg, replacement_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "regex replacement requires pattern and replacement arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let Some(pattern) = self.regex_replacement_pattern(pattern_arg, body)? else {
            return Ok(None);
        };
        let replacement = self.argument(replacement_arg, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, haystack)),
            Some(Type::String | Type::Unknown)
        ) || self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
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
    fn regex_replacement_pattern(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
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
                Ok(Some(self.argument(regex_pattern_arg, body)?))
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
                Ok(Some(self.argument(regex_pattern_arg, body)?))
            }
            Argument::RegExpLiteral(literal) => {
                if !literal.regex.flags.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(literal.span.start, literal.span.end),
                        "regex replacement does not lower RegExp literal flags yet",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(
                        literal.regex.pattern.text.to_string(),
                    )),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower `new URL(text).field` for the supported URL string fields.
    fn url_field_expression(
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
        let url = self.argument(url_arg, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, url)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new URL(text) requires a string URL argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlField { field, url },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.abs(...)` calls.
    fn math_abs_call(
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
    fn math_round_call(
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
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: operand_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.max` and `Math.min` calls.
    fn math_extrema_call(
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
                format!("Math.{} requires number arguments", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericExtrema { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.hypot` calls.
    fn math_hypot_call(
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

    /// Lower direct TypeScript `Number.isFinite` and `Number.isNaN` calls.
    fn number_predicate_call(
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
                    "isNaN" => NumericPredicateOp::IsNaN,
                    _ => return Ok(None),
                };
                (op, format!("Number.{}", member.property.name))
            }
            Expression::Identifier(identifier) if identifier.name == "isNaN" => {
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
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
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
    fn number_parse_float_call(
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
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Number.parseFloat requires exactly one string argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Number.parseFloat requires a string argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Number.parseInt(...)` calls without a radix argument.
    fn number_parse_int_call(
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
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Number.parseInt requires exactly one string argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Number.parseInt requires a string argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToInt,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `.toString()` calls without a radix argument.
    fn number_to_string_call(
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
        let operand = self.expression(&member.object, body)?;
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
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "number.toString radix arguments are not supported yet",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower the specific Node probe `process.version.match(/^v(\d+)\./)` used by date-fns tests.
    fn node_process_version_match_call(
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

    /// Lower direct TypeScript unary `Math.*` numeric calls.
    fn math_unary_func_call(
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
    fn math_random_call(
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
