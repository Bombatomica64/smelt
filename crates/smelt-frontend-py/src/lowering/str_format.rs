// `str.format` on a string literal: `"{} {:>5} {name!r}".format(a, b, name=c)`.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// The format string is a literal, so its replacement fields are parsed here,
// with `str.format`'s grammar: `{{` / `}}` are literal braces, and a field is
// `{[name][!conversion][:spec]}` where `name` is empty (the next positional
// argument), a decimal index, or a keyword argument's name. Each field's text
// is what an f-string interpolation with the same conversion and spec writes
// (`interpolation_text`), and the pieces fold like an f-string's.
//
// Python evaluates every argument once, in source order, before formatting,
// whether a field names it zero or several times; so each argument is bound
// to a temporary in a block whose tail is the formatted text.
//
// Refused, with a diagnostic: a non-literal format string, a field naming an
// attribute or an item (`{0.x}`, `{0[1]}`), a nested field in a spec
// (`{:{w}}`), and format strings `CPython` itself rejects at run time
// (mixed automatic and manual numbering, an index past the arguments, an
// unknown keyword, an unmatched brace), as well as `*args` / `**kwargs`.

/// One literal-text or lowered piece of a formatted string.
enum TextSegment {
    /// Literal text.
    Literal(String),
    /// A lowered `str` expression.
    Expr(smelt_hir::ExprId),
}

/// Which argument a replacement field names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FormatArgRef {
    /// A positional argument, by index.
    Positional(usize),
    /// A keyword argument, by name.
    Keyword(String),
}

/// One piece of a parsed format string.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FormatPiece {
    /// Literal text, braces unescaped.
    Text(String),
    /// A replacement field.
    Field {
        /// The argument it formats.
        arg: FormatArgRef,
        /// Its conversion (`!r`, `!s`, `!a`), if any.
        conversion: ruff_python_ast::ConversionFlag,
        /// Its format spec, if not empty.
        spec: Option<String>,
    },
}

/// Parse a `str.format` format string into pieces, or the reason it is
/// refused.
fn parse_format_string(text: &str) -> Result<Vec<FormatPiece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    let mut next_auto = 0_usize;
    let mut numbering: Option<bool> = None; // Some(true) = automatic
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err("single '}' encountered in format string".to_owned()),
            '{' => {
                let mut field = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    match inner {
                        '}' => {
                            closed = true;
                            break;
                        }
                        '{' => return Err("a nested replacement field in a format spec is not supported".to_owned()),
                        other => field.push(other),
                    }
                }
                if !closed {
                    return Err("single '{' encountered in format string".to_owned());
                }
                let (head, spec) = field.split_once(':').map_or((field.as_str(), None), |(head, spec)| (head, Some(spec)));
                let (name, conversion) = match head.split_once('!') {
                    None => (head, ruff_python_ast::ConversionFlag::None),
                    Some((name, "r")) => (name, ruff_python_ast::ConversionFlag::Repr),
                    Some((name, "s")) => (name, ruff_python_ast::ConversionFlag::Str),
                    Some((name, "a")) => (name, ruff_python_ast::ConversionFlag::Ascii),
                    Some(_) => return Err("unknown conversion specifier in format string".to_owned()),
                };
                if name.contains(['.', '[']) {
                    return Err("a format field naming an attribute or an item is not supported".to_owned());
                }
                let arg = if name.is_empty() {
                    if numbering == Some(false) {
                        return Err("cannot switch from manual field specification to automatic field numbering".to_owned());
                    }
                    numbering = Some(true);
                    let index = next_auto;
                    next_auto = next_auto.saturating_add(1);
                    FormatArgRef::Positional(index)
                } else if let Ok(index) = name.parse::<usize>() {
                    if numbering == Some(true) {
                        return Err("cannot switch from automatic field numbering to manual field specification".to_owned());
                    }
                    numbering = Some(false);
                    FormatArgRef::Positional(index)
                } else {
                    FormatArgRef::Keyword(name.to_owned())
                };
                if !literal.is_empty() {
                    pieces.push(FormatPiece::Text(std::mem::take(&mut literal)));
                }
                pieces.push(FormatPiece::Field {
                    arg,
                    conversion,
                    spec: spec.filter(|written| !written.is_empty()).map(str::to_owned),
                });
            }
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        pieces.push(FormatPiece::Text(literal));
    }
    Ok(pieces)
}

