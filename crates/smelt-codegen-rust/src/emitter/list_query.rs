//! List query and fold emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list search operation to Rust text.
    /// Converts a list search operation to Rust text.
    pub(super) fn list_search_text(
        &self,
        op: smelt_hir::ListSearchOp,
        list: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list search receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list search item must match the list element type",
            ));
        }
        let method_name = match op {
            smelt_hir::ListSearchOp::Find => "position",
            smelt_hir::ListSearchOp::RFind => "rposition",
        };
        Ok(format!(
            "{}.iter().{method_name}(|item| item == &{}).map_or(-1.0, |idx| idx as f64)",
            self.operand_text(list)?,
            self.operand_text(item)?
        ))
    }

    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    /// Converts a capture-free callback list operation to Rust iterator text.
    pub(super) fn list_callback_text(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        callback: &smelt_hir::CallbackExpr,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list callback receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        let list_text = self.operand_text(list)?;
        let callback_text = Self::callback_expr_text(callback, &["item", "index", "array"])?;
        let closure = format!(
            "|(index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}"
        );
        let ref_closure = format!(
            "|(index, item)| {{ let item = (**item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}"
        );
        match op {
            smelt_hir::ListCallbackOp::Map => {
                if self.mir.types.get(dest_ty) != Some(&Type::List(callback.ty)) {
                    return Err(EmitError::new("array map destination must be a list"));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().map({closure}).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListCallbackOp::Filter => {
                self.validate_bool_callback(callback, "array filter")?;
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array filter destination must match the receiver list type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().filter({ref_closure}).map(|(_, item)| item.clone()).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListCallbackOp::Find => {
                self.validate_bool_callback(callback, "array find")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Optional(element_ty)) {
                    return Err(EmitError::new(
                        "array find destination must be optional element type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().find({ref_closure}).map(|(_, item)| item.clone())"
                ))
            }
            smelt_hir::ListCallbackOp::FindIndex => {
                self.validate_bool_callback(callback, "array findIndex")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Float) {
                    return Err(EmitError::new(
                        "array findIndex destination must be a number",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().position({closure}).map_or(-1.0, |idx| idx as f64)"
                ))
            }
            smelt_hir::ListCallbackOp::Some => {
                self.validate_bool_callback(callback, "array some")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new("array some destination must be boolean"));
                }
                Ok(format!("{list_text}.iter().enumerate().any({closure})"))
            }
            smelt_hir::ListCallbackOp::Every => {
                self.validate_bool_callback(callback, "array every")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new("array every destination must be boolean"));
                }
                Ok(format!("{list_text}.iter().enumerate().all({closure})"))
            }
            smelt_hir::ListCallbackOp::ForEach => {
                if dest_ty != self.none_ty {
                    return Err(EmitError::new("array forEach destination must be none"));
                }
                Ok(format!(
                    "{{ {list_text}.iter().enumerate().for_each(|(index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); let _ = {callback_text}; }}); () }}"
                ))
            }
        }
    }

    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    /// Converts a capture-free array reduce callback into Rust `fold` text.
    pub(super) fn list_reduce_text(
        &self,
        list: &Operand,
        initial: Option<&Operand>,
        callback: &smelt_hir::CallbackExpr,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array reduce receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        if let Some(initial_operand) = initial {
            if self.operand_ty(initial_operand)? != dest_ty {
                return Err(EmitError::new(
                    "array reduce initial value and callback result must match the destination type",
                ));
            }
        }
        if callback.ty != dest_ty {
            return Err(EmitError::new(
                "array reduce initial value and callback result must match the destination type",
            ));
        }
        let list_text = self.operand_text(list)?;
        let callback_text = Self::callback_expr_text(callback, &["acc", "item", "index", "array"])?;
        if let Some(initial_operand) = initial {
            let initial_text = self.operand_text(initial_operand)?;
            Ok(format!(
                "{list_text}.iter().enumerate().fold({initial_text}, |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }})"
            ))
        } else if dest_ty == element_ty {
            Ok(format!(
                "{{ let mut reduce_items = {list_text}.iter().enumerate(); let (_, first) = reduce_items.next().expect(\"reduce of empty array with no initial value\"); reduce_items.fold(first.clone(), |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}) }}"
            ))
        } else {
            Err(EmitError::new(
                "array reduce without an initial value must produce the element type",
            ))
        }
    }

    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    /// Validates that a lowered callback expression returns a boolean.
    pub(super) fn validate_bool_callback(
        &self,
        callback: &smelt_hir::CallbackExpr,
        context: &'static str,
    ) -> Result<(), EmitError> {
        if self.mir.types.get(callback.ty) == Some(&Type::Bool) {
            Ok(())
        } else {
            Err(EmitError::new(format!(
                "{context} callback must return boolean"
            )))
        }
    }

    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    /// Converts a capture-free callback expression tree to Rust source text.
    pub(super) fn callback_expr_text(
        expr: &smelt_hir::CallbackExpr,
        params: &[&str],
    ) -> Result<String, EmitError> {
        match &expr.kind {
            smelt_hir::CallbackExprKind::Param(index) => params
                .get(*index)
                .map(|param| (*param).to_owned())
                .ok_or_else(|| EmitError::new("callback parameter index is out of bounds")),
            smelt_hir::CallbackExprKind::Literal(literal) => Ok(hir_literal_text(literal)),
            smelt_hir::CallbackExprKind::Unary { op, operand } => {
                let op_text = match op {
                    smelt_hir::UnaryOp::Not => "!",
                    smelt_hir::UnaryOp::Neg => "-",
                };
                Ok(format!(
                    "{op_text}({})",
                    Self::callback_expr_text(operand, params)?
                ))
            }
            smelt_hir::CallbackExprKind::Binary { op, lhs, rhs } => Ok(format!(
                "({} {} {})",
                Self::callback_expr_text(lhs, params)?,
                smelt_hir::bin_op_text(*op),
                Self::callback_expr_text(rhs, params)?
            )),
        }
    }

    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    pub(super) fn list_count_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list count receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list count item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            return Err(EmitError::new("list count destination must be int"));
        }
        Ok(format!(
            "{}.iter().filter(|item| *item == &{}).count() as i64",
            self.operand_text(list)?,
            self.operand_text(item)?
        ))
    }

    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    pub(super) fn list_sum_text(&self, list: &Operand, dest_ty: TypeId) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list sum receiver must be a list"));
        };
        if dest_ty != *item_ty {
            return Err(EmitError::new(
                "list sum destination must match the list element type",
            ));
        }
        match self.mir.types.get(*item_ty) {
            Some(Type::Int) => Ok(format!(
                "{}.iter().copied().sum::<i64>()",
                self.operand_text(list)?
            )),
            Some(Type::Float) => Ok(format!(
                "{}.iter().copied().sum::<f64>()",
                self.operand_text(list)?
            )),
            _ => Err(EmitError::new("list sum supports int and float lists")),
        }
    }

    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    pub(super) fn list_bool_fold_text(
        &self,
        op: smelt_hir::BoolFoldOp,
        list: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list boolean fold receiver must be a list"));
        };
        if !matches!(self.mir.types.get(*item_ty), Some(Type::Bool)) {
            return Err(EmitError::new(
                "list boolean fold supports boolean lists only",
            ));
        }
        let method_name = match op {
            smelt_hir::BoolFoldOp::All => "all",
            smelt_hir::BoolFoldOp::Any => "any",
        };
        Ok(format!(
            "{}.iter().copied().{method_name}(|value| value)",
            self.operand_text(list)?
        ))
    }

    // Sorted-list helpers continue in `list_ordering.rs`.
}
