impl ModuleBuilder<'_> {
    /// Lower supported Python string trim calls into HIR string runtime calls.
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
        let span = self.span(call.range());
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "str strip/lstrip/rstrip argument-based trim is not supported yet",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::PyStr) {
            return Err(SmeltError::unsupported(
                span,
                "str.strip() requires a str receiver",
            ));
        }
        let ty = self.intern_type(Type::PyStr);
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
        let span = self.span(call.range());
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "str.startswith()/endswith() require exactly one argument",
            ));
        }
        let haystack = self.expression(&attr.value, body)?;
        let needle = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::PyStr)
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
        let span = self.span(call.range());
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "str.find()/rfind() optional start/end arguments are not supported yet",
            ));
        }
        let haystack = self.expression(&attr.value, body)?;
        let needle = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::PyStr)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.find()/rfind() require str receiver and argument",
            ));
        }
        let search = ExprKind::StringSearch { op, haystack, needle, from_index: None };
        Ok(Some(self.machine_int_as_py_int(search, span, body)))
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
        let span = self.span(call.range());
        let [pattern_expr, replacement_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.replace() requires exactly pattern and replacement string arguments; count is not supported yet",
            ));
        };
        let haystack = self.expression(&attr.value, body)?;
        let pattern = self.expression(pattern_expr, body)?;
        let replacement = self.expression(replacement_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(Self::expr_ty(body, replacement)) != Some(&Type::PyStr)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.replace() requires str receiver, pattern, and replacement",
            ));
        }
        let ty = self.intern_type(Type::PyStr);
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
        let span = self.span(call.range());
        let [affix_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.removeprefix()/removesuffix() require exactly one str argument",
            ));
        };
        let haystack = self.expression(&attr.value, body)?;
        let affix = self.expression(affix_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(Self::expr_ty(body, affix)) != Some(&Type::PyStr)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.removeprefix()/removesuffix() require str receiver and argument",
            ));
        }
        let ty = self.intern_type(Type::PyStr);
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
        let span = self.span(call.range());
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "str.isdigit()/isalpha()/isalnum() require no arguments",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::PyStr) {
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
        let span = self.span(call.range());
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
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::PyStr)
            || self.ctx.krate.types.get(separator_ty) != Some(&Type::PyStr)
        {
            return Err(SmeltError::unsupported(
                span,
                "str.split() requires str receiver and separator",
            ));
        }
        let string_ty = self.intern_type(Type::PyStr);
        let ty = self.intern_type(Type::List(string_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
                limit: None,
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
        let span = self.span(call.range());
        let [items_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "str.join() requires exactly one list[str] argument",
            ));
        };
        let separator = self.expression(&attr.value, body)?;
        let items = self.expression(items_expr, body)?;
        let string_ty = self.intern_type(Type::PyStr);
        if self.ctx.krate.types.get(Self::expr_ty(body, separator)) != Some(&Type::PyStr)
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

    /// Lower the `str` methods whose rules the runtime's `py_str.rs` carries:
    /// `title`, `capitalize`, `swapcase`, `zfill`, `center`/`ljust`/`rjust`,
    /// `count`, `index`/`rindex`, `find`/`rfind` with bounds, `split`/`rsplit`
    /// with no separator or with `maxsplit`, `replace` with a count, and
    /// `strip`/`lstrip`/`rstrip` with a character set. `None` for any other
    /// call, which the narrower string handlers try next.
    fn py_str_method_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        use smelt_hir::PyStrOp;
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let method = attr.attr.as_str();
        let positional = call.arguments.args.len();
        let keyword = |name: &str| call.arguments.keywords.iter().find(|keyword| keyword.arg.as_ref().is_some_and(|arg| arg.as_str() == name));
        let known_keywords: &[&str] = match method {
            "split" | "rsplit" => &["sep", "maxsplit"],
            "replace" => &["count"],
            _ => &[],
        };
        if call
            .arguments
            .keywords
            .iter()
            .any(|given| given.arg.as_ref().is_none_or(|arg| !known_keywords.contains(&arg.as_str())))
        {
            return Ok(None);
        }
        // Which methods this handler owns, by arity; anything else stays with
        // the narrower handlers (`find(sub)`, `split(sep)`, `strip()`, ...).
        let owned = match method {
            "title" | "capitalize" | "swapcase" => positional == 0,
            "zfill" => positional == 1,
            "center" | "ljust" | "rjust" => (1..=2).contains(&positional),
            "count" | "index" | "rindex" => (1..=3).contains(&positional),
            "find" | "rfind" => (2..=3).contains(&positional),
            "split" | "rsplit" => positional <= 2,
            "replace" => positional == 3 || keyword("count").is_some(),
            "strip" | "lstrip" | "rstrip" => positional == 1,
            _ => false,
        };
        if !owned {
            return Ok(None);
        }
        // `re.split(...)` and other module functions spelled like a method.
        if let Expr::Name(name) = attr.value.as_ref()
            && self.qualified_import_path(name.id.as_str(), &[]).is_some()
        {
            return Ok(None);
        }
        let receiver = self.expression(&attr.value, body)?;
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, receiver)), Some(Type::PyStr)) {
            return Ok(None);
        }
        let span = self.span(call.range());
        let mut lowered = Vec::new();
        for arg in call.arguments.args.iter() {
            lowered.push(if matches!(arg, Expr::NoneLiteral(_)) { None } else { Some(self.expression(arg, body)?) });
        }
        let keyword_value = |this: &mut Self, name: &str, target: &mut Body| -> Result<Option<Option<smelt_hir::ExprId>>, SmeltError> {
            match keyword(name) {
                Some(found) if matches!(found.value, Expr::NoneLiteral(_)) => Ok(Some(None)),
                Some(found) => Ok(Some(Some(this.expression(&found.value, target)?))),
                None => Ok(None),
            }
        };
        let int_ty = self.intern_type(Type::Int);
        let machine = |value: i64, target: &mut Body| {
            target.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(value)), ty: int_ty, span })
        };
        let mut args = vec![receiver];
        let op = match method {
            "title" => PyStrOp::Title,
            "capitalize" => PyStrOp::Capitalize,
            "swapcase" => PyStrOp::SwapCase,
            "zfill" | "center" | "ljust" | "rjust" => {
                let width = lowered.first().copied().flatten().ok_or_else(|| SmeltError::unsupported(span, "the width must be an int"))?;
                args.push(self.py_int_as_size(width, body));
                if method != "zfill" {
                    let fill = lowered.get(1).copied().flatten().unwrap_or_else(|| {
                        let ty = self.intern_type(Type::PyStr);
                        body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(" ".to_owned())), ty, span })
                    });
                    args.push(fill);
                }
                match method {
                    "zfill" => PyStrOp::ZFill,
                    "center" => PyStrOp::Center,
                    "ljust" => PyStrOp::LJust,
                    _ => PyStrOp::RJust,
                }
            }
            "count" | "index" | "rindex" | "find" | "rfind" => {
                let sub = lowered.first().copied().flatten().ok_or_else(|| SmeltError::unsupported(span, "the substring must be a str"))?;
                args.push(sub);
                let start = match lowered.get(1).copied().flatten() {
                    Some(start) => self.py_int_as_clamped_bound(start, body),
                    None => machine(0, body),
                };
                let end = match lowered.get(2).copied().flatten() {
                    Some(end) => self.py_int_as_clamped_bound(end, body),
                    None => machine(i64::MAX, body),
                };
                args.extend([start, end]);
                match method {
                    "count" => PyStrOp::Count,
                    "index" => PyStrOp::Index,
                    "rindex" => PyStrOp::RIndex,
                    "find" => PyStrOp::Find,
                    _ => PyStrOp::RFind,
                }
            }
            "split" | "rsplit" => {
                let sep = keyword_value(self, "sep", body)?.unwrap_or_else(|| lowered.first().copied().flatten());
                let maxsplit = match keyword_value(self, "maxsplit", body)? {
                    Some(Some(maxsplit)) => self.py_int_as_clamped_bound(maxsplit, body),
                    Some(None) | None => match lowered.get(1).copied().flatten() {
                        Some(maxsplit) => self.py_int_as_clamped_bound(maxsplit, body),
                        None => machine(-1, body),
                    },
                };
                let reverse = method == "rsplit";
                if let Some(separator) = sep {
                    args.extend([separator, maxsplit]);
                    if reverse { PyStrOp::RSplit } else { PyStrOp::Split }
                } else {
                    args.push(maxsplit);
                    if reverse { PyStrOp::RSplitWhitespace } else { PyStrOp::SplitWhitespace }
                }
            }
            "replace" => {
                let (Some(old), Some(new)) = (lowered.first().copied().flatten(), lowered.get(1).copied().flatten()) else {
                    return Err(SmeltError::unsupported(span, "str.replace() needs str arguments"));
                };
                let count = match keyword_value(self, "count", body)? {
                    Some(Some(count)) => self.py_int_as_clamped_bound(count, body),
                    Some(None) | None => match lowered.get(2).copied().flatten() {
                        Some(count) => self.py_int_as_clamped_bound(count, body),
                        None => machine(-1, body),
                    },
                };
                args.extend([old, new, count]);
                PyStrOp::Replace
            }
            "strip" | "lstrip" | "rstrip" => {
                // `strip(None)` is `strip()`, which the narrower handler owns.
                let Some(chars) = lowered.first().copied().flatten() else {
                    return Ok(None);
                };
                args.push(chars);
                match method {
                    "strip" => PyStrOp::StripChars,
                    "lstrip" => PyStrOp::LStripChars,
                    _ => PyStrOp::RStripChars,
                }
            }
            _ => return Ok(None),
        };
        // Text operands must be `str`; a non-`str` argument is the type
        // checker's error, which never reaches here.
        let answer = match op {
            PyStrOp::Count | PyStrOp::Find | PyStrOp::RFind | PyStrOp::Index | PyStrOp::RIndex => None,
            PyStrOp::Split | PyStrOp::RSplit | PyStrOp::SplitWhitespace | PyStrOp::RSplitWhitespace => {
                let text = self.intern_type(Type::PyStr);
                Some(self.intern_type(Type::List(text)))
            }
            _ => Some(self.intern_type(Type::PyStr)),
        };
        let kind = ExprKind::PyStrCall { op, args };
        Ok(Some(match answer {
            Some(ty) => body.push_expr(HirExpr { kind, ty, span }),
            // A position or a count is a machine integer the runtime answers,
            // seen as the Python `int` it is.
            None => self.machine_int_as_py_int(kind, span, body),
        }))
    }
}
