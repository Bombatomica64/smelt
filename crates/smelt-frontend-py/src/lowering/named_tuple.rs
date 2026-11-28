// `typing.NamedTuple` classes.
//
// `class Point(NamedTuple): x: int; y: int` is a tuple whose positions have
// names and types. Its construction (positional or keyword, defaults), field
// reads, `repr` (`Point(x=1, y=2)`), equality and immutability are a frozen
// dataclass's, so the class lowers as one (`lower_class_def`), a struct with
// one typed field per position, which is what a hand port writes. What a
// dataclass lacks is the tuple behaviour, lowered here over the same fields:
//
// * `p[i]` with a constant index (negative counts from the end) is the field
//   at that position, with that field's own type;
// * `x, y = p` unpacks the fields in order;
// * `len(p)` is the field count;
// * `p._replace(x=..)` is a new instance with the named fields replaced;
// * `p._asdict()` is a `dict` from field name to value (its values must share
//   one type, as every `dict` here does).
//
// Behavioural requirement: each answers what `CPython` answers for the same
// tuple; type preservation: every position keeps its declared type, so no
// read widens to a union or an erased value. `collections.namedtuple(...)`,
// whose fields carry no types, is refused with a diagnostic naming the typed
// form.

impl ModuleBuilder<'_> {
    /// Record every top-level class whose base denotes `typing.NamedTuple`,
    /// with its annotated field names in declaration order.
    fn collect_named_tuple_classes(&mut self, module: &ModModule) {
        for stmt in &module.body {
            let Stmt::ClassDef(class) = stmt else {
                continue;
            };
            let Some(arguments) = class.arguments.as_deref() else {
                continue;
            };
            let scope = self.bindings.class_statement_scope(class.range.start());
            let is_named_tuple = arguments.args.iter().any(|base| {
                self.bindings
                    .resolve(base, scope)
                    .is_some_and(|resolution| resolution.stdlib() == StdlibMatch::Exact(StdlibName::NamedTuple))
            });
            if !is_named_tuple {
                continue;
            }
            let fields = class
                .body
                .iter()
                .filter_map(|member| match member {
                    Stmt::AnnAssign(StmtAnnAssign { target, .. }) => match target.as_ref() {
                        Expr::Name(name) => Some(name.id.as_str().to_owned()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            self.named_tuple_fields.insert(class.name.as_str().to_owned(), fields);
        }
    }

    /// The class name and field names of a named-tuple type.
    fn named_tuple_of(&self, ty: TypeId) -> Option<(Symbol, Vec<String>)> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        let fields = self.named_tuple_fields.get(self.ctx.krate.symbols.get(*name)?)?;
        Some((*name, fields.clone()))
    }

    /// The read of field `field` of the named-tuple value `receiver`, typed
    /// as the class declares it.
    fn named_tuple_field_read(
        &mut self,
        receiver: smelt_hir::ExprId,
        class: Symbol,
        field: &str,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let field_sym = self.intern_name(field);
        let ty = self
            .class_item_by_symbol(class)
            .and_then(|item| item.fields.iter().find(|candidate| candidate.name == field_sym))
            .map(|found| found.ty)
            .ok_or_else(|| SmeltError::unsupported(span, format!("named tuple field `{field}` is not declared yet")))?;
        Ok(body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field: field_sym }, ty, span }))
    }

    /// `p[i]` of a named tuple with a constant index: the field at `i`.
    fn named_tuple_subscript(
        &mut self,
        receiver: smelt_hir::ExprId,
        index: &Expr,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((class, fields)) = self.named_tuple_of(Self::expr_ty(body, receiver)) else {
            return Ok(None);
        };
        let Some(position) = Self::static_int_literal(index)? else {
            return Err(SmeltError::unsupported(
                span,
                "a named tuple is indexed by a constant position (its fields have different types)",
            ));
        };
        let count = i64::try_from(fields.len()).unwrap_or(i64::MAX);
        let at = if position < 0 { position + count } else { position };
        let field = usize::try_from(at)
            .ok()
            .and_then(|at| fields.get(at))
            .ok_or_else(|| SmeltError::unsupported(span, "tuple index out of range"))?
            .clone();
        self.named_tuple_field_read(receiver, class, &field, span, body).map(Some)
    }

    /// A named-tuple value as the plain tuple of its fields, evaluated once:
    /// `{ let t = value; (t.f0, t.f1, ..) }`. `None` for any other value.
    fn named_tuple_as_tuple(
        &mut self,
        value: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let value_ty = Self::expr_ty(body, value);
        let Some((class, fields)) = self.named_tuple_of(value_ty) else {
            return Ok(None);
        };
        let span = Self::expr_span(body, value);
        let block = body.push_block(span);
        let local = body.push_local(LocalDecl { name: None, ty: value_ty, mutable: false, span });
        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty: value_ty, value: Some(value) });
        let mut items = Vec::with_capacity(fields.len());
        let mut item_tys = Vec::with_capacity(fields.len());
        for field in &fields {
            let read = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: value_ty, span });
            let item = self.named_tuple_field_read(read, class, field, span, body)?;
            item_tys.push(Self::expr_ty(body, item));
            items.push(item);
        }
        let tuple_ty = self.intern_type(Type::Tuple(item_tys));
        let tuple = body.push_expr(HirExpr { kind: ExprKind::TupleLit(items), ty: tuple_ty, span });
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(tuple);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: tuple_ty, span })))
    }

    /// `len(p)` of a named tuple: its field count, after evaluating `p`.
    fn named_tuple_len(&mut self, operand: smelt_hir::ExprId, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let (_, fields) = self.named_tuple_of(Self::expr_ty(body, operand))?;
        let count = i64::try_from(fields.len()).ok()?;
        let int_ty = self.intern_type(Type::PyInt);
        let literal = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(count)), ty: int_ty, span });
        let block = body.push_block(span);
        body.push_stmt_to_block(block, HirStmt::Expr(operand));
        let index = usize::try_from(block.0).ok()?;
        body.blocks[index].tail = Some(literal);
        Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: int_ty, span }))
    }

    /// `p._replace(..)` and `p._asdict()` on a named tuple named by a plain
    /// name, rewritten to the construction and the dict literal they mean,
    /// which then lower (and are type-checked) like any written one. Also
    /// refuses `collections.namedtuple(..)`, whose fields have no types.
    fn named_tuple_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.range());
        if self.qualified_callee(call).as_deref() == Some("collections.namedtuple") {
            return Err(SmeltError::unsupported(
                span,
                "`collections.namedtuple(..)` declares fields without types; declare a `typing.NamedTuple` class with annotated fields instead",
            ));
        }
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let method = attr.attr.as_str();
        if !matches!(method, "_replace" | "_asdict") {
            return Ok(None);
        }
        let Some(receiver_ty) = self.static_receiver_type(&attr.value, body) else {
            return Ok(None);
        };
        let Some((class, fields)) = self.named_tuple_of(receiver_ty) else {
            return Ok(None);
        };
        if !matches!(attr.value.as_ref(), Expr::Name(_)) {
            return Err(SmeltError::unsupported(
                span,
                format!("`{method}()` is supported on a named tuple held by a variable"),
            ));
        }
        let class_name = self.ctx.krate.symbols.get(class).unwrap_or_default().to_owned();
        let receiver_text = match attr.value.as_ref() {
            Expr::Name(name) => name.id.as_str().to_owned(),
            _ => return Ok(None),
        };
        let source = match method {
            "_replace" => {
                if !call.arguments.args.is_empty() {
                    return Err(SmeltError::unsupported(span, "`_replace()` takes field names as keywords"));
                }
                for keyword in &call.arguments.keywords {
                    let Some(name) = keyword.arg.as_ref() else {
                        return Err(SmeltError::unsupported(span, "`_replace(**mapping)` is not supported"));
                    };
                    if !fields.iter().any(|field| field == name.as_str()) {
                        return Err(SmeltError::unsupported(
                            span,
                            format!("`_replace()` got an unexpected field name '{}'", name.as_str()),
                        ));
                    }
                }
                let arguments = fields
                    .iter()
                    .map(|field| {
                        let replaced = call
                            .arguments
                            .keywords
                            .iter()
                            .position(|keyword| keyword.arg.as_ref().is_some_and(|name| name.as_str() == field.as_str()));
                        match replaced {
                            Some(index) => format!("{field}=__smelt_replace_{index}"),
                            None => format!("{field}={receiver_text}.{field}"),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{class_name}({arguments})")
            }
            _ => {
                if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
                    return Err(SmeltError::unsupported(span, "`_asdict()` takes no arguments"));
                }
                let entries = fields
                    .iter()
                    .map(|field| format!("{field:?}: {receiver_text}.{field}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{entries}}}")
            }
        };
        let parsed = ruff_python_parser::parse_expression(&source)
            .map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        let mut rewritten = *parsed.into_syntax().body;
        ruff_python_ast::relocate::relocate_expr(&mut rewritten, call.range());
        // Put the written replacement expressions back in place of their
        // placeholders, so each is lowered from its own source.
        if let Expr::Call(constructed) = &mut rewritten {
            for keyword in &mut constructed.arguments.keywords {
                if let Expr::Name(placeholder) = &keyword.value
                    && let Some(index) = placeholder.id.as_str().strip_prefix("__smelt_replace_")
                    && let Ok(index) = index.parse::<usize>()
                    && let Some(written) = call.arguments.keywords.get(index)
                {
                    keyword.value = written.value.clone();
                }
            }
        }
        self.expression(&rewritten, body).map(Some)
    }
}
