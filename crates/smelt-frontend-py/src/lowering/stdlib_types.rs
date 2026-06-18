impl ModuleBuilder<'_> {
    /// Return whether a type can be represented by Python JSON serialization helpers.
    fn is_json_serializable_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
                self.is_json_serializable_type(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.is_json_serializable_type(*item)),
            Some(Type::Dict(key, value)) => {
                matches!(self.ctx.krate.types.get(*key), Some(Type::String))
                    && self.is_json_serializable_type(*value)
            }
            _ => false,
        }
    }

    /// Lower primitive Python conversion builtins for bool, int, float, and string values.
    pub(super) fn primitive_cast_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let (op, result_ty) = match name.id.as_str() {
            "bool" => (PrimitiveCastOp::ToBool, Type::Bool),
            "int" => (PrimitiveCastOp::ToInt, Type::Int),
            "float" => (PrimitiveCastOp::ToFloat, Type::Float),
            "str" => (PrimitiveCastOp::ToString, Type::String),
            _ => return Ok(None),
        };
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "primitive conversions currently support exactly one positional argument",
            ));
        }
        let operand = self.expression(&call.arguments.args[0], body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "primitive conversions currently support bool, int, float, and str values",
            ));
        }
        let ty = self.intern_type(result_ty);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::PrimitiveCast { op, operand },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `sum(values)` calls for int and float lists.
    pub(super) fn numeric_sum_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "sum" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "sum() currently supports exactly one list argument",
            ));
        }
        let list = self.expression(&call.arguments.args[0], body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sum() argument must be an int or float list",
            ));
        };
        if !matches!(
            self.ctx.krate.types.get(*item_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sum() argument must be an int or float list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSum { list },
            ty: *item_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `all(values)` and `any(values)` calls for boolean lists.
    pub(super) fn bool_fold_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match name.id.as_str() {
            "all" => BoolFoldOp::All,
            "any" => BoolFoldOp::Any,
            _ => return Ok(None),
        };
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "all() and any() currently support exactly one bool list argument",
            ));
        }
        let list = self.expression(&call.arguments.args[0], body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "all() and any() argument must be a bool list",
            ));
        };
        if self.ctx.krate.types.get(*item_ty) != Some(&Type::Bool) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "all() and any() argument must be a bool list",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListBoolFold { op, list },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `sorted(values, key=..., reverse=...)` for sortable lists.
    ///
    /// Lists of scalar items sort directly; an optional `key=` lambda or local
    /// callable sorts by its mapped value, and `reverse=True` produces a
    /// descending order. This mirrors the TypeScript `Array.prototype.sort`
    /// comparator support so the two frontends keep parity.
    pub(super) fn sorted_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "sorted" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "sorted() currently supports exactly one list argument",
            ));
        }
        let list = self.expression(&call.arguments.args[0], body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sorted() argument must be a sortable list",
            ));
        };
        let element_ty = *item_ty;
        let (key, reverse) =
            self.sort_keyword_arguments(&call.arguments.keywords, element_ty, body)?;
        if key.is_none()
            && !matches!(
                self.ctx.krate.types.get(element_ty),
                Some(Type::Bool | Type::Int | Type::Float | Type::String)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sorted() argument must be a sortable list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSorted {
                list,
                key,
                reverse,
            },
            ty: list_ty,
            span: self.span(call.range),
        })))
    }

    /// Parse Python `key=` and `reverse=` sort keyword arguments.
    ///
    /// A `key` lambda or local callable lowers to a closure mapping one list
    /// item to a scalar sort value; `key=None` is treated as absent. `reverse`
    /// accepts only boolean literals because non-literal flags would need
    /// runtime branching that the sort lowering does not model yet.
    fn sort_keyword_arguments(
        &mut self,
        keywords: &[ruff_python_ast::Keyword],
        element_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<(Option<smelt_hir::ExprId>, bool), SmeltError> {
        let mut key = None;
        let mut reverse = false;
        for keyword in keywords {
            match keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) {
                Some("key") => {
                    if matches!(&keyword.value, Expr::NoneLiteral(_)) {
                        continue;
                    }
                    let callback =
                        self.python_callback_argument(&keyword.value, &[element_ty], body)?;
                    if !matches!(
                        self.ctx.krate.types.get(callback.return_ty),
                        Some(Type::Bool | Type::Int | Type::Float | Type::String)
                    ) {
                        return Err(SmeltError::unsupported(
                            self.span(keyword.range),
                            "sort key must return bool, int, float, or str",
                        ));
                    }
                    key = Some(callback.expr);
                }
                Some("reverse") => match &keyword.value {
                    Expr::BooleanLiteral(value) => reverse = value.value,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(keyword.range),
                            "sort reverse must be a boolean literal",
                        ));
                    }
                },
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(keyword.range),
                        "sort supports only key and reverse keyword arguments",
                    ));
                }
            }
        }
        Ok((key, reverse))
    }

    /// Lower Python `reversed(values)` calls for list values.
    pub(super) fn reversed_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "reversed" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "reversed() currently supports exactly one list argument and no keywords",
            ));
        }
        let list = self.expression(&call.arguments.args[0], body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "reversed() argument must be a list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListReversed { list },
            ty: list_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `enumerate(values)` as a materialized list of `(index, value)` tuples.
    pub(super) fn enumerate_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "enumerate" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "enumerate() currently supports exactly one iterable argument and no start",
            ));
        }
        let raw_iter = self.expression(&call.arguments.args[0], body)?;
        let list = self.for_iterable(raw_iter, body);
        let list_ty = Self::expr_ty(body, list);
        let item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "enumerate() argument must be a list, set, or dict",
            ));
        };
        let int_ty = self.intern_type(Type::Int);
        let tuple_ty = self.intern_type(Type::Tuple(vec![int_ty, item_ty]));
        let ty = self.intern_type(Type::List(tuple_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListEnumerate { list },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `zip(left, right)` as a materialized list of pair tuples.
    pub(super) fn zip_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "zip" {
            return Ok(None);
        }
        if call.arguments.args.len() != 2 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "zip() currently supports exactly two iterable arguments",
            ));
        }
        let raw_left = self.expression(&call.arguments.args[0], body)?;
        let left = self.for_iterable(raw_left, body);
        let raw_right = self.expression(&call.arguments.args[1], body)?;
        let right = self.for_iterable(raw_right, body);
        let left_ty = Self::expr_ty(body, left);
        let right_ty = Self::expr_ty(body, right);
        let left_item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(left_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "zip() left argument must be a list, set, or dict",
            ));
        };
        let right_item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(right_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "zip() right argument must be a list, set, or dict",
            ));
        };
        let tuple_ty = self.intern_type(Type::Tuple(vec![left_item_ty, right_item_ty]));
        let ty = self.intern_type(Type::List(tuple_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListZip { left, right },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `range(...)` as a materialized `list[int]`.
    pub(super) fn range_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "range" {
            return Ok(None);
        }
        let span = self.span(call.range);
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "range() keyword arguments are not supported",
            ));
        }
        let int_ty = self.intern_type(Type::Int);
        let int_literal = |hir_body: &mut Body, value| {
            hir_body.push_expr(HirExpr {
                kind: ExprKind::Literal(smelt_hir::Literal::Int(value)),
                ty: int_ty,
                span,
            })
        };
        let (start, end, step) = match call.arguments.args.as_ref() {
            [end_expr] => (
                int_literal(body, 0),
                self.expression(end_expr, body)?,
                int_literal(body, 1),
            ),
            [start_expr, end_expr] => (
                self.expression(start_expr, body)?,
                self.expression(end_expr, body)?,
                int_literal(body, 1),
            ),
            [start_expr, end_expr, step_expr] => (
                self.expression(start_expr, body)?,
                self.expression(end_expr, body)?,
                self.expression(step_expr, body)?,
            ),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "range() requires one, two, or three integer arguments",
                ));
            }
        };
        for bound in [start, end, step] {
            if self.ctx.krate.types.get(Self::expr_ty(body, bound)) != Some(&Type::Int) {
                return Err(SmeltError::unsupported(
                    span,
                    "range() arguments must be integers",
                ));
            }
        }
        let ty = self.intern_type(Type::List(int_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListRange { start, end, step },
            ty,
            span,
        })))
    }

    /// Lower Python `list.extend(other)` calls for same-typed lists.
    pub(super) fn list_extend_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "extend" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.extend() requires exactly one list argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        let other = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, other) != list_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.extend() argument must match the receiver list type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListExtend { list, other },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.insert(index, item)` calls with integer indexes.
    pub(super) fn list_insert_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "insert" {
            return Ok(None);
        }
        if call.arguments.args.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.insert() requires index and item arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let index = self.expression(&call.arguments.args[0], body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, index)),
            Some(Type::Int)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.insert() index must be int",
            ));
        }
        let item = self.expression(&call.arguments.args[1], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "list.insert() item must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListInsert { list, index, item },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.index(item)` calls without start or stop arguments.
    pub(super) fn list_index_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "index" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.index() currently supports exactly one item argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.index() item must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::Int);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListIndex { list, item },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.remove(item)` calls.
    pub(super) fn list_remove_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "remove" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.remove() requires exactly one item argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.remove() item must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListRemove { list, item },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.sort(key=..., reverse=...)` calls in place.
    ///
    /// Scalar lists sort directly; an optional `key=` callable sorts by its
    /// mapped value and `reverse=True` orders descending, matching the
    /// `sorted()` lowering and the TypeScript array sort comparator support.
    pub(super) fn list_sort_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "sort" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.sort() currently supports no positional arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let (key, reverse) =
            self.sort_keyword_arguments(&call.arguments.keywords, element_ty, body)?;
        if key.is_none()
            && !matches!(
                self.ctx.krate.types.get(element_ty),
                Some(Type::Bool | Type::Int | Type::Float | Type::String)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(attr.value.range()),
                "list.sort() supports bool, int, float, and str lists for now",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSort {
                list,
                comparator: None,
                key,
                reverse,
            },
            ty,
            span: self.span(call.range),
        })))
    }
}
