/// Source element specification for a comprehension, before its element
/// expressions are lowered.
#[derive(Clone, Copy)]
enum ComprehensionSource<'a> {
    /// `[elt ...]` and `(elt ...)` generator expressions.
    List(&'a Expr),
    /// `{elt ...}` set comprehensions.
    Set(&'a Expr),
    /// `{key: value ...}` dict comprehensions.
    Dict {
        /// Key expression evaluated per produced element.
        key: &'a Expr,
        /// Value expression evaluated per produced element.
        value: &'a Expr,
    },
}

/// A bound comprehension generator clause: the loop pattern, the normalized
/// iterable expression, and the `if` guards (kept as source AST so they lower
/// while the loop target is in scope).
struct ComprehensionLevel<'a> {
    /// Loop binding pattern for this generator's target.
    pat: smelt_hir::PatternId,
    /// Normalized iterable expression for this generator.
    iter: smelt_hir::ExprId,
    /// `if` guard expressions for this generator, in source order.
    ifs: &'a [Expr],
}

/// Shared context threaded through the comprehension loop builder: the
/// accumulator local and type, how to append, and the comprehension span.
struct ComprehensionBuild<'a> {
    /// Mutable local holding the comprehension's accumulator collection.
    acc_local: smelt_hir::LocalId,
    /// Type of the accumulator collection.
    acc_ty: TypeId,
    /// How each produced element is appended to the accumulator.
    append: &'a ComprehensionAppend,
    /// Source span of the comprehension expression.
    span: Span,
}

/// How a comprehension appends each produced element to its accumulator.
enum ComprehensionAppend {
    /// `acc.push(item)` — list comprehensions and generator expressions.
    List {
        /// Lowered element expression.
        item: smelt_hir::ExprId,
    },
    /// `acc.add(item)` — set comprehensions.
    Set {
        /// Lowered element expression.
        item: smelt_hir::ExprId,
    },
    /// `acc[key] = value` — dict comprehensions.
    Dict {
        /// Lowered key expression.
        key: smelt_hir::ExprId,
        /// Lowered value expression.
        value: smelt_hir::ExprId,
    },
}

