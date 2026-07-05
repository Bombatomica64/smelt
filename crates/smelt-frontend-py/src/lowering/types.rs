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

    /// Lower a bare name in annotation position (e.g. `int`, `str`, `MyClass`).
    fn name_annotation(&mut self, name: &str, range: TextRange) -> Result<TypeId, SmeltError> {
        let span = self.span(range);
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
