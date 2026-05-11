//! Focused TypeScript standard-library lowering helpers.

use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, RegexMatchOp, Type};
use smelt_stdlib::RuleId;

use super::{stdlib_dispatch, ModuleBuilder, SmeltError};

impl ModuleBuilder<'_> {
    /// Lower TypeScript `JSON.stringify(value)` calls for JSON-compatible values.
    pub(super) fn json_stringify_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsJsonStringify) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "JSON" || member.property.name != "stringify" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.stringify() currently supports exactly one value argument",
            ));
        };
        let value = self.argument(argument, body)?;
        if !self.is_json_serializable_type(Self::expr_ty(body, value)) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "JSON.stringify() value must be JSON-serializable",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonStringify { value },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `JSON.parse<T>(text)` calls for JSON-compatible targets.
    pub(super) fn json_parse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsJsonParse) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "JSON" || member.property.name != "parse" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse<T>() currently supports exactly one text argument",
            ));
        };
        let Some(type_args) = &call.type_arguments else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse requires an explicit type argument",
            ));
        };
        let [target_ty] = type_args.params.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse requires exactly one type argument",
            ));
        };
        let ty = self.ts_type_to_hir(target_ty)?;
        if !self.is_json_serializable_type(ty) {
            return Err(SmeltError::unsupported(
                self.span(target_ty.span().start, target_ty.span().end),
                "JSON.parse<T>() target type must be JSON-compatible",
            ));
        }
        let text = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, text)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "JSON.parse<T>() text argument must be a string",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse { text },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `new RegExp(pattern).test(text)` to a regex boolean match.
    pub(super) fn regexp_test_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsRegExpTest) {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "test" {
            return Ok(None);
        }
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "RegExp" {
            return Ok(None);
        }
        let [pattern_argument] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new RegExp() currently requires exactly one string pattern argument",
            ));
        };
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires exactly one string argument",
            ));
        };
        let pattern = self.argument(pattern_argument, body)?;
        let haystack = self.argument(haystack_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires string pattern and haystack",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexIsMatch {
                op: RegexMatchOp::Search,
                pattern,
                haystack,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a HIR type can be serialized by the JSON mapping.
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

    /// Lower direct TypeScript `Array.prototype.shift` calls.
    pub(super) fn list_shift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "shift" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array shift requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let ty = self.ctx.krate.types.intern(Type::Optional(element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListShift { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.pop` calls.
    pub(super) fn list_pop_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "pop" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array pop requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let ty = self.ctx.krate.types.intern(Type::Optional(*element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListPop { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.reverse` calls.
    pub(super) fn list_reverse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reverse" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reverse requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReverse { list },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.sort` calls with an optional comparator.
    pub(super) fn list_sort_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "sort" {
            return Ok(None);
        }
        let comparator_argument = match call.arguments.as_slice() {
            [] => None,
            [argument] => Some(argument),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array sort requires at most one comparator argument",
                ));
            }
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let comparator = if let Some(argument) = comparator_argument {
            let callback =
                self.capture_free_arrow_callback(argument, &[element_ty, element_ty])?;
            let number_ty = self.ctx.krate.types.intern(Type::Float);
            self.require_callback_ty(callback.ty, number_ty, call, "array sort")?;
            Some(callback)
        } else {
            None
        };
        if comparator.is_some() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSort { list, comparator },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            self.ctx.krate.types.get(element_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array sort supports boolean, number, and string arrays for now",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSort { list, comparator },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.push` calls.
    pub(super) fn list_push_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "push" {
            return Ok(None);
        }
        let Expression::Identifier(_) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array push currently requires a local array receiver",
            ));
        };
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push currently supports exactly one item argument",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.argument(item_argument, body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListPush { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.unshift` calls.
    pub(super) fn list_unshift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "unshift" {
            return Ok(None);
        }
        let Expression::Identifier(_) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array unshift currently requires a local array receiver",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let mut items = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let item = self.argument(argument, body)?;
            if Self::expr_ty(body, item) != element_ty {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "array unshift arguments must match the array element type",
                ));
            }
            items.push(item);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListUnshift { list, items },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.slice`, `String.prototype.slice`, and
    /// positive-bound `String.prototype.substring` calls.
    pub(super) fn collection_slice_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "slice" | "substring") {
            return Ok(None);
        }
        if call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring currently support only omitted, start, and end arguments",
            ));
        }
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if method == "substring" && self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Ok(None);
        }
        let start = call
            .arguments
            .first()
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let end = call
            .arguments
            .get(1)
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;

        match self.ctx.krate.types.get(operand_ty) {
            Some(Type::String) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringSlice {
                        operand,
                        start,
                        end,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            Some(Type::List(_)) if method == "slice" => Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSlice {
                    list: operand,
                    start,
                    end,
                },
                ty: operand_ty,
                span: self.span(call.span.start, call.span.end),
            }))),
            _ => Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring requires a string receiver, or an array receiver for slice",
            )),
        }
    }

    /// Lower and validate a slice index argument.
    fn slice_index_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "slice indexes must be numbers",
            ));
        }
        Ok(index)
    }
}
