impl ModuleBuilder<'_> {
    /// Lower Python `list.count(item)` calls.
    pub(super) fn list_count_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "count" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.count() requires exactly one item argument",
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
                "list.count() item must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::Int);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListCount { list, item },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.copy()` calls.
    pub(super) fn list_copy_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "copy" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.copy() requires no arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListCopy { list },
            ty: list_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `dict.copy()` calls.
    pub(super) fn dict_copy_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "copy" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "dict.copy() requires no arguments",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        if !matches!(self.ctx.krate.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Ok(None);
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictCopy { dict },
            ty: dict_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `dict.update(other)` calls for same-typed dictionaries.
    pub(super) fn dict_update_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "update" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "dict.update() requires exactly one dict argument",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        if !matches!(self.ctx.krate.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Ok(None);
        }
        let other = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, other) != dict_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "dict.update() argument must match the receiver dict type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictUpdate { dict, other },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `dict.pop(key[, default])` calls.
    pub(super) fn dict_pop_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "pop" {
            return Ok(None);
        }
        if call.arguments.args.is_empty() || call.arguments.args.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "dict.pop() requires a key and optional default",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let key = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "dict.pop() key must match the dict key type",
            ));
        }
        let default = call
            .arguments
            .args
            .get(1)
            .map(|default| self.expression(default, body))
            .transpose()?;
        if let Some(default_expr) = default
            && Self::expr_ty(body, default_expr) != value_ty
        {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "dict.pop() default must match the dict value type",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictPop { dict, key, default },
            ty: value_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `dict.get(key[, default])` calls.
    pub(super) fn dict_get_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "get" {
            return Ok(None);
        }
        if call.arguments.args.is_empty() || call.arguments.args.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "dict.get() requires a key and optional default",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let key = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "dict.get() key must match the dict key type",
            ));
        }
        let default = call
            .arguments
            .args
            .get(1)
            .map(|default| self.expression(default, body))
            .transpose()?;
        let ty = if let Some(default_expr) = default {
            if Self::expr_ty(body, default_expr) != value_ty {
                return Err(SmeltError::unsupported(
                    self.span(call.arguments.args[1].range()),
                    "dict.get() default must match the dict value type",
                ));
            }
            value_ty
        } else {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, value_ty)
        };
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictGet { dict, key, default },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `dict.setdefault(key, default)` calls.
    pub(super) fn dict_setdefault_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "setdefault" {
            return Ok(None);
        }
        if call.arguments.args.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "dict.setdefault() currently requires key and default arguments",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let key = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "dict.setdefault() key must match the dict key type",
            ));
        }
        let default = self.expression(&call.arguments.args[1], body)?;
        if Self::expr_ty(body, default) != value_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "dict.setdefault() default must match the dict value type",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictSetDefault { dict, key, default },
            ty: value_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.clear()`, `dict.clear()`, and `set.clear()` calls.
    pub(super) fn collection_clear_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "clear" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "collection clear requires no arguments",
            ));
        }
        let collection = self.expression(&attr.value, body)?;
        let collection_ty = Self::expr_ty(body, collection);
        let kind = match self.ctx.krate.types.get(collection_ty) {
            Some(Type::List(_)) => ExprKind::ListClear { list: collection },
            Some(Type::Dict(_, _)) => ExprKind::DictClear { dict: collection },
            Some(Type::Set(_)) => ExprKind::SetClear { set: collection },
            _ => return Ok(None),
        };
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind,
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python set mutation methods with direct `HashSet` semantics.
    pub(super) fn set_method_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let method = attr.attr.as_str();
        if !matches!(
            method,
            "add"
                | "discard"
                | "remove"
                | "copy"
                | "union"
                | "intersection"
                | "difference"
                | "symmetric_difference"
                | "isdisjoint"
                | "issubset"
                | "issuperset"
        ) {
            return Ok(None);
        }
        let set = self.expression(&attr.value, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        if method == "copy" {
            if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "set.copy() requires no arguments",
                ));
            }
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::SetCopy { set },
                ty: set_ty,
                span: self.span(call.range),
            })));
        }
        if matches!(
            method,
            "union" | "intersection" | "difference" | "symmetric_difference"
        ) {
            if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "set union/intersection/difference/symmetric_difference require exactly one set argument",
                ));
            }
            let right = self.expression(&call.arguments.args[0], body)?;
            if Self::expr_ty(body, right) != set_ty {
                return Err(SmeltError::unsupported(
                    self.span(call.arguments.args[0].range()),
                    "set algebra argument must match the receiver set type",
                ));
            }
            let op = match method {
                "union" => SetBinaryOp::Union,
                "intersection" => SetBinaryOp::Intersection,
                "difference" => SetBinaryOp::Difference,
                "symmetric_difference" => SetBinaryOp::SymmetricDifference,
                _ => return Ok(None),
            };
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::SetBinary {
                    op,
                    left: set,
                    right,
                },
                ty: set_ty,
                span: self.span(call.range),
            })));
        }
        if method == "isdisjoint" {
            if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "set.isdisjoint() requires exactly one set argument",
                ));
            }
            let right = self.expression(&call.arguments.args[0], body)?;
            if Self::expr_ty(body, right) != set_ty {
                return Err(SmeltError::unsupported(
                    self.span(call.arguments.args[0].range()),
                    "set.isdisjoint() argument must match the receiver set type",
                ));
            }
            let ty = self.intern_type(Type::Bool);
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::SetDisjoint { left: set, right },
                ty,
                span: self.span(call.range),
            })));
        }
        if matches!(method, "issubset" | "issuperset") {
            if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "set.issubset()/issuperset() require exactly one set argument",
                ));
            }
            let right = self.expression(&call.arguments.args[0], body)?;
            if Self::expr_ty(body, right) != set_ty {
                return Err(SmeltError::unsupported(
                    self.span(call.arguments.args[0].range()),
                    "set.issubset()/issuperset() argument must match the receiver set type",
                ));
            }
            let op = match method {
                "issubset" => SetRelationOp::IsSubset,
                "issuperset" => SetRelationOp::IsSuperset,
                _ => return Ok(None),
            };
            let ty = self.intern_type(Type::Bool);
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::SetRelation {
                    op,
                    left: set,
                    right,
                },
                ty,
                span: self.span(call.range),
            })));
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "set add/discard/remove require exactly one item argument",
            ));
        }
        let item = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "set add/discard/remove item must match the set element type",
            ));
        }
        let ty = self.intern_type(Type::None);
        let kind = match method {
            "add" => ExprKind::SetAdd { set, item },
            "discard" => ExprKind::SetRemove {
                op: SetRemoveOp::Discard,
                set,
                item,
            },
            "remove" => ExprKind::SetRemove {
                op: SetRemoveOp::Remove,
                set,
                item,
            },
            _ => return Ok(None),
        };
        Ok(Some(body.push_expr(HirExpr {
            kind,
            ty,
            span: self.span(call.range),
        })))
    }
}
