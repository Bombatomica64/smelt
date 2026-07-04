impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Type annotation lowering
    // -----------------------------------------------------------------------

    /// Resolve a function/method return type, reconciling any source-declared
    /// annotation with `ty`'s inferred *actual* returned type (issue #93).
    ///
    /// Behaviour, in order:
    /// * No annotation → use `ty`'s inferred return type; `None` if `ty` could
    ///   not resolve one (the caller then raises the explicit-annotation error).
    /// * Annotation present → lower it. If `ty` also resolved a representable
    ///   return type and it *differs* from the annotation, prefer `ty`'s: the
    ///   annotation may be stale/wider/narrower than what the body actually
    ///   returns, and lowering must follow the accurate type. If `ty` resolved
    ///   nothing (or the same type), keep the annotation.
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
                // Prefer ty's inferred actual return type when it diverges.
                Some(match inferred {
                    Some(inferred_ty) if inferred_ty != declared => inferred_ty,
                    _ => declared,
                })
            }
        }
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
