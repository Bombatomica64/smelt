//! Focused Python standard-library and built-in operation lowering helpers.

use ruff_python_ast::{Expr, ExprSubscript};
use ruff_text_size::Ranged;
use smelt_hir::{
    Body, BoolFoldOp, Expr as HirExpr, ExprKind, FileId, RegexMatchOp, SetBinaryOp, SetRemoveOp,
    Span, Type,
};

use super::{ModuleBuilder, SmeltError};

impl ModuleBuilder<'_> {
    /// Lower Python `json.dumps(value)` calls for JSON-compatible values.
    pub(super) fn json_dumps_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "json" || attr.attr.as_str() != "dumps" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "json.dumps() currently supports exactly one value argument",
            ));
        }
        let value = self.expression(&call.arguments.args[0], body)?;
        if !self.is_json_serializable_type(Self::expr_ty(body, value)) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "json.dumps() value must be JSON-serializable",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::JsonStringify { value },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `json.loads(text)` calls with an annotated destination type.
    pub(super) fn json_loads_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "json" || attr.attr.as_str() != "loads" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "json.loads() currently supports exactly one text argument",
            ));
        }
        let Some(ty) = type_hint else {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "json.loads() requires an annotated destination type",
            ));
        };
        if !self.is_json_serializable_type(ty) {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "json.loads() destination type must be JSON-compatible",
            ));
        }
        let text = self.expression(&call.arguments.args[0], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, text)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "json.loads() text argument must be a string",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::JsonParse { text },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Lower Python `re.search`, `re.match`, and `re.fullmatch` calls to boolean regex matches.
    pub(super) fn re_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "re" {
            return Ok(None);
        }
        let op = match attr.attr.as_str() {
            "search" => RegexMatchOp::Search,
            "match" => RegexMatchOp::Match,
            "fullmatch" => RegexMatchOp::FullMatch,
            _ => return Ok(None),
        };
        if call.arguments.args.len() != 2 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "re.search/match/fullmatch currently require pattern and text arguments only",
            ));
        }
        let pattern = self.expression(&call.arguments.args[0], body)?;
        let haystack = self.expression(&call.arguments.args[1], body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "re.search/match/fullmatch require string pattern and text arguments",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::RegexIsMatch {
                op,
                pattern,
                haystack,
            },
            ty,
            span: self.span(call.range),
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

    /// Lower Python `sorted(values)` calls for directly sortable lists.
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
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "sorted() currently supports exactly one list argument and no keywords",
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
        if !matches!(
            self.ctx.krate.types.get(*item_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sorted() argument must be a sortable list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSorted { list },
            ty: list_ty,
            span: self.span(call.range),
        })))
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

    /// Lower Python `list.sort()` calls without key or reverse arguments.
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
        if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.sort() currently supports no arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        if !matches!(
            self.ctx.krate.types.get(*list_element_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                self.span(attr.value.range()),
                "list.sort() supports bool, int, float, and str lists for now",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSort { list },
            ty,
            span: self.span(call.range),
        })))
    }

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
            self.intern_type(Type::Optional(value_ty))
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
            "add" | "discard" | "remove" | "copy" | "union" | "intersection" | "difference"
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
        if matches!(method, "union" | "intersection" | "difference") {
            if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "set union/intersection/difference require exactly one set argument",
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
                "set method item must match the set element type",
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
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "list.pop() index arguments are not supported yet",
            ));
        }
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

    /// Lower Python list and string slicing with Python-style clamped bounds.
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
            Some(Type::Tuple(items)) => {
                let item_tys = items.clone();
                let raw_start = match slice.lower.as_deref() {
                    Some(expr) => Self::static_int_literal(expr)?.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(expr.range()),
                            "tuple slice bounds must be static integer literals",
                        )
                    })?,
                    None => 0,
                };
                let raw_end = match slice.upper.as_deref() {
                    Some(expr) => Self::static_int_literal(expr)?.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(expr.range()),
                            "tuple slice bounds must be static integer literals",
                        )
                    })?,
                    None => i64::try_from(item_tys.len()).map_err(|_err| {
                        SmeltError::unsupported(self.span(sub.range), "tuple length is too large")
                    })?,
                };
                let (normalized_start, normalized_end) =
                    Self::normalize_tuple_slice(raw_start, raw_end, item_tys.len())?;
                let ty = self.intern_type(Type::Tuple(
                    item_tys[normalized_start..normalized_end].to_vec(),
                ));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::TupleSlice {
                        tuple: receiver,
                        start: normalized_start,
                        end: normalized_end,
                    },
                    ty,
                    span: self.span(sub.range),
                })))
            }
            _ => Err(SmeltError::unsupported(
                self.span(sub.range),
                "slicing requires a list, string, or tuple receiver",
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

    /// Normalize Python tuple slice bounds for statically-known tuple lengths.
    fn normalize_tuple_slice(
        start: i64,
        end: i64,
        len: usize,
    ) -> Result<(usize, usize), SmeltError> {
        let span = Span::new(FileId(0), 0, 0);
        let len_i64 = i64::try_from(len)
            .map_err(|_err| SmeltError::unsupported(span, "tuple length is too large"))?;
        let normalize = |index: i64| -> Result<i64, SmeltError> {
            if index < 0 {
                let shifted = len_i64
                    .checked_add(index)
                    .ok_or_else(|| SmeltError::unsupported(span, "tuple slice bound is invalid"))?;
                Ok(shifted.clamp(0, len_i64))
            } else {
                Ok(index.clamp(0, len_i64))
            }
        };
        let normalized_start = normalize(start)?;
        let normalized_end = normalize(end)?;
        let start_index = usize::try_from(normalized_start)
            .map_err(|_err| SmeltError::unsupported(span, "tuple slice start is invalid"))?;
        let end_index = usize::try_from(normalized_end)
            .map_err(|_err| SmeltError::unsupported(span, "tuple slice end is invalid"))?;
        Ok((start_index, end_index.max(start_index)))
    }
}
