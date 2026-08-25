impl ModuleBuilder<'_> {
    /// Lower Python `json.dumps(value)` calls for JSON-compatible values.
    pub(super) fn json_dumps_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::PyJsonDumps) {
            return Ok(None);
        }
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "json" || attr.attr.as_str() != "dumps" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "json.dumps() currently supports exactly one value argument",
            ));
        }
        let value = self.expression(&call.arguments.args[0], body)?;
        if !self.is_json_serializable_type(Self::expr_ty(body, value)) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "json.dumps() value must be JSON-serializable",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::JsonStringify { value },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `json.loads(text)` calls with an annotated destination type.
    pub(super) fn json_loads_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::PyJsonLoads) {
            return Ok(None);
        }
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "json" || attr.attr.as_str() != "loads" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "json.loads() currently supports exactly one text argument",
            ));
        }
        let Some(ty) = type_hint else {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "json.loads() requires an annotated destination type",
            ));
        };
        if !self.is_json_serializable_type(ty) {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "json.loads() destination type must be JSON-compatible",
            ));
        }
        let text = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, text)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "json.loads() text argument must be a string",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::JsonParse { text },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `re.search`, `re.match`, and `re.fullmatch` calls to boolean regex matches.
    pub(super) fn re_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(rule) = stdlib_dispatch::call_rule(call) else {
            return Ok(None);
        };
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "re" {
            return Ok(None);
        }
        let op = match rule {
            RuleId::PyReSearch => RegexMatchOp::Search,
            RuleId::PyReMatch => RegexMatchOp::Match,
            RuleId::PyReFullMatch => RegexMatchOp::FullMatch,
            _ => return Ok(None),
        };
        if call.arguments.args.len() != 2 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "re.search/match/fullmatch currently require pattern and text arguments only",
            ));
        }
        let pattern = self.expression(&call.arguments.args[0], body)?;
        let haystack = self.expression(&call.arguments.args[1], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "re.search/match/fullmatch require string pattern and text arguments",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::RegexIsMatch {
                op,
                pattern,
                haystack,
            },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `re.sub` and `re.split` calls to regex-backed Rust operations.
    pub(super) fn re_expanded_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "re" {
            return Ok(None);
        }
        match attr.attr.as_str() {
            "sub" => {
                if call.arguments.args.len() != 3 || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "re.sub() currently supports pattern, replacement, and text arguments only",
                    ));
                }
                let pattern = self.expression(&call.arguments.args[0], body)?;
                let replacement = self.expression(&call.arguments.args[1], body)?;
                let haystack = self.expression(&call.arguments.args[2], body)?;
                self.require_string_exprs(&[pattern, replacement, haystack], body, call.range())?;
                let ty = self.intern_type(Type::String);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::RegexReplace {
                        op: StringReplaceOp::All,
                        pattern,
                        haystack,
                        replacement,
                    },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            "split" => {
                if call.arguments.args.len() != 2 || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "re.split() currently supports pattern and text arguments only",
                    ));
                }
                let pattern = self.expression(&call.arguments.args[0], body)?;
                let haystack = self.expression(&call.arguments.args[1], body)?;
                self.require_string_exprs(&[pattern, haystack], body, call.range())?;
                let str_ty = self.intern_type(Type::String);
                let ty = self.intern_type(Type::List(str_ty));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::RegexSplit { pattern, haystack },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower simple text-mode Python `open(...).read()` and `open(...).write(text)` calls.
    pub(super) fn file_io_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(method) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Call(open_call) = method.value.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(open_name) = open_call.func.as_ref() else {
            return Ok(None);
        };
        if open_name.id.as_str() != "open" {
            return Ok(None);
        }
        let path = self.open_path_argument(open_call, body)?;
        match method.attr.as_str() {
            "read" => {
                if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "open(...).read() currently supports no read size argument",
                    ));
                }
                let ty = self.intern_type(Type::String);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::FileReadText { path },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            "write" => {
                if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "open(...).write(text) requires exactly one text argument",
                    ));
                }
                let text = self.expression(&call.arguments.args[0], body)?;
                self.require_string_exprs(&[text], body, call.range())?;
                let ty = self.intern_type(Type::Int);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::FileWriteText { path, text },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower a small chrono-backed Python `datetime.datetime.*` subset.
    pub(super) fn datetime_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        match stdlib_dispatch::datetime_rule(call) {
            Some(RuleId::PyDateTimeNow) => {
                if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "datetime.datetime.now()/utcnow() currently support no arguments",
                    ));
                }
                let ty = self.intern_type(Type::String);
                let now = body.push_expr(HirExpr {
                    kind: ExprKind::DateNow,
                    ty: self.intern_type(Type::Int),
                    span: self.span(call.range()),
                });
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::DateToIsoString { timestamp_ms: now },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            Some(RuleId::PyDateTimeFromTimestamp) => {
                if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "datetime.datetime.fromtimestamp() requires one timestamp argument",
                    ));
                }
                let seconds = self.expression(&call.arguments.args[0], body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, seconds)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "datetime.datetime.fromtimestamp() requires a numeric timestamp",
                    ));
                }
                let thousand = body.push_expr(HirExpr {
                    kind: ExprKind::Literal(smelt_hir::Literal::Int(1000)),
                    ty: self.intern_type(Type::Int),
                    span: self.span(call.range()),
                });
                let timestamp_ms = body.push_expr(HirExpr {
                    kind: ExprKind::BinOp {
                        op: smelt_hir::BinOp::Mul,
                        lhs: seconds,
                        rhs: thousand,
                    },
                    ty: Self::expr_ty(body, seconds),
                    span: self.span(call.range()),
                });
                let ty = self.intern_type(Type::String);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::DateToIsoString { timestamp_ms },
                    ty,
                    span: self.span(call.range()),
                })))
            }
            Some(_) | None => Ok(None),
        }
    }

    /// Lower `urllib.parse.urlparse(url).field` for common string fields.
    pub(super) fn urlparse_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(field_attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let field = match field_attr.attr.as_str() {
            "geturl" => UrlField::Href,
            _ => return Ok(None),
        };
        let Expr::Call(parse_call) = field_attr.value.as_ref() else {
            return Ok(None);
        };
        let Some(url) = self.urlparse_url_argument(parse_call, body)? else {
            return Ok(None);
        };
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::UrlField { field, url },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower URL parse attribute access such as `urlparse(url).hostname`.
    pub(super) fn urlparse_attribute_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let field = match attr.attr.as_str() {
            "hostname" => UrlField::Hostname,
            "path" => UrlField::Pathname,
            "query" => UrlField::Search,
            "scheme" => UrlField::Protocol,
            _ => return Ok(None),
        };
        let Expr::Call(parse_call) = attr.value.as_ref() else {
            return Ok(None);
        };
        let Some(url) = self.urlparse_url_argument(parse_call, body)? else {
            return Ok(None);
        };
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::UrlField { field, url },
            ty,
            span: self.span(attr.range),
        })))
    }

    /// Extract and validate the path argument from text-mode `open(...)`.
    fn open_path_argument(
        &mut self,
        open_call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if open_call.arguments.args.is_empty() || open_call.arguments.args.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(open_call.range()),
                "open() currently supports path and optional text mode only",
            ));
        }
        if let Some(mode) = open_call.arguments.args.get(1)
            && !matches!(mode, Expr::StringLiteral(_))
        {
            return Err(SmeltError::unsupported(
                self.span(mode.range()),
                "open() mode must be a static text mode",
            ));
        }
        let path = self.expression(&open_call.arguments.args[0], body)?;
        self.require_string_exprs(&[path], body, open_call.range())?;
        Ok(path)
    }

    /// Extract URL text from `urllib.parse.urlparse(url)`.
    fn urlparse_url_argument(
        &mut self,
        parse_call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(parse_call.func.as_ref(), Expr::Attribute(_)) {
            return Ok(None);
        }
        if stdlib_dispatch::urlparse_field_rule(parse_call) != Some(RuleId::PyUrlparseField) {
            return Ok(None);
        }
        if parse_call.arguments.args.len() != 1 || !parse_call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(parse_call.range()),
                "urllib.parse.urlparse() currently requires exactly one URL argument",
            ));
        }
        let url = self.expression(&parse_call.arguments.args[0], body)?;
        self.require_string_exprs(&[url], body, parse_call.range())?;
        Ok(Some(url))
    }

    /// Ensure HIR expressions have string type.
    fn require_string_exprs(
        &self,
        exprs: &[smelt_hir::ExprId],
        body: &Body,
        range: ruff_text_size::TextRange,
    ) -> Result<(), SmeltError> {
        if exprs
            .iter()
            .all(|expr| self.ctx.krate.types.get(Self::expr_ty(body, *expr)) == Some(&Type::String))
        {
            Ok(())
        } else {
            Err(SmeltError::unsupported(
                self.span(range),
                "operation requires string arguments",
            ))
        }
    }

    // JSON type compatibility lives in `stdlib_types.rs`.
}
