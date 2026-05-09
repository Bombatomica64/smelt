//! Focused Python standard-library and built-in operation lowering helpers.

use ruff_python_ast::{Expr, ExprSubscript, UnaryOp as RuffUnaryOp};
use ruff_text_size::Ranged;
use smelt_hir::{Body, Expr as HirExpr, ExprKind, Type};

use super::{ModuleBuilder, SmeltError};

impl ModuleBuilder<'_> {
    /// Lower Python `list.pop()` calls without an index argument.
    pub(super) fn list_pop_call_expression(
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
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.pop() index arguments are not supported yet",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let ty = *list_element_ty;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListPop { list },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.reverse()` calls.
    pub(super) fn list_reverse_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "reverse" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.reverse() requires no arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListReverse { list },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `list.append(...)` calls.
    pub(super) fn list_append_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "append" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.append() requires exactly one item argument",
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
                self.span(call.range),
                "list.append() argument must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListPush { list, item },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python list and string slicing with omitted or positive bounds.
    pub(super) fn slice_subscript(
        &mut self,
        sub: &ExprSubscript,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Slice(slice) = sub.slice.as_ref() else {
            return Ok(None);
        };
        if slice.step.is_some() {
            return Err(SmeltError::unsupported(
                self.span(slice.range),
                "slice steps are not supported yet",
            ));
        }
        for bound in [slice.lower.as_deref(), slice.upper.as_deref()]
            .into_iter()
            .flatten()
        {
            if is_negative_numeric_literal(bound) {
                return Err(SmeltError::unsupported(
                    self.span(bound.range()),
                    "slice negative indexes are not supported yet",
                ));
            }
        }

        let receiver = self.expression(&sub.value, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let lower = slice
            .lower
            .as_deref()
            .map(|expr| self.slice_bound(expr, body))
            .transpose()?;
        let upper = slice
            .upper
            .as_deref()
            .map(|expr| self.slice_bound(expr, body))
            .transpose()?;

        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(_)) => Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::ListSlice {
                    list: receiver,
                    start: lower,
                    end: upper,
                },
                ty: receiver_ty,
                span: self.span(sub.range),
            }))),
            Some(Type::String) => {
                let ty = self.intern_type(Type::String);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::StringSlice {
                        operand: receiver,
                        start: lower,
                        end: upper,
                    },
                    ty,
                    span: self.span(sub.range),
                })))
            }
            _ => Err(SmeltError::unsupported(
                self.span(sub.range),
                "slicing requires a list or string receiver",
            )),
        }
    }

    /// Lower and validate a Python slice bound.
    fn slice_bound(
        &mut self,
        expr: &Expr,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bound = self.expression(expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, bound)) != Some(&Type::Int) {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                "slice bounds must be integers",
            ));
        }
        Ok(bound)
    }
}

/// Return true for syntactic negative numeric literal bounds.
fn is_negative_numeric_literal(expr: &Expr) -> bool {
    let Expr::UnaryOp(unary) = expr else {
        return false;
    };
    unary.op == RuffUnaryOp::USub && matches!(unary.operand.as_ref(), Expr::NumberLiteral(_))
}