impl ModuleBuilder<'_> {
    /// Lower `"<literal>".format(args)` (see the file header), or `None`
    /// when `call` is not a `format` call on a string literal.
    fn str_format_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "format" {
            return Ok(None);
        }
        let Expr::StringLiteral(format_literal) = attr.value.as_ref() else {
            return Ok(None);
        };
        let span = self.span(call.range());
        let pieces = parse_format_string(format_literal.value.to_str())
            .map_err(|reason| SmeltError::unsupported(self.span(format_literal.range), reason))?;
        if call.arguments.args.iter().any(|arg| matches!(arg, Expr::Starred(_)))
            || call.arguments.keywords.iter().any(|keyword| keyword.arg.is_none())
        {
            return Err(SmeltError::unsupported(span, "str.format with `*args` or `**kwargs` is not supported"));
        }

        // Every argument once, in source order, into a temporary.
        let block = body.push_block(span);
        let mut positional = Vec::new();
        let mut keywords: HashMap<String, (smelt_hir::LocalId, TypeId)> = HashMap::new();
        for (index, argument) in call.arguments.iter_source_order().enumerate() {
            let (value_expr, keyword) = match argument {
                ruff_python_ast::ArgOrKeyword::Arg(value) => (value, None),
                ruff_python_ast::ArgOrKeyword::Keyword(keyword) => (&keyword.value, keyword.arg.as_ref().map(|arg| arg.as_str().to_owned())),
            };
            let value = self.expression(value_expr, body)?;
            let ty = Self::expr_ty(body, value);
            let name = self.intern_synthesized_name(&format!("__smelt_format_arg_{index}"));
            let local = body.push_local(LocalDecl { name: Some(name), ty, mutable: false, span });
            let pat = body.push_pattern(HirPattern::Binding(local));
            body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(value) });
            match keyword {
                None => positional.push((local, ty)),
                Some(keyword_name) => {
                    keywords.insert(keyword_name, (local, ty));
                }
            }
        }

        let mut segments = Vec::new();
        for piece in pieces {
            match piece {
                FormatPiece::Text(text) => segments.push(TextSegment::Literal(text)),
                FormatPiece::Field { arg, conversion, spec } => {
                    let bound = match &arg {
                        FormatArgRef::Positional(index) => positional.get(*index).copied().ok_or_else(|| {
                            SmeltError::unsupported(span, format!("replacement index {index} out of range for positional args tuple"))
                        })?,
                        FormatArgRef::Keyword(name) => keywords
                            .get(name)
                            .copied()
                            .ok_or_else(|| SmeltError::unsupported(span, format!("format field `{name}` names no keyword argument")))?,
                    };
                    let (local, ty) = bound;
                    let read = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span });
                    let text = self.interpolation_text(read, conversion, spec, span, body)?;
                    segments.push(TextSegment::Expr(text));
                }
            }
        }
        let text = self.fold_text_segments(segments, span, body);
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(text);
        let ty = self.intern_type(Type::PyStr);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty, span })))
    }
}

#[cfg(test)]
mod str_format_parse_tests {
    use super::{FormatArgRef, FormatPiece, parse_format_string};
    use ruff_python_ast::ConversionFlag;

    #[test]
    #[expect(clippy::literal_string_with_formatting_args, reason = "the literal is a Python format string under test")]
    fn fields_escapes_and_specs_parse_as_str_format_reads_them() -> Result<(), String> {
        let pieces = parse_format_string("{{x}} {} {:>5} {name!r:^7}|")?;
        let field = |arg, conversion, spec: Option<&str>| FormatPiece::Field { arg, conversion, spec: spec.map(str::to_owned) };
        let expected = vec![
                FormatPiece::Text("{x} ".to_owned()),
                field(FormatArgRef::Positional(0), ConversionFlag::None, None),
                FormatPiece::Text(" ".to_owned()),
                field(FormatArgRef::Positional(1), ConversionFlag::None, Some(">5")),
                FormatPiece::Text(" ".to_owned()),
                field(FormatArgRef::Keyword("name".to_owned()), ConversionFlag::Repr, Some("^7")),
                FormatPiece::Text("|".to_owned()),
        ];
        if pieces == expected { Ok(()) } else { Err(format!("{pieces:?}")) }
    }

    #[test]
    fn format_strings_cpython_rejects_are_refused() -> Result<(), String> {
        for text in ["{} {0}", "{0} {}", "a } b", "{0", "{0.x}", "{0[1]}", "{:{w}}", "{!q}"] {
            if parse_format_string(text).is_ok() {
                return Err(format!("`{text}` parsed"));
            }
        }
        Ok(())
    }
}
