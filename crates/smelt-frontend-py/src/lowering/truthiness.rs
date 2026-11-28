// Python truth testing.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Every place Python tests a value for truth (`if`, `elif`, `while`,
// `assert`, a conditional expression, a comprehension `if`, `not`, the
// operands `and` / `or` decide on, `bool(x)`, `filter(None, xs)`) asks the
// object: `__bool__`, else `__len__() != 0`, else true. For the builtin types
// that is: `False`, `0`, `0.0`, `""`, `None` and every empty container are
// false.
//
// HIR's container types are shared with TypeScript, whose truthiness makes
// every object true, so a non-`bool` condition cannot be left to codegen's
// `ToBool`: the condition is lowered here to a `bool` with Python's meaning.
//
// * `bool`: itself.
// * `int`, `float`, `str`, `None`: `ToBool`, whose rule for these
//   Python-specific types already is Python's (zero, NaN-free zero, empty).
// * `list`, `dict`, `set`: `len(x) != 0`.
// * a tuple: its arity, which the type fixes (`()` is false), with the value
//   still evaluated.
// * `T | None`: present, and the payload's own truth test. A payload whose
//   rule is `ToBool` keeps codegen's `is_some_and`; any other needs the
//   payload read, so the operand must be a local.
// * a class instance: its `__bool__()`, else `__len__() != 0`, else true.
// * a union of several types or a callable: `ToBool`, refused where its
//   rule is not Python's (a union holding a container).

impl ModuleBuilder<'_> {
    /// Lower `operand` as a Python truth test: a `bool` expression with
    /// Python's meaning (see the file header).
    fn python_truth(
        &mut self,
        operand: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        let bool_ty = self.intern_type(Type::Bool);
        match self.ctx.krate.types.get(operand_ty).cloned() {
            Some(Type::Bool) => Ok(operand),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_)) => Ok(self.nonempty_test(operand, span, body)),
            Some(Type::Tuple(elements)) => {
                // The arity is static; the operand still runs, for its effects.
                let decided = body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Bool(!elements.is_empty())),
                    ty: bool_ty,
                    span,
                });
                if elements.is_empty() {
                    let cast = self.bool_cast(operand, span, body);
                    return Ok(body.push_expr(HirExpr {
                        kind: ExprKind::Conditional { cond: cast, then_expr: decided, else_expr: decided },
                        ty: bool_ty,
                        span,
                    }));
                }
                Ok(self.bool_cast(operand, span, body))
            }
            Some(Type::Optional(inner, _)) => self.optional_truth(operand, inner, span, body),
            Some(Type::Class { name, .. }) => {
                if let Some(len) = self.py_collection_len(operand, span, body) {
                    let int_ty = self.intern_type(Type::Int);
                    let zero = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(0)), ty: int_ty, span });
                    return Ok(body.push_expr(HirExpr {
                        kind: ExprKind::BinOp { op: BinOp::NotEq, lhs: len, rhs: zero },
                        ty: bool_ty,
                        span,
                    }));
                }
                self.instance_truth(operand, name, span, body)
            }
            Some(Type::Union(members)) => {
                if members.iter().any(|member| !self.truth_is_to_bool(*member)) {
                    return Err(SmeltError::unsupported(
                        span,
                        "the truth test of a union holding a container or class instance is not supported; test a narrowed value",
                    ));
                }
                Ok(self.bool_cast(operand, span, body))
            }
            _ => Ok(self.bool_cast(operand, span, body)),
        }
    }

    /// Lower a test expression (`if`, `while`, ...) as a Python truth test.
    fn python_condition(&mut self, test: &Expr, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(test, body)?;
        self.python_truth(value, self.span(test.range()), body)
    }

    /// Whether `ty`'s Python truth test is codegen's `ToBool` (the scalar
    /// types and `None`), so it composes under `is_some_and`.
    fn truth_is_to_bool(&self, ty: TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr | Type::None)
        )
    }

    /// `ToBool` of `operand`.
    fn bool_cast(&mut self, operand: smelt_hir::ExprId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let ty = self.intern_type(Type::Bool);
        body.push_expr(HirExpr {
            kind: ExprKind::PrimitiveCast { op: PrimitiveCastOp::ToBool, operand },
            ty,
            span,
        })
    }

    /// `len(operand) != 0` for a sized builtin container.
    fn nonempty_test(&mut self, operand: smelt_hir::ExprId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let int_ty = self.intern_type(Type::Int);
        let bool_ty = self.intern_type(Type::Bool);
        let len = body.push_expr(HirExpr { kind: ExprKind::Len { operand }, ty: int_ty, span });
        let zero = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(0)), ty: int_ty, span });
        body.push_expr(HirExpr {
            kind: ExprKind::BinOp { op: BinOp::NotEq, lhs: len, rhs: zero },
            ty: bool_ty,
            span,
        })
    }

    /// The truth test of a `T | None` value: present, and the payload true.
    fn optional_truth(
        &mut self,
        operand: smelt_hir::ExprId,
        inner: TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if self.truth_is_to_bool(inner) {
            return Ok(self.bool_cast(operand, span, body));
        }
        let ExprKind::Local(local) = body.exprs[usize::try_from(operand.0).unwrap_or(usize::MAX)].kind else {
            return Err(SmeltError::unsupported(
                span,
                "the truth test of an optional container computed by an expression is not supported; bind it to a name first",
            ));
        };
        let bool_ty = self.intern_type(Type::Bool);
        let none_ty = self.intern_type(Type::None);
        let none = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span });
        let present = self.compare_pair(CmpOp::IsNot, operand, none, span, body)?;
        let payload = self.narrowed_local_read(local, inner, span, body);
        let payload_truth = self.python_truth(payload, span, body)?;
        let absent = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(false)), ty: bool_ty, span });
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Conditional { cond: present, then_expr: payload_truth, else_expr: absent },
            ty: bool_ty,
            span,
        }))
    }

    /// The truth test of a class instance: `__bool__()`, else
    /// `__len__() != 0`, else true (`ToBool` of an object).
    fn instance_truth(
        &mut self,
        operand: smelt_hir::ExprId,
        class_sym: Symbol,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        for dunder in ["__bool__", "__len__"] {
            let Some(method) = self.instance_method_item(class_sym, dunder) else {
                continue;
            };
            let Item::Function(function) = self.item_ref(method) else {
                continue;
            };
            let return_ty = function.return_ty;
            let method_sym = function.name;
            let call = body.push_expr(HirExpr {
                kind: ExprKind::Method { receiver: operand, method: method_sym, args: Vec::new() },
                ty: return_ty,
                span,
            });
            // `__len__` returns an `int`, whose truth is `!= 0`; `__bool__`
            // returns a `bool`.
            return self.python_truth(call, span, body);
        }
        Ok(self.bool_cast(operand, span, body))
    }

    /// The instance method `name` of class `class_sym` or a base.
    fn instance_method_item(&self, class_sym: Symbol, name: &str) -> Option<ItemId> {
        let class_name = self.ctx.krate.symbols.get(class_sym)?;
        if let Some(item) = self.class_methods.get(class_name).and_then(|methods| methods.get(name)) {
            return Some(*item);
        }
        let class = self.class_item_by_symbol(class_sym)?;
        if let Some(base) = class.base
            && let Some(item) = self.instance_method_item(base, name)
        {
            return Some(item);
        }
        self.class_method_item_by_name(class_name, name)
    }
}
