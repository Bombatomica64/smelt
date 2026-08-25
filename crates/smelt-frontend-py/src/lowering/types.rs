impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Type annotation lowering
    // -----------------------------------------------------------------------

    /// Resolve a function/method return type, reconciling any source-declared
    /// annotation with `ty`'s inferred *actual* returned type (issue #93).
    ///
    /// The reconciliation is **annotation-respecting**: an explicit source
    /// contract is honoured and is never widened just because `ty` infers a
    /// broader type for the body. Behaviour, in order:
    ///
    /// * No annotation → use `ty`'s inferred return type; `None` if `ty` could
    ///   not resolve one (the caller then raises the explicit-annotation error).
    /// * Annotation present → lower it to `declared`, then:
    ///   * `ty` resolved nothing, or resolved the same type → keep `declared`.
    ///   * `declared` is assignable to `ty`'s inferred type — i.e. the
    ///     annotation is an equal-or-narrower, still-valid description of what
    ///     the body returns (e.g. `-> float` while `ty` infers `int | float`)
    ///     → keep `declared`. This is the fix for the #93 regression: an
    ///     annotated `-> float` must stay `f64`, not degrade into an erased
    ///     union.
    ///   * Otherwise `ty` inferred a strictly narrower / genuinely more precise
    ///     type than the annotation (`declared` is *not* assignable to it, e.g.
    ///     an annotation of `object`/`float` while the body only ever returns
    ///     `int`) → prefer `ty`'s refinement.
    ///
    /// The `func` node's start offset is the key `smelt-py-types` records return
    /// types under.
    fn resolve_return_ty(
        &mut self,
        func: &StmtFunctionDef,
        annotation: Option<&Expr>,
    ) -> Option<TypeId> {
        let inferred = self
            .resolved_types
            .return_type_at(func.start())
            .map(ToOwned::to_owned)
            .and_then(|spelling| self.resolved_spelling_to_hir(&spelling));
        match annotation {
            None => inferred,
            Some(ann) => {
                let declared = self.annotation_to_hir(ann).ok()?;
                Some(match inferred {
                    // A genuine refinement: `ty` inferred a strictly narrower
                    // type than the annotation, so the annotation is not a valid
                    // (equal-or-narrower) description of it. Follow `ty`.
                    Some(inferred_ty)
                        if inferred_ty != declared
                            && !self.type_assignable_to(declared, inferred_ty) =>
                    {
                        inferred_ty
                    }
                    // No inference, an identical type, or the annotation is a
                    // still-valid equal-or-narrower contract: respect the
                    // explicit annotation and never widen it.
                    _ => declared,
                })
            }
        }
    }

    /// Whether the `value` HIR type is assignable to (a subtype of, or equal to)
    /// the `target` HIR type under the subset of Python typing rules the frontend
    /// needs to reconcile annotations with `ty`'s inference (issue #93).
    ///
    /// This is intentionally narrow and conservative — it exists only to decide
    /// whether an explicit annotation remains a valid description of `ty`'s
    /// inferred type, so it errs toward `false` for shapes it does not model:
    ///
    /// * Equal types are assignable.
    /// * `int` is assignable to `float` (Python's numeric tower).
    /// * `value` is assignable to a `Union`/`Optional` `target` when it is
    ///   assignable to at least one member (Optional is treated as `T | None`).
    /// * A `Union`/`Optional` `value` is assignable to `target` when *every*
    ///   member is assignable to `target`.
    ///
    /// Anything else (containers, functions, unrelated classes) is only
    /// assignable when structurally equal. Returning `false` on the unmodelled
    /// cases is safe: the caller then keeps the explicit annotation rather than
    /// widening, which is the conservative choice for a source contract.
    fn type_assignable_to(&self, value: TypeId, target: TypeId) -> bool {
        if value == target {
            return true;
        }
        let types = &self.ctx.krate.types;
        let (Some(value_kind), Some(target_kind)) = (types.get(value), types.get(target)) else {
            return false;
        };

        // Python numeric widening: an `int` value satisfies a `float` contract.
        if is_int_like(value_kind) && is_float_like(target_kind) {
            return true;
        }

        // A union/optional on the value side is assignable only when every member
        // is; an `Optional(T)` (`T | None`) member must include `None` so it is
        // not silently narrowed to `T`.
        if let Some(members) = self.union_like_members(value_kind) {
            return members
                .into_iter()
                .all(|member| self.type_assignable_to(member, target));
        }

        // A union/optional on the target side accepts the value when any member
        // does.
        if let Some(members) = self.union_like_members(target_kind) {
            return members
                .into_iter()
                .any(|member| self.type_assignable_to(value, member));
        }

        false
    }

    /// Flatten a `Union`/`Optional` type into its member [`TypeId`]s, or `None`
    /// when `ty` is not a union-like type.
    ///
    /// `Optional(T)` expands to `[T, None]` so an optional on the assignable-to
    /// *source* side is not silently narrowed by dropping its `None` arm. The
    /// `None` member is looked up read-only from the interner; if no `Type::None`
    /// has been interned (it always has in practice, being pervasive) the inner
    /// type is returned alone.
    fn union_like_members(&self, ty: &Type) -> Option<Vec<TypeId>> {
        match ty {
            Type::Union(members) => Some(members.clone()),
            Type::Optional(inner) => {
                let mut members = vec![*inner];
                if let Some(none_id) = self.find_interned_type(&Type::None) {
                    members.push(none_id);
                }
                Some(members)
            }
            _ => None,
        }
    }

    /// Find the [`TypeId`] of an already-interned type without mutating the
    /// interner. Read-only counterpart to [`Self::intern_type`] used by the
    /// annotation/inference reconciliation, which must stay `&self`.
    fn find_interned_type(&self, needle: &Type) -> Option<TypeId> {
        let idx = self
            .ctx
            .krate
            .types
            .all()
            .iter()
            .position(|existing| existing == needle)?;
        u32::try_from(idx).ok().map(TypeId)
    }

    /// Return the `ty`-resolved HIR type for parameter `p`, if `ty` resolved a
    /// representable type for it.
    ///
    /// Used when the source omits the parameter's annotation (issue #93). The
    /// key is the parameter node's start offset, matching how `smelt-py-types`
    /// records resolved parameter types.
    fn resolved_param_ty(&mut self, p: &ruff_python_ast::Parameter) -> Option<TypeId> {
        let spelling = self.resolved_types.param_type_at(p.start())?.to_owned();
        self.resolved_spelling_to_hir(&spelling)
    }

    /// Lower a `ty`-resolved type *spelling* (a canonical Python type string
    /// such as `"int"`, `"list[int]"`, or `"str | None"`) to a HIR [`TypeId`].
    ///
    /// The spelling is re-parsed as a Python annotation expression and routed
    /// through the same [`Self::annotation_to_hir`] path source annotations use,
    /// so unions, `list[T]`, custom classes, etc. all lower identically to hand-
    /// written annotations. Returns `None` if the spelling does not parse or
    /// lands on an annotation form the frontend cannot lower, letting the caller
    /// fall back to its explicit-annotation requirement (an explicit boundary,
    /// never a silent widening).
    fn resolved_spelling_to_hir(&mut self, spelling: &str) -> Option<TypeId> {
        let parsed = ruff_python_parser::parse_expression(spelling).ok()?;
        let expr = parsed.syntax().body.as_ref();
        self.annotation_to_hir(expr).ok()
    }

    /// Lower a Python type annotation expression to a HIR [`TypeId`].
    fn annotation_to_hir(&mut self, annotation: &Expr) -> Result<TypeId, SmeltError> {
        if let Expr::Name(name) = annotation {
            self.name_annotation(name.id.as_str(), name.range)
        } else if let Expr::StringLiteral(name) = annotation {
            self.name_annotation(name.value.to_str().as_ref(), name.range)
        } else if let Expr::NoneLiteral(_) = annotation {
            Ok(self.intern_type(Type::None))
        } else if let Expr::Attribute(attr) = annotation {
            // `typing.Optional[T]`, `typing.Union[T, U]` etc.
            self.name_annotation(attr.attr.as_str(), attr.range)
        } else if let Expr::Subscript(sub) = annotation {
            self.subscript_annotation(sub)
        } else if let Expr::BinOp(b) = annotation {
            // PEP 604: `T | U`
            if b.op == Operator::BitOr {
                self.bitor_annotation(annotation)
            } else {
                Err(SmeltError::unsupported(
                    self.span(annotation.range()),
                    "unsupported type annotation form",
                ))
            }
        } else if let Expr::Tuple(t) = annotation {
            // Bare tuple in annotation position (e.g. inside Callable)
            let items = t
                .elts
                .iter()
                .map(|e| self.annotation_to_hir(e))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(self.intern_type(Type::Tuple(items)))
        } else {
            Err(SmeltError::unsupported(
                self.span(annotation.range()),
                "unsupported type annotation form",
            ))
        }
    }

    /// Whether a module-level statement declares a *type alias*.
    ///
    /// Three spellings are recognized, all of which are unambiguously
    /// type-level in valid typed Python:
    ///
    /// * `Result: TypeAlias = Ok | Err` — the explicit PEP 613 annotation;
    /// * `type Result = Ok | Err` — the PEP 695 statement;
    /// * `Result = Union[Ok, Err]` / `Result = Ok | Err` — the pre-3.12 idiom,
    ///   accepted only when the right-hand side is itself a type expression
    ///   (`Union[..]`/`Optional[..]` or a PEP 604 `|` chain), so an ordinary
    ///   value assignment is never mistaken for an alias.
    fn is_type_alias_statement(stmt: &Stmt) -> bool {
        Self::type_alias_parts(stmt).is_some()
    }

    /// Split a type-alias statement into its declared name and aliased type
    /// expression, or `None` when the statement is not an alias.
    fn type_alias_parts(stmt: &Stmt) -> Option<(&str, &Expr)> {
        /// Whether an annotation names PEP 613's `TypeAlias`.
        fn is_type_alias_marker(annotation: &Expr) -> bool {
            match annotation {
                Expr::Name(name) => name.id.as_str() == "TypeAlias",
                Expr::Attribute(attribute) => attribute.attr.as_str() == "TypeAlias",
                Expr::StringLiteral(literal) => literal.value.to_str() == "TypeAlias",
                _ => false,
            }
        }

        /// Whether an expression is unambiguously a *type* expression.
        ///
        /// Only the forms that cannot appear as ordinary program data: a
        /// `Union[..]`/`Optional[..]` subscript, or a PEP 604 `A | B` chain of
        /// type names.
        fn is_type_expression(value: &Expr) -> bool {
            match value {
                Expr::Subscript(subscript) => matches!(
                    expr_type_name(&subscript.value),
                    Some("Union" | "Optional")
                ),
                Expr::BinOp(binary) => {
                    binary.op == Operator::BitOr
                        && is_type_operand(&binary.left)
                        && is_type_operand(&binary.right)
                }
                _ => false,
            }
        }

        /// One side of a PEP 604 union: a name, a qualified name, `None`, a
        /// subscripted generic, or a nested union.
        fn is_type_operand(value: &Expr) -> bool {
            match value {
                Expr::Name(_) | Expr::Attribute(_) | Expr::NoneLiteral(_) => true,
                Expr::Subscript(_) => true,
                Expr::BinOp(binary) => {
                    binary.op == Operator::BitOr
                        && is_type_operand(&binary.left)
                        && is_type_operand(&binary.right)
                }
                _ => false,
            }
        }

        match stmt {
            // `type Result = Ok | Err` (PEP 695).
            Stmt::TypeAlias(alias) => match alias.name.as_ref() {
                Expr::Name(name) => Some((name.id.as_str(), alias.value.as_ref())),
                _ => None,
            },
            // `Result: TypeAlias = Ok | Err` (PEP 613).
            Stmt::AnnAssign(assign) => {
                let Expr::Name(target) = assign.target.as_ref() else {
                    return None;
                };
                if !is_type_alias_marker(&assign.annotation) {
                    return None;
                }
                assign
                    .value
                    .as_deref()
                    .map(|value| (target.id.as_str(), value))
            }
            // `Result = Union[Ok, Err]` (pre-3.12 idiom).
            Stmt::Assign(assign) => {
                let [Expr::Name(target)] = assign.targets.as_slice() else {
                    return None;
                };
                is_type_expression(&assign.value)
                    .then(|| (target.id.as_str(), assign.value.as_ref()))
            }
            _ => None,
        }
    }

    /// Register a module-level type alias, returning whether the statement was
    /// one.
    ///
    /// The aliased type is lowered eagerly through the ordinary annotation path,
    /// so `Result: TypeAlias = Ok | Err` resolves to the same `Type::Union` an
    /// inline `Ok | Err` annotation produces — which is what lets a method call
    /// on a `Result` receiver dispatch across the union's arms.
    ///
    /// A right-hand side the annotation lowerer cannot represent is skipped
    /// rather than reported: the alias then stays unresolved and any *use* of it
    /// reports at the use site, where the diagnostic points at real code.
    fn register_type_alias_statement(&mut self, stmt: &Stmt) -> bool {
        let Some((name, value)) = Self::type_alias_parts(stmt) else {
            return false;
        };
        let name = name.to_owned();
        let mut params = Vec::new();
        self.collect_alias_params(value, &mut params);
        if let Ok(ty) = self.annotation_to_hir(value) {
            self.type_aliases.insert(name, TypeAliasDef { ty, params });
        }
        true
    }

    /// Collect the type-parameter names an alias's right-hand side mentions, in
    /// first-appearance order.
    ///
    /// Pre-PEP-695 Python does not declare an alias's parameters; they are
    /// simply whichever `TypeVar`s appear in the aliased expression, and a
    /// subscripted use supplies them left to right. Walking the source
    /// expression (rather than the lowered type) keeps that order exact.
    fn collect_alias_params(&self, value: &Expr, params: &mut Vec<String>) {
        match value {
            Expr::Name(name) => {
                let name = name.id.as_str();
                if self.type_param_names.iter().any(|known| known == name)
                    && !params.iter().any(|seen| seen == name)
                {
                    params.push(name.to_owned());
                }
            }
            Expr::Subscript(subscript) => {
                self.collect_alias_params(&subscript.value, params);
                self.collect_alias_params(&subscript.slice, params);
            }
            Expr::Tuple(tuple) => {
                for element in &tuple.elts {
                    self.collect_alias_params(element, params);
                }
            }
            Expr::BinOp(binary) => {
                self.collect_alias_params(&binary.left, params);
                self.collect_alias_params(&binary.right, params);
            }
            _ => {}
        }
    }

    /// Resolve a subscripted type alias (`Result[int, str]`) by substituting the
    /// supplied arguments for the alias's parameters.
    ///
    /// Returns `None` when the base name is not an alias, so the ordinary
    /// generic handling still runs. A use that supplies the wrong number of
    /// arguments substitutes what it can and leaves the rest as declared, which
    /// keeps the alias usable while `ty` reports the arity error at the source.
    fn alias_subscript_annotation(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
    ) -> Result<Option<TypeId>, SmeltError> {
        let Some(name) = expr_type_name(&sub.value) else {
            return Ok(None);
        };
        let Some(alias) = self.type_aliases.get(name).cloned() else {
            return Ok(None);
        };
        let arg_exprs: Vec<&Expr> = match sub.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            single => vec![single],
        };
        let mut substitutions = HashMap::new();
        for (param, arg) in alias.params.iter().zip(arg_exprs) {
            let arg_ty = self.annotation_to_hir(arg)?;
            substitutions.insert(param.clone(), arg_ty);
        }
        Ok(Some(self.substitute_alias_params(alias.ty, &substitutions)))
    }

    /// Replace an alias's parameter placeholders with concrete arguments.
    ///
    /// The Python frontend lowers an unresolved annotation name to an
    /// argument-free `Type::Class` placeholder, so a parameter appears as a
    /// class whose name matches. Only that shape is rewritten; every other type
    /// is rebuilt with its children substituted.
    fn substitute_alias_params(
        &mut self,
        ty: TypeId,
        substitutions: &HashMap<String, TypeId>,
    ) -> TypeId {
        if substitutions.is_empty() {
            return ty;
        }
        let Some(kind) = self.ctx.krate.types.get(ty).cloned() else {
            return ty;
        };
        let substitute_all = |builder: &mut Self, items: &[TypeId]| {
            items
                .iter()
                .map(|item| builder.substitute_alias_params(*item, substitutions))
                .collect::<Vec<_>>()
        };
        match kind {
            Type::Class { name, args } => {
                if args.is_empty()
                    && let Some(param_name) = self.ctx.krate.symbols.get(name)
                    && let Some(replacement) = substitutions.get(param_name)
                {
                    return *replacement;
                }
                let args = substitute_all(self, &args);
                self.intern_type(Type::Class { name, args })
            }
            Type::TypeParam { name } => self
                .ctx
                .krate
                .symbols
                .get(name)
                .and_then(|param_name| substitutions.get(param_name).copied())
                .unwrap_or(ty),
            Type::Union(items) => {
                let items = substitute_all(self, &items);
                self.intern_type(Type::Union(items))
            }
            Type::Tuple(items) => {
                let items = substitute_all(self, &items);
                self.intern_type(Type::Tuple(items))
            }
            Type::Optional(inner) => {
                let inner = self.substitute_alias_params(inner, substitutions);
                self.intern_type(Type::Optional(inner))
            }
            Type::List(item) => {
                let item = self.substitute_alias_params(item, substitutions);
                self.intern_type(Type::List(item))
            }
            Type::Set(item) => {
                let item = self.substitute_alias_params(item, substitutions);
                self.intern_type(Type::Set(item))
            }
            Type::Future(item) => {
                let item = self.substitute_alias_params(item, substitutions);
                self.intern_type(Type::Future(item))
            }
            Type::Dict(key, value) => {
                let key = self.substitute_alias_params(key, substitutions);
                let value = self.substitute_alias_params(value, substitutions);
                self.intern_type(Type::Dict(key, value))
            }
            _ => ty,
        }
    }

    /// Lower a bare name in annotation position (e.g. `int`, `str`, `MyClass`).
    fn name_annotation(&mut self, name: &str, range: TextRange) -> Result<TypeId, SmeltError> {
        let span = self.span(range);
        // A module-level type alias resolves to the type it names.
        if let Some(alias) = self.type_aliases.get(name) {
            return Ok(alias.ty);
        }
        match name {
            "int" => Ok(self.intern_type(Type::Int)),
            "float" => Ok(self.intern_type(Type::Float)),
            "str" => Ok(self.intern_type(Type::String)),
            "bool" => Ok(self.intern_type(Type::Bool)),
            "None" | "NoneType" => Ok(self.intern_type(Type::None)),
            "object" => {
                // Top type — no exact HIR equivalent, map to opaque Class.
                let sym = self.intern_name("object");
                Ok(self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                }))
            }
            // Bare generic names without args are an error.
            "Optional" | "Union" | "List" | "Dict" | "Set" | "Tuple" | "Callable" | "Awaitable" => {
                Err(SmeltError::unsupported(
                    span,
                    format!("'{name}' requires type arguments, e.g. {name}[T]"),
                ))
            }
            other => {
                // Unknown → assume a class type (will be resolved later).
                let sym = self.intern_name(other);
                Ok(self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                }))
            }
        }
    }

    /// Lower a subscript annotation: `list[T]`, `Optional[T]`, `dict[K, V]`, …
    fn subscript_annotation(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
    ) -> Result<TypeId, SmeltError> {
        // `Result[int, str]` — a parameterised module-level alias.
        if let Some(aliased) = self.alias_subscript_annotation(sub)? {
            return Ok(aliased);
        }
        let span = self.span(sub.range);
        let type_name = expr_type_name(&sub.value).unwrap_or("");

        match type_name {
            "list" | "List" => {
                let item = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::List(item)))
            }
            "set" | "Set" => {
                let item = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::Set(item)))
            }
            "dict" | "Dict" => {
                let (k, v) = two_type_args(&sub.slice, span)?;
                let key = self.annotation_to_hir(k)?;
                let val = self.annotation_to_hir(v)?;
                Ok(self.intern_type(Type::Dict(key, val)))
            }
            "tuple" | "Tuple" => {
                // `tuple[()]` is the empty tuple; otherwise lower each element.
                let items = if let Expr::Tuple(t) = sub.slice.as_ref() {
                    t.elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    vec![self.annotation_to_hir(&sub.slice)?]
                };
                Ok(self.intern_type(Type::Tuple(items)))
            }
            "Optional" => {
                let inner = self.annotation_to_hir(&sub.slice)?;
                Ok(smelt_hir::type_normalize::optional_of(
                    &mut self.ctx.krate.types,
                    inner,
                ))
            }
            "Union" => {
                let types = if let Expr::Tuple(t) = sub.slice.as_ref() {
                    t.elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    return self.annotation_to_hir(&sub.slice);
                };
                self.union_from_types(types, span)
            }
            "Callable" => {
                // `Callable[[P1, P2], R]`
                let (param_list_expr, return_expr) = two_type_args(&sub.slice, span)?;
                let params = if let Expr::List(l) = param_list_expr {
                    l.elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    return Err(SmeltError::unsupported(
                        span,
                        "Callable first argument must be a list of param types, e.g. [int, str]",
                    ));
                };
                let return_ty = self.annotation_to_hir(return_expr)?;
                Ok(self.intern_type(Type::Function(FunctionType {
                    params,
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: false,
                    may_throw: false,
                })))
            }
            "Awaitable" | "Coroutine" => {
                let inner = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::Future(inner)))
            }
            _ => {
                // Generic class: `Foo[T, U]`
                let sym = self.intern_name(type_name);
                let args = if let Expr::Tuple(t) = sub.slice.as_ref() {
                    t.elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    vec![self.annotation_to_hir(&sub.slice)?]
                };
                Ok(self.intern_type(Type::Class { name: sym, args }))
            }
        }
    }

    /// Lower a PEP 604 `T | U | V` expression to Optional or Union.
    fn bitor_annotation(&mut self, expr: &Expr) -> Result<TypeId, SmeltError> {
        let span = self.span(expr.range());
        let mut parts: Vec<&Expr> = Vec::new();
        collect_bitor_parts(expr, &mut parts);
        let types = parts
            .iter()
            .map(|p| self.annotation_to_hir(p))
            .collect::<Result<Vec<_>, _>>()?;
        self.union_from_types(types, span)
    }

    /// Apply Optional vs Union logic — mirrors `ts_type_to_hir`'s union branch.
    fn union_from_types(
        &mut self,
        mut types: Vec<TypeId>,
        span: Span,
    ) -> Result<TypeId, SmeltError> {
        let none_ty = self.intern_type(Type::None);
        let has_none = types.iter().any(|&t| t == none_ty);
        types.retain(|&t| t != none_ty);

        match (types.len(), has_none) {
            (0, _) => Ok(none_ty),
            (1, true) => {
                let [inner] = types.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "internal error: union normalization expected one non-None type",
                    ));
                };
                Ok(smelt_hir::type_normalize::optional_of(
                    &mut self.ctx.krate.types,
                    *inner,
                ))
            }
            (1, false) => {
                let [inner] = types.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "internal error: union normalization expected one non-None type",
                    ));
                };
                Ok(*inner)
            }
            (_, true) => {
                types.push(none_ty);
                let union = self.intern_type(Type::Union(types));
                Ok(smelt_hir::type_normalize::normalize_type(
                    &mut self.ctx.krate.types,
                    union,
                    smelt_hir::type_normalize::NormalizeOptions::default(),
                ))
            }
            (_, false) => {
                let union = self.intern_type(Type::Union(types));
                Ok(smelt_hir::type_normalize::normalize_type(
                    &mut self.ctx.krate.types,
                    union,
                    smelt_hir::type_normalize::NormalizeOptions::default(),
                ))
            }
        }
    }

}

/// Whether `ty` is the `int` primitive, for the numeric-tower widening rule in
/// [`ModuleBuilder::type_assignable_to`].
fn is_int_like(ty: &Type) -> bool {
    matches!(ty, Type::Int)
}

/// Whether `ty` is the `float` primitive, for the numeric-tower widening rule in
/// [`ModuleBuilder::type_assignable_to`] (an `int` value satisfies a `float`
/// contract in Python).
fn is_float_like(ty: &Type) -> bool {
    matches!(ty, Type::Float)
}
