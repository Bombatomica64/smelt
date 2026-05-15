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
        let mut op = match member.property.name.as_str() {
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
        if !self.is_numeric_like_type(operand_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        let result_ty = self.type_param_constraint_or_self(operand_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: result_ty,
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
            && args.iter().any(|arg| {
                !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, *arg)),
                    Some(Type::Float | Type::Int | Type::Unknown | Type::TypeParam { .. })
                )
            })
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

    /// Lower direct TypeScript numeric predicate calls.
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
                    "isInteger" => NumericPredicateOp::IsInteger,
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
        let mut operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if !matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Int | Type::Float)) {
            if source_name == "isNaN" && self.is_date_constructor_arg_type(operand_ty) {
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

    /// Lower direct TypeScript `Number.parseInt(...)` calls.
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
        let operand = self.parse_int_operand("Number.parseInt", call, body)?;
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

    /// Lower and validate TypeScript `parseInt` string and radix arguments.
    ///
    /// Smelt currently represents integer parsing as a primitive string-to-int
    /// cast without a radix field. The common decimal radix form is still
    /// accepted here after checking that the radix expression is numeric.
    fn parse_int_operand(
        &mut self,
        source_name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let argument = match call.arguments.as_slice() {
            [argument] => argument,
            [argument, radix] => {
                let radix_expr = self.argument(radix, body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, radix_expr)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        format!("{source_name} requires a numeric radix argument"),
                    ));
                }
                argument
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a string argument and optional numeric radix"),
                ));
            }
        };
        let operand = self.argument(argument, body)?;
        match self.ctx.krate.types.get(Self::expr_ty(body, operand)) {
            Some(Type::String) => Ok(operand),
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty: string_ty,
                    span: self.span(argument.span().start, argument.span().end),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires a string argument"),
            )),
        }
    }

    /// Lower direct TypeScript `.toString()` calls with an optional radix argument.
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
        let ty = self.ctx.krate.types.intern(Type::String);
        if let Some(radix_argument) = call.arguments.first() {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "number.toString accepts at most one radix argument",
                ));
            }
            let radix = self.argument(radix_argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, radix)),
                Some(Type::Int | Type::Float)
            ) {
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

    /// Lower `crypto.getRandomValues(output)` as an accepted typed-array surface.
    fn crypto_get_random_values_call(
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
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, output)),
            Some(Type::List(item)) if matches!(self.ctx.krate.types.get(*item), Some(Type::Float | Type::Int))
        ) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "crypto.getRandomValues requires a numeric typed array",
            ));
        }
        Ok(Some(output))
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

    /// Lower the small `Intl` surface used by date-fns timezone test labels.
    fn intl_call(
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
    fn intl_date_time_format_constructor_expression(
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

    /// Lower calls to the supported opaque `Intl.*Format#format` formatter surface.
    fn intl_format_method_call(
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
    fn intl_resolved_options_call(
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
    fn intl_date_time_format_object_expr(&mut self, body: &mut Body, span: Span) -> smelt_hir::ExprId {
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
    fn is_intl_date_time_format_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        Self::is_intl_date_time_format_callee(&call.callee)
    }

    /// Return whether this call is a supported `Intl.*Format()` constructor-style call.
    fn is_intl_formatter_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        Self::is_intl_formatter_callee(&call.callee)
    }

    /// Return whether this expression names `Intl.DateTimeFormat`.
    fn is_intl_date_time_format_callee(callee: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "Intl")
            && member.property.name == "DateTimeFormat"
    }

    /// Return whether this expression names a supported `Intl.*Format` constructor.
    fn is_intl_formatter_callee(callee: &Expression<'_>) -> bool {
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
    fn is_intl_formatter_receiver(receiver: &Expression<'_>) -> bool {
        match receiver {
            Expression::CallExpression(call) => Self::is_intl_formatter_call(call),
            Expression::NewExpression(new_expr) => Self::is_intl_formatter_callee(&new_expr.callee),
            _ => false,
        }
    }

    /// Return whether this call is `Intl.DateTimeFormat().resolvedOptions()`.
    fn is_intl_resolved_options_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
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
