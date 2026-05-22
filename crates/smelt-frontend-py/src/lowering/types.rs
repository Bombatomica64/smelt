impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Type annotation lowering
    // -----------------------------------------------------------------------

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
                Ok(self.intern_type(Type::Optional(inner)))
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
                Ok(self.intern_type(Type::Optional(*inner)))
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
                Ok(self.intern_type(Type::Union(types)))
            }
            (_, false) => Ok(self.intern_type(Type::Union(types))),
        }
    }

}
