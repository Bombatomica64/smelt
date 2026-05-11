impl ModuleBuilder<'_> {
    fn string_trim_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let side = match attr.attr.as_str() {
            "strip" => StringTrimSide::Both,
            "lstrip" => StringTrimSide::Start,
            "rstrip" => StringTrimSide::End,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "str strip/lstrip/rstrip argument-based trim is not supported yet",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                span,
                "str.strip() requires a str receiver",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringTrim { side, operand },
            ty,
            span,
        })))
    }

    /// Lower direct Python string prefix and suffix methods.
    fn string_affix_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "startswith" => StringAffixOp::StartsWith,
            "endswith" => StringAffixOp::EndsWith,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "str.startswith()/endswith() require exactly one argument",
            ));
        }
        let haystack = self.expression(&attr.value, body)?;
        let needle = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.startswith()/endswith() require str receiver and argument",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringAffix {
                op,
                haystack,
                needle,
            },
            ty,
            span,
        })))
    }

    /// Lower direct Python string search methods.
    fn string_search_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "find" => StringSearchOp::Find,
            "rfind" => StringSearchOp::RFind,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "str.find()/rfind() optional start/end arguments are not supported yet",
            ));
        }
        let haystack = self.expression(&attr.value, body)?;
        let needle = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.find()/rfind() require str receiver and argument",
            ));
        }
        let ty = self.intern_type(Type::Int);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringSearch {
                op,
                haystack,
                needle,
            },
            ty,
            span,
        })))
    }

    /// Lower direct Python literal string replacement.
    fn string_replace_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "replace" {
            return Ok(None);
        }
        let span = self.span(call.range);
        let [pattern_expr, replacement_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.replace() requires exactly pattern and replacement string arguments; count is not supported yet",
            ));
        };
        let haystack = self.expression(&attr.value, body)?;
        let pattern = self.expression(pattern_expr, body)?;
        let replacement = self.expression(replacement_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, replacement)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.replace() requires str receiver, pattern, and replacement",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringReplace {
                op: StringReplaceOp::All,
                haystack,
                pattern,
                replacement,
            },
            ty,
            span,
        })))
    }

    /// Lower direct Python remove-prefix and remove-suffix string methods.
    fn string_remove_affix_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "removeprefix" => StringAffixOp::StartsWith,
            "removesuffix" => StringAffixOp::EndsWith,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        let [affix_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.removeprefix()/removesuffix() require exactly one str argument",
            ));
        };
        let haystack = self.expression(&attr.value, body)?;
        let affix = self.expression(affix_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, affix)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.removeprefix()/removesuffix() require str receiver and argument",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringRemoveAffix {
                op,
                haystack,
                affix,
            },
            ty,
            span,
        })))
    }

    /// Lower direct Python string character predicates.
    fn string_predicate_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "isdigit" => StringPredicateOp::IsDigit,
            "isalpha" => StringPredicateOp::IsAlpha,
            "isalnum" => StringPredicateOp::IsAlnum,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "str.isdigit()/isalpha()/isalnum() require no arguments",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                span,
                "str.isdigit()/isalpha()/isalnum() require a str receiver",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringPredicate { op, operand },
            ty,
            span,
        })))
    }

    /// Lower direct Python string splitting.
    fn string_split_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "split" {
            return Ok(None);
        }
        let span = self.span(call.range);
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "str.split() requires exactly one separator argument",
            ));
        }
        let haystack = self.expression(&attr.value, body)?;
        let [separator_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.split() requires exactly one separator argument",
            ));
        };
        let separator = self.expression(separator_expr, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(separator_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.split() requires str receiver and separator",
            ));
        }
        let string_ty = self.intern_type(Type::String);
        let ty = self.intern_type(Type::List(string_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
            },
            ty,
            span,
        })))
    }

    /// Lower direct Python separator string join for lists of strings.
    fn string_join_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "join" {
            return Ok(None);
        }
        let span = self.span(call.range);
        let [items_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.join() requires exactly one list[str] argument",
            ));
        };
        let separator = self.expression(&attr.value, body)?;
        let items = self.expression(items_expr, body)?;
        let string_ty = self.intern_type(Type::String);
        if self.ctx.krate.types.get(Self::expr_ty(body, separator)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, items)) != Some(&Type::List(string_ty))
        {
            return Err(SmeltError::unsupported(
                span,
                "str.join() requires str receiver and list[str] argument",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span,
        })))
    }

    // Dict/module call helpers continue in `map.rs`.
}