impl ModuleBuilder<'_> {
    /// Resolve a class method by inspecting class metadata directly.
    fn class_method_item_by_name(&self, class_name: &str, method: &str) -> Option<ItemId> {
        let class_sym = self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if self.ctx.krate.symbols.source(class.name) == Some(class_name) => {
                Some(class.name)
            }
            _ => None,
        })?;
        // The first class of the MRO declaring it (its base chain under
        // single inheritance).
        self.class_mro(class_sym).into_iter().find_map(|ancestor| {
            let found = self.class_item_named(ancestor)?;
            found.methods.iter().copied().find(|method_item| {
                matches!(self.item_ref(*method_item), Item::Function(function)
                    if self.ctx.krate.symbols.source(function.name) == Some(method))
            })
        })
    }

    /// Return targeted diagnostics for deferred Python stdlib/native library APIs.
    fn unsupported_deferred_stdlib_call(
        &self,
        call: &ruff_python_ast::ExprCall,
    ) -> Option<SmeltError> {
        let span = self.span(call.range());
        match call.func.as_ref() {
            Expr::Name(name) if name.id.as_str() == "open" && !self.program_binds_name("open") => Some(SmeltError::unsupported(
                span,
                "Python open() is not supported yet; file IO needs a dedicated text-mode mapping",
            )),
            Expr::Attribute(attr) => {
                if matches!(
                    attr.value.as_ref(),
                    Expr::Call(inner)
                        if matches!(inner.func.as_ref(), Expr::Name(name) if name.id.as_str() == "open" && !self.program_binds_name("open"))
                ) {
                    return Some(SmeltError::unsupported(
                        span,
                        "Python open() is not supported yet; file IO needs a dedicated text-mode mapping",
                    ));
                }
                let Expr::Name(module) = attr.value.as_ref() else {
                    return None;
                };
                let message = match module.id.as_str() {
                    "datetime" => {
                        "Python datetime is not supported yet; datetime/date/timedelta need a chrono-backed mapping"
                    }
                    "urllib" | "urlparse" => {
                        "Python URL parsing is not supported yet; urllib/urlparse need a URL mapping policy"
                    }
                    "numpy" | "np" => {
                        "NumPy is deferred from Phase 6; array dtype, ownership, shape, and broadcasting semantics need a dedicated design"
                    }
                    "pandas" | "pd" => {
                        "pandas is out of scope for Phase 6; dataframe semantics need a dedicated native-data-library design"
                    }
                    _ => return None,
                };
                Some(SmeltError::unsupported(span, message))
            }
            _ => None,
        }
    }

    /// Lower direct Python container constructor calls.
    fn container_constructor_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let constructor = name.id.as_str();
        if !matches!(constructor, "list" | "set" | "dict" | "tuple") {
            return Ok(None);
        }
        let span = self.span(call.range());
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "container constructors do not support keyword arguments yet",
            ));
        }
        match (constructor, call.arguments.args.as_ref()) {
            ("list", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty list() requires a list type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::List(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "list() type annotation must be list[T]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("set", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty set() requires a set type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Set(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "set() type annotation must be set[T]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::SetLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("dict", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty dict() requires a dict type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict() type annotation must be dict[K, V]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("tuple", []) => {
                let ty = type_hint.unwrap_or_else(|| self.intern_type(Type::Tuple(Vec::new())));
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple() type annotation must be tuple[...]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::TupleLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("list", [arg]) => self.list_constructor_from_arg(arg, body, span),
            ("set", [arg]) => self.set_constructor_from_arg(arg, body, span),
            ("dict", [arg]) => self.dict_constructor_from_arg(arg, body, span),
            ("tuple", [arg]) => self.tuple_constructor_from_arg(arg, body, span, type_hint),
            _ => Err(SmeltError::unsupported(
                span,
                "container constructors support zero arguments or one same-container argument",
            )),
        }
    }

    /// `value` as a list for a builtin that reads an iterable without caring
    /// about its order (`sorted`, `sum`, `min`, `max`, `all`, `any`): a set
    /// is projected to the list of its items, any other value is returned
    /// unchanged for the builtin's own type check.
    pub(super) fn order_free_iterable_as_list(&mut self, value: smelt_hir::ExprId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        if let Some(collected) = self.generator_as_list(value, body) {
            return collected;
        }
        let Some(Type::Set(item_ty)) = self.ctx.krate.types.get(Self::expr_ty(body, value)).cloned() else {
            return value;
        };
        let ty = self.intern_type(Type::List(item_ty));
        body.push_expr(HirExpr {
            kind: ExprKind::SetProjection { op: SetProjectionOp::Values, set: value },
            ty,
            span,
        })
    }

    /// Lower `list(value)` for the currently supported direct container inputs.
    fn list_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expr::Call(call) = arg
            && let Some(expr) = self.map_filter_list_callback(call, body, span)?
        {
            return Ok(Some(expr));
        }
        let source = self.expression(arg, body)?;
        if let Some(collected) = self.generator_as_list(source, body) {
            return Ok(Some(collected));
        }
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::List(_)) => (ExprKind::ListCopy { list: source }, source_ty),
            Some(Type::Set(item_type)) => {
                let item_ty = *item_type;
                (
                    ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: source,
                    },
                    self.intern_type(Type::List(item_ty)),
                )
            }
            Some(Type::Dict(key_type, _)) => {
                let key_ty = *key_type;
                (
                    ExprKind::DictProjection {
                        op: DictProjectionOp::Keys,
                        dict: source,
                    },
                    self.intern_type(Type::List(key_ty)),
                )
            }
            Some(Type::Tuple(items)) => {
                let item_ty = Self::homogeneous_tuple_item_ty(items, span)?;
                (
                    ExprKind::TupleToList { tuple: source },
                    self.intern_type(Type::List(item_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "list(value) currently requires a list, set, dict, or homogeneous tuple value",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `list(map(callback, values))` and `list(filter(callback, values))`.
    fn map_filter_list_callback(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match name.id.as_str() {
            "map" => ListCallbackOp::Map,
            "filter" => ListCallbackOp::Filter,
            _ => return Ok(None),
        };
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "map/filter callbacks do not support keyword arguments",
            ));
        }
        let [callback_arg, list_arg] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "map/filter require callback and one list argument",
            ));
        };
        let list = self.expression(list_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                span,
                "map/filter input must be a list",
            ));
        };
        let item_ty = *element_ty;
        // `filter(None, xs)` keeps the items that are true themselves: the
        // identity predicate.
        let identity = (op == ListCallbackOp::Filter && matches!(callback_arg, Expr::NoneLiteral(_)))
            .then(|| Self::identity_lambda(callback_arg.range()))
            .flatten();
        let callback_source = identity.as_ref().unwrap_or(callback_arg);
        let callback = self.python_callback_argument(callback_source, &[item_ty], body)?;
        let ty = if name.id.as_str() == "filter" {
            // `filter` keeps an item when the predicate's result is true. A
            // non-`bool` result is tested by the callback adapter's `ToBool`,
            // which is Python's truth test for the scalar types and their
            // optionals only.
            let truth_by_cast = |this: &Self, ty: TypeId| {
                this.truth_is_to_bool(ty)
                    || matches!(this.ctx.krate.types.get(ty), Some(Type::Optional(inner, _)) if this.truth_is_to_bool(*inner))
            };
            if !truth_by_cast(self, callback.return_ty) {
                return Err(SmeltError::unsupported(
                    self.span(callback_source.range()),
                    "filter callback must return bool, int, float, str, or an optional of one",
                ));
            }
            list_ty
        } else {
            self.intern_type(Type::List(callback.return_ty))
        };
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListCallback {
                op,
                list,
                callback: callback.expr,
            },
            ty,
            span,
        })))
    }

    /// `enumerate(xs, start)`: the pairs of `enumerate(xs)` with `start`
    /// (evaluated once, before the iteration, as Python does) added to each
    /// index, materialized as `enumerate(xs)` is.
    #[expect(clippy::too_many_arguments, reason = "the pairs, the start, their item type and where they lower")]
    fn enumerate_from(
        &mut self,
        enumerated: smelt_hir::ExprId,
        start_arg: &Expr,
        item_ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let int_ty = self.intern_type(Type::PyInt);
        let none_ty = self.intern_type(Type::None);
        let tuple_ty = self.intern_type(Type::Tuple(vec![int_ty, item_ty]));
        let list_ty = self.intern_type(Type::List(tuple_ty));
        let start = self.expression(start_arg, body)?;
        if Self::expr_ty(body, start) != int_ty {
            return Err(SmeltError::unsupported(self.span(start_arg.range()), "enumerate() start must be an int"));
        }
        let outer = body.push_block(span);
        let new_local = |this: &mut Self, arena: &mut Body, spelled: &str, ty: TypeId, mutable: bool| {
            let symbol = this.intern_synthesized_name(spelled);
            arena.push_local(LocalDecl { name: Some(symbol), ty, mutable, span })
        };
        let start_local = new_local(self, body, "__smelt_enumerate_start", int_ty, false);
        let start_pat = body.push_pattern(HirPattern::Binding(start_local));
        body.push_stmt_to_block(outer, HirStmt::Let { pat: start_pat, ty: int_ty, value: Some(start) });
        let acc_local = new_local(self, body, "__smelt_enumerate_acc", list_ty, true);
        let acc_pat = body.push_pattern(HirPattern::Binding(acc_local));
        let empty = body.push_expr(HirExpr { kind: ExprKind::ListLit(Vec::new()), ty: list_ty, span });
        body.push_stmt_to_block(outer, HirStmt::Let { pat: acc_pat, ty: list_ty, value: Some(empty) });
        let index_local = new_local(self, body, "__smelt_enumerate_index", int_ty, false);
        let item_local = new_local(self, body, "__smelt_enumerate_item", item_ty, false);
        let index_pat = body.push_pattern(HirPattern::Binding(index_local));
        let item_pat = body.push_pattern(HirPattern::Binding(item_local));
        let pair_pat = body.push_pattern(HirPattern::Tuple(vec![index_pat, item_pat]));
        let loop_body = body.push_block(span);
        let index = body.push_expr(HirExpr { kind: ExprKind::Local(index_local), ty: int_ty, span });
        let start_read = body.push_expr(HirExpr { kind: ExprKind::Local(start_local), ty: int_ty, span });
        let shifted = self.python_arithmetic(PythonArithmetic::Plain(BinOp::Add), index, start_read, span, body)?;
        let item = body.push_expr(HirExpr { kind: ExprKind::Local(item_local), ty: item_ty, span });
        let pair = body.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![shifted, item]), ty: tuple_ty, span });
        let acc = body.push_expr(HirExpr { kind: ExprKind::Local(acc_local), ty: list_ty, span });
        let push = body.push_expr(HirExpr { kind: ExprKind::ListPush { list: acc, item: pair }, ty: none_ty, span });
        body.push_stmt_to_block(loop_body, HirStmt::Expr(push));
        body.push_stmt_to_block(outer, HirStmt::For { pat: pair_pat, iter: enumerated, body: loop_body });
        let tail = body.push_expr(HirExpr { kind: ExprKind::Local(acc_local), ty: list_ty, span });
        let outer_idx = usize::try_from(outer.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_idx].tail = Some(tail);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty: list_ty, span }))
    }

    /// `lambda v: v`, at `range`: the identity callback.
    fn identity_lambda(range: TextRange) -> Option<Expr> {
        let parsed = ruff_python_parser::parse_expression("lambda __smelt_item: __smelt_item").ok()?;
        let mut expr = *parsed.into_syntax().body;
        ruff_python_ast::relocate::relocate_expr(&mut expr, range);
        Some(expr)
    }

    /// Lower a Python list comprehension `[elt for t in it if cond ...]`.
    pub(crate) fn list_comprehension(
        &mut self,
        comp: &ruff_python_ast::ExprListComp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(comp.range);
        self.comprehension(&comp.generators, ComprehensionSource::List(&comp.elt), span, body)
    }

    /// Lower a Python set comprehension `{elt for t in it if cond ...}`.
    pub(crate) fn set_comprehension(
        &mut self,
        comp: &ruff_python_ast::ExprSetComp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(comp.range);
        self.comprehension(&comp.generators, ComprehensionSource::Set(&comp.elt), span, body)
    }

    /// Lower a Python dict comprehension `{key: value for t in it if cond ...}`.
    pub(crate) fn dict_comprehension(
        &mut self,
        comp: &ruff_python_ast::ExprDictComp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(comp.range);
        let Some(key) = comp.key.as_deref() else {
            return Err(SmeltError::unsupported(
                span,
                "dict comprehension requires an explicit key expression",
            ));
        };
        self.comprehension(
            &comp.generators,
            ComprehensionSource::Dict {
                key,
                value: &comp.value,
            },
            span,
            body,
        )
    }

    /// Lower a Python generator expression `(elt for t in it if cond ...)`.
    ///
    /// Generator expressions are materialized eagerly into a list, which is
    /// correct for the common eager sinks (`list(...)`, `sum(...)`,
    /// `" ".join(...)`) but does not preserve laziness.
    pub(crate) fn generator_expression(
        &mut self,
        comp: &ruff_python_ast::ExprGenerator,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(comp.range);
        self.comprehension(&comp.generators, ComprehensionSource::List(&comp.elt), span, body)
    }

    /// Lower any comprehension form to an imperative accumulator loop wrapped in
    /// a block expression (see `docs/python-comprehensions.md`).
    ///
    /// The loop targets, element/key/value, and `if` guards all lower through
    /// the normal expression path, so the full expression language is available
    /// inside comprehension bodies. Comprehension targets are scoped to the
    /// comprehension: the locals are snapshotted on entry and restored on exit
    /// so the loop variables do not leak into the enclosing scope.
    fn comprehension(
        &mut self,
        generators: &[ruff_python_ast::Comprehension],
        source: ComprehensionSource<'_>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.with_block_scope(|this| this.comprehension_impl(generators, source, span, body))
    }

    /// Inner comprehension lowering; see [`Self::comprehension`] for the
    /// surrounding scope save/restore.
    fn comprehension_impl(
        &mut self,
        generators: &[ruff_python_ast::Comprehension],
        source: ComprehensionSource<'_>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if generators.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "comprehension requires at least one `for` clause",
            ));
        }

        // Bind every generator target in source order, lowering each iterable so
        // later iterables and conditions can reference earlier loop variables.
        let mut levels: Vec<ComprehensionLevel<'_>> = Vec::with_capacity(generators.len());
        for generator in generators {
            if generator.is_async {
                return Err(SmeltError::unsupported(
                    span,
                    "async comprehensions are not supported",
                ));
            }
            // A name or a (nested) tuple/list target binds as a `for`
            // statement's target does (`for k, v in d.items()`).
            if !matches!(&generator.target, Expr::Name(_) | Expr::Tuple(_) | Expr::List(_)) {
                return Err(SmeltError::unsupported(
                    self.span(generator.target.range()),
                    "comprehension targets must be a name or a tuple of names",
                ));
            }
            let raw_iter = self.expression(&generator.iter, body)?;
            let iter = self.for_iterable(raw_iter, body);
            let iter_ty = Self::expr_ty(body, iter);
            let Some(item_ty) = self.iter_item_type(iter_ty) else {
                return Err(SmeltError::unsupported(
                    self.span(generator.iter.range()),
                    "comprehensions can only iterate over a list, set, dict, or string",
                ));
            };
            let pat = self.binding_pattern_from_target(&generator.target, body, Some(item_ty))?;
            levels.push(ComprehensionLevel {
                pat,
                iter,
                ifs: &generator.ifs,
            });
        }

        // Lower the element/key/value while all loop targets are in scope and
        // derive the accumulator's collection type from them.
        let (acc_ty, append) = match source {
            ComprehensionSource::List(elt) => {
                let item = self.expression(elt, body)?;
                let acc_ty = self.intern_type(Type::List(Self::expr_ty(body, item)));
                (acc_ty, ComprehensionAppend::List { item })
            }
            ComprehensionSource::Set(elt) => {
                let item = self.expression(elt, body)?;
                let acc_ty = self.intern_type(Type::Set(Self::expr_ty(body, item)));
                (acc_ty, ComprehensionAppend::Set { item })
            }
            ComprehensionSource::Dict { key, value } => {
                let key_expr = self.expression(key, body)?;
                let value_expr = self.expression(value, body)?;
                let acc_ty = self.intern_type(Type::Dict(
                    Self::expr_ty(body, key_expr),
                    Self::expr_ty(body, value_expr),
                ));
                (
                    acc_ty,
                    ComprehensionAppend::Dict {
                        key: key_expr,
                        value: value_expr,
                    },
                )
            }
        };

        // Declare the mutable accumulator and seed it with an empty collection.
        let acc_name = self.intern_synthesized_name("__smelt_comp_acc");
        let acc_local = body.push_local(LocalDecl {
            name: Some(acc_name),
            ty: acc_ty,
            mutable: true,
            span,
        });
        let init_kind = match &append {
            ComprehensionAppend::List { .. } => ExprKind::ListLit(Vec::new()),
            ComprehensionAppend::Set { .. } => ExprKind::SetLit(Vec::new()),
            ComprehensionAppend::Dict { .. } => ExprKind::DictLit(Vec::new()),
        };
        let init = body.push_expr(HirExpr {
            kind: init_kind,
            ty: acc_ty,
            span,
        });
        let acc_pat = body.push_pattern(HirPattern::Binding(acc_local));
        let outer = body.push_block(span);
        body.push_stmt_to_block(
            outer,
            HirStmt::Let {
                pat: acc_pat,
                ty: acc_ty,
                value: Some(init),
            },
        );

        // Build the nested for/if structure with the append at the innermost.
        let build = ComprehensionBuild {
            acc_local,
            acc_ty,
            append: &append,
            span,
        };
        self.emit_comprehension_levels(outer, &levels, &build, body)?;

        // The block evaluates to the populated accumulator.
        let tail = body.push_expr(HirExpr {
            kind: ExprKind::Local(acc_local),
            ty: acc_ty,
            span,
        });
        let outer_idx = usize::try_from(outer.0)
            .map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_idx].tail = Some(tail);

        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Block(outer),
            ty: acc_ty,
            span,
        }))
    }

    /// Emit the nested `for` loop for the first remaining generator level into
    /// `target_block`: this level's `if` guards are applied as a chain of `if`
    /// statements and inner levels are emitted by recursion. When no levels
    /// remain it emits the accumulator append.
    fn emit_comprehension_levels(
        &mut self,
        target_block: smelt_hir::BlockId,
        levels: &[ComprehensionLevel<'_>],
        build: &ComprehensionBuild<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let Some((level, rest)) = levels.split_first() else {
            let stmt = self.comprehension_append_stmt(build, body)?;
            body.push_stmt_to_block(target_block, stmt);
            return Ok(());
        };
        // The loop body wraps the inner levels in this level's `if` guards,
        // applied in source order as nested `if` statements.
        let loop_body = body.push_block(build.span);
        let mut innermost = loop_body;
        for condition in level.ifs {
            let cond = self.python_condition(condition, body)?;
            let then_block = body.push_block(build.span);
            body.push_stmt_to_block(
                innermost,
                HirStmt::If {
                    cond,
                    then_block,
                    else_block: None,
                },
            );
            innermost = then_block;
        }
        self.emit_comprehension_levels(innermost, rest, build, body)?;
        body.push_stmt_to_block(
            target_block,
            HirStmt::For {
                pat: level.pat,
                iter: level.iter,
                body: loop_body,
            },
        );
        Ok(())
    }

    /// Build the statement that appends one produced element to the accumulator.
    fn comprehension_append_stmt(
        &mut self,
        build: &ComprehensionBuild<'_>,
        body: &mut Body,
    ) -> Result<HirStmt, SmeltError> {
        let ComprehensionBuild {
            acc_local,
            acc_ty,
            append,
            span,
        } = *build;
        let none_ty = self.intern_type(Type::None);
        let acc = body.push_expr(HirExpr {
            kind: ExprKind::Local(acc_local),
            ty: acc_ty,
            span,
        });
        match append {
            ComprehensionAppend::List { item } => {
                let push = body.push_expr(HirExpr {
                    kind: ExprKind::ListPush { list: acc, item: *item },
                    ty: none_ty,
                    span,
                });
                Ok(HirStmt::Expr(push))
            }
            ComprehensionAppend::Set { item } => {
                let add = body.push_expr(HirExpr {
                    kind: ExprKind::SetAdd { set: acc, item: *item },
                    ty: none_ty,
                    span,
                });
                Ok(HirStmt::Expr(add))
            }
            ComprehensionAppend::Dict { key, value } => {
                let value_ty = Self::expr_ty(body, *value);
                let target = body.push_expr(HirExpr {
                    kind: ExprKind::Index { receiver: acc, index: *key },
                    ty: value_ty,
                    span,
                });
                Ok(HirStmt::Assign {
                    target,
                    value: *value,
                })
            }
        }
    }

    /// Lower a callback argument: an inline lambda typed by the API's parameters,
    /// or any expression that is already a callable of exactly those parameters.
    pub(super) fn python_callback_argument(
        &mut self,
        arg: &Expr,
        expected_param_tys: &[TypeId],
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Expr::Lambda(lambda) = arg {
            let closure = self.lambda_closure(lambda, expected_param_tys, None, body)?;
            return Ok(ClosureCallback {
                expr: closure.expr,
                return_ty: closure.return_ty,
            });
        }
        if let Some(lambda) = self.builtin_callback_lambda(arg, expected_param_tys.len()) {
            let closure = self.lambda_closure(&lambda, expected_param_tys, None, body)?;
            return Ok(ClosureCallback {
                expr: closure.expr,
                return_ty: closure.return_ty,
            });
        }
        let mut expr = self.expression(arg, body)?;
        let index = usize::try_from(expr.0).unwrap_or(usize::MAX);
        if let ExprKind::Item(item) = body.exprs[index].kind {
            expr = self.function_item_closure(item, expr, self.span(arg.range()), body)?;
        }
        let Some(Type::Function(function)) =
            self.ctx.krate.types.get(Self::expr_ty(body, expr)).cloned()
        else {
            return Err(SmeltError::unsupported(
                self.span(arg.range()),
                "callback arguments must be callable",
            ));
        };
        if function.params != expected_param_tys {
            return Err(SmeltError::unsupported(
                self.span(arg.range()),
                "callback parameter types do not match the values it receives",
            ));
        }
        Ok(ClosureCallback {
            expr,
            return_ty: function.return_ty,
        })
    }

    /// A builtin function named as a callback (`sorted(xs, key=len)`), as the
    /// lambda that calls it with the callback's parameters: `len` becomes
    /// `lambda a: len(a)`, so the call lowers through the same builtin rules
    /// (and the same refusals) as a written call, typed by the values the
    /// callback receives. The synthesized nodes take `arg`'s source range, so
    /// a diagnostic points at the name. An unbound builtin-type method
    /// (`str.lower`) calls the method on the first parameter. `None` when
    /// `arg` is neither, or the program binds the name.
    fn builtin_callback_lambda(&self, arg: &Expr, arity: usize) -> Option<ruff_python_ast::ExprLambda> {
        let params = (0..arity).map(|position| format!("__smelt_builtin_arg_{position}")).collect::<Vec<_>>();
        let (source, range) = match arg {
            Expr::Name(name) => {
                let spelled = name.id.as_str();
                if self.program_binds_name(spelled) || !ruff_python_stdlib::builtins::is_python_builtin(spelled, ruff_python_ast::PythonVersion::default().minor, false) {
                    return None;
                }
                (format!("lambda {list}: {spelled}({list})", list = params.join(", ")), name.range)
            }
            // An unbound method of a builtin type (`key=str.lower`) is the
            // lambda calling it on its first argument: `str.lower` is
            // `lambda a: a.lower()`, and the method call lowers (and is
            // refused) by the ordinary method rules for the item's type.
            Expr::Attribute(attr) => {
                let Expr::Name(owner) = attr.value.as_ref() else {
                    return None;
                };
                let owner_name = owner.id.as_str();
                if !matches!(owner_name, "str" | "int" | "float" | "list" | "dict" | "set" | "tuple") || self.program_binds_name(owner_name) {
                    return None;
                }
                let (first, rest) = params.split_first()?;
                (
                    format!("lambda {list}: {first}.{method}({rest})", list = params.join(", "), method = attr.attr.as_str(), rest = rest.join(", ")),
                    attr.range,
                )
            }
            _ => return None,
        };
        let parsed = ruff_python_parser::parse_expression(&source).ok()?;
        let mut expr = *parsed.into_syntax().body;
        ruff_python_ast::relocate::relocate_expr(&mut expr, range);
        match expr {
            Expr::Lambda(lambda) => Some(lambda),
            _ => None,
        }
    }

    /// Wrap a module-level function used as a value (`sorted(xs, key=neg)`) in
    /// a closure that forwards its parameters to the function: `neg` becomes
    /// `lambda x: neg(x)` with `neg`'s own parameter and return types. The
    /// function item itself is only ever a callee, so a value position needs
    /// this forwarding closure, as a lambda argument already is one. A
    /// function with a `*args`/`**kwargs` tail is refused: the callback caller
    /// passes positional values only, and their packing is not modelled here.
    fn function_item_closure(
        &mut self,
        item: ItemId,
        item_expr: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item_ty = Self::expr_ty(body, item_expr);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(item_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "only a function can be passed as a callback"));
        };
        if function.rest.is_some() || function.is_async {
            return Err(SmeltError::unsupported(
                span,
                "a callback function with a `*args` tail or an `async def` is not supported",
            ));
        }
        let mut closure_body = Body::new(None, span);
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (position, &param_ty) in function.params.iter().enumerate() {
            let name = self.intern_name(&format!("__callback_param_{position}"));
            let local = closure_body.push_local(LocalDecl { name: Some(name), ty: param_ty, mutable: false, span });
            closure_body.params.push(local);
            params.push(Param { name, local, ty: param_ty, span });
            args.push(closure_body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: param_ty, span }));
        }
        let callee = closure_body.push_expr(HirExpr { kind: ExprKind::Item(item), ty: item_ty, span });
        let call = closure_body.push_expr(HirExpr {
            kind: ExprKind::Call { callee, args },
            ty: function.return_ty,
            span,
        });
        closure_body.blocks[0].tail = Some(call);
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.intern_type(Type::Function(FunctionType {
            params: function.params.clone(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: false,
            may_throw: function.may_throw,
        }));
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body: body_id,
                name: None,
                length: None,
                function_item: None,
                this_binding: smelt_hir::ClosureThis::Own,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower `set(value)` for the currently supported direct container inputs.
    fn set_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::Set(_)) => (ExprKind::SetCopy { set: source }, source_ty),
            Some(Type::List(item_type)) => {
                let item_ty = *item_type;
                (
                    ExprKind::ListToSet { list: source },
                    self.intern_type(Type::Set(item_ty)),
                )
            }
            Some(Type::Tuple(items)) => {
                let item_ty = Self::homogeneous_tuple_item_ty(items, span)?;
                (
                    ExprKind::TupleToSet { tuple: source },
                    self.intern_type(Type::Set(item_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "set(value) currently requires a set, list, or homogeneous tuple value",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `dict(value)` for the currently supported direct container inputs.
    fn dict_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) => (ExprKind::DictCopy { dict: source }, source_ty),
            Some(Type::List(item_ty)) => {
                let Some(Type::Tuple(items)) = self.ctx.krate.types.get(*item_ty) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict(value) list input must contain 2-item tuples",
                    ));
                };
                let [key_ty, value_ty] = items.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict(value) list input must contain 2-item tuples",
                    ));
                };
                (
                    ExprKind::ListPairsToDict { list: source },
                    self.intern_type(Type::Dict(*key_ty, *value_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "dict(value) currently requires a dict value or list of 2-item tuples",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `tuple(value)` for the currently supported direct container inputs.
    fn tuple_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Tuple(_)) => Ok(Some(source)),
            Some(Type::List(item_ty)) => {
                let Some(tuple_ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) requires a tuple type annotation",
                    ));
                };
                let Some(Type::Tuple(items)) = self.ctx.krate.types.get(tuple_ty) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) type annotation must be tuple[...]",
                    ));
                };
                if !items.iter().all(|tuple_item| tuple_item == item_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) annotation item types must match the list item type",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::ListToTuple { list: source },
                    ty: tuple_ty,
                    span,
                })))
            }
            _ => Err(SmeltError::unsupported(
                span,
                "tuple(value) currently requires a tuple value or annotated list value",
            )),
        }
    }

    // Homogeneous tuple helpers continue in `math.rs`.
}
