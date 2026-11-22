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

    /// `"template" % args`, printf-style formatting with a literal template:
    /// each conversion is parsed here and lowered to the format-spec
    /// formatting an f-string uses, so `%5.2f` is `format(x, "5.2f")`.
    ///
    /// Flags map onto the spec (`-` left-aligns, `0` zero-pads a number, `+`,
    /// space and `#` are the spec's own); `%d`/`%i`/`%u` truncate a `float`
    /// first, as `CPython` does (`int(x)`, raising for a NaN or an infinity);
    /// `%s`/`%r`/`%a` are `str`/`repr`/`ascii` of the value, right-aligned in
    /// a width unless `-`; `%c` takes a code point or a one-character `str`;
    /// `%%` is a `%`. A tuple right operand supplies one value per
    /// conversion, any other value is the single argument. A count that does
    /// not match, a `%(name)s` mapping key, a `*` width and a template that is
    /// not a literal are refused (the first raises `TypeError` in `CPython`).
    fn percent_format_expression(
        &mut self,
        template: &str,
        args: &Expr,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let values: Vec<&Expr> = match args {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            other => vec![other],
        };
        let str_ty = self.intern_type(Type::PyStr);
        let mut pieces: Vec<smelt_hir::ExprId> = Vec::new();
        let mut literal = String::new();
        let mut next_value = values.iter();
        let mut chars = template.chars().peekable();
        let unsupported = |message: &str| SmeltError::unsupported(span, format!("`%` formatting: {message}"));
        while let Some(ch) = chars.next() {
            if ch != '%' {
                literal.push(ch);
                continue;
            }
            let mut flags = String::new();
            while let Some(&flag) = chars.peek().filter(|flag| "-+ #0".contains(**flag)) {
                flags.push(flag);
                chars.next();
            }
            let mut width = String::new();
            while let Some(&digit) = chars.peek().filter(|digit| digit.is_ascii_digit()) {
                width.push(digit);
                chars.next();
            }
            let mut precision = None;
            if chars.peek() == Some(&'.') {
                chars.next();
                let mut digits = String::new();
                while let Some(&digit) = chars.peek().filter(|digit| digit.is_ascii_digit()) {
                    digits.push(digit);
                    chars.next();
                }
                precision = Some(if digits.is_empty() { "0".to_owned() } else { digits });
            }
            while chars.peek().is_some_and(|modifier| "hlL".contains(*modifier)) {
                chars.next();
            }
            let Some(conversion) = chars.next() else {
                return Err(unsupported("the template ends inside a conversion (ValueError in CPython)"));
            };
            if conversion == '%' && flags.is_empty() && width.is_empty() && precision.is_none() {
                literal.push('%');
                continue;
            }
            if matches!(conversion, '(' | '*') || chars.peek() == Some(&'*') {
                return Err(unsupported("mapping keys and `*` widths are not supported"));
            }
            let Some(value_ast) = next_value.next() else {
                return Err(unsupported("not enough arguments for the template (TypeError in CPython)"));
            };
            let value = self.expression(value_ast, body)?;
            let value_ty = self.ctx.krate.types.get(Self::expr_ty(body, value)).cloned();
            let left = flags.contains('-');
            let precision_text = precision.as_ref().map(|digits| format!(".{digits}")).unwrap_or_default();
            let formatted = match conversion {
                'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                    let integral = matches!(conversion, 'd' | 'i' | 'u' | 'o' | 'x' | 'X');
                    if integral && precision.is_some() {
                        return Err(unsupported("a precision on an integer conversion is not supported"));
                    }
                    let shown = match (&value_ty, conversion) {
                        (Some(Type::Float), 'd' | 'i' | 'u') => {
                            self.py_numeric_call(PyNumericOp::FromFloat, vec![value], span, body)
                        }
                        (Some(Type::Float), 'o' | 'x' | 'X') => {
                            return Err(unsupported("an integer conversion of a float (TypeError in CPython)"));
                        }
                        (Some(Type::PyInt | Type::Float | Type::Bool), _) => value,
                        _ => return Err(unsupported("a numeric conversion needs an int, float or bool")),
                    };
                    let mut spec = String::new();
                    if left {
                        spec.push('<');
                    }
                    if flags.contains('+') {
                        spec.push('+');
                    } else if flags.contains(' ') {
                        spec.push(' ');
                    }
                    if flags.contains('#') {
                        spec.push('#');
                    }
                    if flags.contains('0') && !left {
                        spec.push('0');
                    }
                    spec.push_str(&width);
                    spec.push_str(&precision_text);
                    spec.push(if matches!(conversion, 'i' | 'u') { 'd' } else { conversion });
                    self.formatted_value(shown, spec, span, body)?
                }
                's' | 'r' | 'a' | 'c' => {
                    let text = match conversion {
                        's' if value_ty == Some(Type::PyStr) => value,
                        's' => self.python_text_expr(value, false, span, body)?,
                        'r' => self.python_text_expr(value, true, span, body)?,
                        'a' => self.python_ascii_expr(value, span, body)?,
                        _ => match value_ty {
                            Some(Type::PyInt) => {
                                let point = self.py_int_as_clamped_bound(value, body);
                                self.py_numeric_call(PyNumericOp::Chr, vec![point], span, body)
                            }
                            Some(Type::PyStr) => value,
                            _ => return Err(unsupported("`%c` needs an int or a one-character str")),
                        },
                    };
                    if width.is_empty() && precision.is_none() {
                        text
                    } else {
                        let spec = format!("{}{width}{precision_text}", if left { '<' } else { '>' });
                        self.formatted_value(text, spec, span, body)?
                    }
                }
                other => return Err(unsupported(&format!("unsupported format character '{other}' (ValueError in CPython)"))),
            };
            if !literal.is_empty() {
                pieces.push(body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(::std::mem::take(&mut literal))), ty: str_ty, span }));
            }
            pieces.push(formatted);
        }
        if next_value.next().is_some() {
            return Err(unsupported("not all arguments converted (TypeError in CPython)"));
        }
        if !literal.is_empty() || pieces.is_empty() {
            pieces.push(body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(literal)), ty: str_ty, span }));
        }
        let mut pieces = pieces.into_iter();
        let first = pieces.next().ok_or_else(|| unsupported("an empty template"))?;
        Ok(pieces.fold(first, |acc, piece| {
            body.push_expr(HirExpr { kind: ExprKind::BinOp { op: BinOp::Add, lhs: acc, rhs: piece }, ty: str_ty, span })
        }))
    }
}
