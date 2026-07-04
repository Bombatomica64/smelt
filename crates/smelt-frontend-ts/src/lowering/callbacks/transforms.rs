//! Callback value transforms: string/template/typeof/binary/conditional
//! callback expressions and the list search/entries/`at` collection calls.

use crate::lowering::{
    Argument, BinOp, BinaryOperator, BindingPattern, Body, CallbackCallArg, CallbackExpr,
    CallbackExprKind, Expr, ExprKind, Expression, HashMap, ListSearchOp, Literal, ModuleBuilder,
    SmeltError, Type, UnaryOp, UnaryOperator, UnknownKind, unknown_kind_from_typeof,
};
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Lower `value.replace(/.../, (match) => match.toUpperCase())` inside callbacks.
    ///
    /// Date-fns builds locale distance tokens by uppercasing a regex match
    /// within a `reduce` callback. The callback-expression path is still a
    /// compact IR while it is migrated to regular closure bodies, so this
    /// recognizes that public `String.prototype.replace` API shape directly.
    pub(in crate::lowering) fn callback_regex_replace_uppercase_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "replace" {
            return Ok(None);
        }
        let [
            Argument::RegExpLiteral(pattern),
            Argument::ArrowFunctionExpression(replacement),
        ] = call.arguments.as_slice()
        else {
            return Ok(None);
        };
        if !self.arrow_callback_returns_param_uppercase(replacement)? {
            return Ok(None);
        }

        let receiver = self.callback_expression(&member.object, params, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let pattern = CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(Self::regex_literal_pattern_text(
                pattern,
            ))),
            ty: string_ty,
        };
        Ok(Some(CallbackExpr {
            kind: CallbackExprKind::MethodCall {
                receiver: Box::new(receiver),
                method: self.intern_source_name("__smelt_replace_first_match_uppercase"),
                args: vec![CallbackCallArg {
                    expr: pattern,
                    spread: false,
                }],
            },
            ty: string_ty,
        }))
    }

    /// Return whether a replacement callback is `(m) => m.toUpperCase()`.
    pub(in crate::lowering) fn arrow_callback_returns_param_uppercase(
        &self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ) -> Result<bool, SmeltError> {
        if arrow.params.items.len() != 1 || arrow.params.rest.is_some() {
            return Ok(false);
        }
        let Some(param) = arrow.params.items.first() else {
            return Ok(false);
        };
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Ok(false);
        };
        let returned = self.arrow_return_expression(arrow)?;
        let Expression::CallExpression(call) = returned else {
            return Ok(false);
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        let Expression::Identifier(receiver) = &member.object else {
            return Ok(false);
        };
        Ok(receiver.name == binding.name
            && member.property.name == "toUpperCase"
            && call.arguments.is_empty())
    }

    /// Lower a callback template literal as string concatenation.
    pub(in crate::lowering) fn callback_template_literal(
        &mut self,
        template: &oxc::ast::ast::TemplateLiteral<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let Some(first_quasi) = template.quasis.first() else {
            return Err(SmeltError::unsupported(
                self.span(template.span.start, template.span.end),
                "callback template literals must contain at least one quasi",
            ));
        };
        let first_text = first_quasi
            .value
            .cooked
            .as_ref()
            .map_or_else(|| first_quasi.value.raw.as_str(), |cooked| cooked.as_str())
            .to_owned();
        let mut acc = CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(first_text)),
            ty: string_ty,
        };
        for (index, expression) in template.expressions.iter().enumerate() {
            let part = self.callback_expression(expression, params, body)?;
            acc = CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::Add,
                    lhs: Box::new(acc),
                    rhs: Box::new(part),
                },
                ty: string_ty,
            };
            if let Some(quasi) = template.quasis.get(index.saturating_add(1)) {
                let text = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |cooked| cooked.as_str());
                if !text.is_empty() {
                    let literal = CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::String(text.to_owned())),
                        ty: string_ty,
                    };
                    acc = CallbackExpr {
                        kind: CallbackExprKind::Binary {
                            op: BinOp::Add,
                            lhs: Box::new(acc),
                            rhs: Box::new(literal),
                        },
                        ty: string_ty,
                    };
                }
            }
        }
        Ok(acc)
    }

    /// Lower callback `typeof value` expressions to string literals.
    pub(in crate::lowering) fn callback_typeof_unary(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let operand = self.callback_expression(&unary.argument, params, body)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        if self.typeof_type_name(operand.ty).is_none() {
            return Ok(CallbackExpr {
                kind: CallbackExprKind::TypeofValue {
                    value: Box::new(operand),
                },
                ty,
            });
        }
        let kind = self.typeof_type_name(operand.ty).unwrap_or("object");
        Ok(CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(kind.to_owned())),
            ty,
        })
    }

    /// Compute the result type of a callback conditional expression.
    pub(in crate::lowering) fn callback_unify_conditional_exprs(
        &mut self,
        then_expr: CallbackExpr,
        else_expr: CallbackExpr,
        start: u32,
        end: u32,
    ) -> Result<(CallbackExpr, CallbackExpr, smelt_hir::TypeId), SmeltError> {
        if let Ok(ty) = self.callback_conditional_type(then_expr.ty, else_expr.ty, start, end) {
            return Ok((then_expr, else_expr, ty));
        }
        if let Some(coerced_then) =
            self.coerce_callback_object_literal_to_type(then_expr.clone(), else_expr.ty)
        {
            let ty = else_expr.ty;
            return Ok((coerced_then, else_expr, ty));
        }
        if let Some(coerced_else) =
            self.coerce_callback_object_literal_to_type(else_expr.clone(), then_expr.ty)
        {
            let ty = then_expr.ty;
            return Ok((then_expr, coerced_else, ty));
        }
        self.callback_conditional_type(then_expr.ty, else_expr.ty, start, end)
            .map(|ty| (then_expr, else_expr, ty))
    }

    /// Coerce a callback object literal to a structural object type when its fields fit.
    pub(in crate::lowering) fn coerce_callback_object_literal_to_type(
        &mut self,
        mut expr: CallbackExpr,
        target_ty: smelt_hir::TypeId,
    ) -> Option<CallbackExpr> {
        let CallbackExprKind::DictLit(entries) = &expr.kind else {
            return None;
        };
        if !self.callback_object_literal_assignable_to(entries, target_ty) {
            return None;
        }
        expr.ty = target_ty;
        Some(expr)
    }

    /// Return whether callback object-literal entries are compatible with a structural type.
    pub(in crate::lowering) fn callback_object_literal_assignable_to(
        &mut self,
        entries: &[(smelt_hir::Symbol, CallbackExpr)],
        target_ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(target_ty).cloned() {
            Some(Type::Class { .. }) => entries.iter().all(|(field, value)| {
                self.class_field_type(target_ty, *field)
                    .is_ok_and(|field_ty| self.callback_type_assignable(value.ty, field_ty))
            }),
            Some(Type::Dict(_, value_ty)) => entries
                .iter()
                .all(|(_, value)| self.callback_type_assignable(value.ty, value_ty)),
            _ => false,
        }
    }

    /// Lightweight callback assignment compatibility for contextual branch typing.
    pub(in crate::lowering) fn callback_type_assignable(
        &self,
        source_ty: smelt_hir::TypeId,
        target_ty: smelt_hir::TypeId,
    ) -> bool {
        source_ty == target_ty
            || matches!(self.ctx.krate.types.get(source_ty), Some(Type::Unknown))
            || matches!(self.ctx.krate.types.get(target_ty), Some(Type::Unknown))
    }

    /// Compute the result type of a callback conditional expression.
    pub(in crate::lowering) fn callback_conditional_type(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if then_ty == else_ty {
            return Ok(then_ty);
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::Never) {
            return Ok(else_ty);
        }
        if self.ctx.krate.types.get(else_ty) == Some(&Type::Never) {
            return Ok(then_ty);
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(then_ty)
            || self.type_contains_unknown(else_ty)
            || self.erased_or_union_surface(then_ty)
            || self.erased_or_union_surface(else_ty)
        {
            return Ok(self.ctx.krate.types.intern(Type::Unknown));
        }
        if let Some(inner) = self.non_nullish_type(then_ty)
            && inner == else_ty
        {
            return Ok(then_ty);
        }
        if let Some(inner) = self.non_nullish_type(else_ty)
            && inner == then_ty
        {
            return Ok(else_ty);
        }
        if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            return Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)));
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            return Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)));
        }
        Err(SmeltError::unsupported(
            self.span(start, end),
            "callback conditional expression branches must have compatible lowered types",
        ))
    }

    /// Lower `value === undefined/null` checks inside callback expressions.
    pub(in crate::lowering) fn callback_nullish_binary(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::Equality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let Some((value_expr, nullish_expr)) =
            Self::nullish_comparison_parts(&binary.left, &binary.right)
        else {
            return Ok(None);
        };
        let is_undefined_comparison = Self::is_undefined_expression(nullish_expr);
        let is_strict = matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality
        );
        let value = self.callback_expression(value_expr, params, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let is_inequality = matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        );
        if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
            let check = if is_strict {
                CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value),
                        kind: if is_undefined_comparison {
                            UnknownKind::Undefined
                        } else {
                            UnknownKind::Null
                        },
                    },
                    ty: bool_ty,
                }
            } else {
                let null_check = CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value.clone()),
                        kind: UnknownKind::Null,
                    },
                    ty: bool_ty,
                };
                let undefined_check = CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value),
                        kind: UnknownKind::Undefined,
                    },
                    ty: bool_ty,
                };
                CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op: BinOp::Or,
                        lhs: Box::new(null_check),
                        rhs: Box::new(undefined_check),
                    },
                    ty: bool_ty,
                }
            };
            if is_inequality {
                return Ok(Some(CallbackExpr {
                    kind: CallbackExprKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(check),
                    },
                    ty: bool_ty,
                }));
            }
            return Ok(Some(check));
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let op = match binary.operator {
            BinaryOperator::StrictEquality => BinOp::JsStrictEq,
            BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
            BinaryOperator::Equality => BinOp::Eq,
            BinaryOperator::Inequality => BinOp::NotEq,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    "callback nullish comparison operator is not supported",
                ));
            }
        };
        Ok(Some(CallbackExpr {
            kind: CallbackExprKind::Binary {
                op,
                lhs: Box::new(value),
            rhs: Box::new(CallbackExpr {
                    kind: CallbackExprKind::Literal(if is_undefined_comparison {
                        Literal::Undefined
                    } else {
                        Literal::None
                    }),
                    ty: none_ty,
                }),
            },
            ty: bool_ty,
        }))
    }

    /// Lower `typeof value === "kind"` checks inside callback expressions.
    pub(in crate::lowering) fn callback_typeof_binary(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::Equality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return Ok(None);
        };
        if unary.operator != UnaryOperator::Typeof {
            return Ok(None);
        }
        let Expression::StringLiteral(kind_literal) = &binary.right else {
            return Ok(None);
        };
        let Some(kind) = unknown_kind_from_typeof(kind_literal.value.as_str()) else {
            return Ok(None);
        };
        let value = self.callback_expression(&unary.argument, params, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut expr = if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
            CallbackExpr {
                kind: CallbackExprKind::UnknownIs {
                    value: Box::new(value),
                    kind,
                },
                ty: bool_ty,
            }
        } else {
            CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(
                    self.type_matches_typeof(value.ty, kind_literal.value.as_str()),
                )),
                ty: bool_ty,
            }
        };
        if matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            expr = CallbackExpr {
                kind: CallbackExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(expr),
                },
                ty: bool_ty,
            };
        }
        Ok(Some(expr))
    }

    /// Maps supported TypeScript callback binary operators to HIR operators.
    pub(in crate::lowering) fn callback_binary_op(
        &self,
        operator: BinaryOperator,
        start: u32,
        end: u32,
    ) -> Result<BinOp, SmeltError> {
        match operator {
            BinaryOperator::Addition => Ok(BinOp::Add),
            BinaryOperator::Subtraction => Ok(BinOp::Sub),
            BinaryOperator::Multiplication => Ok(BinOp::Mul),
            BinaryOperator::Division => Ok(BinOp::Div),
            BinaryOperator::Remainder => Ok(BinOp::Rem),
            // `===`/`!==` keep JS reference semantics (`JsStrictEq`); `==`/`!=`
            // stay structural (`Eq`). See builder_part08's mapping.
            BinaryOperator::StrictEquality => Ok(BinOp::JsStrictEq),
            BinaryOperator::Equality => Ok(BinOp::Eq),
            BinaryOperator::StrictInequality => Ok(BinOp::JsStrictNotEq),
            BinaryOperator::Inequality => Ok(BinOp::NotEq),
            BinaryOperator::LessThan => Ok(BinOp::Lt),
            BinaryOperator::LessEqualThan => Ok(BinOp::Lte),
            BinaryOperator::GreaterThan => Ok(BinOp::Gt),
            BinaryOperator::GreaterEqualThan => Ok(BinOp::Gte),
            BinaryOperator::ShiftLeft => Ok(BinOp::Shl),
            BinaryOperator::ShiftRight => Ok(BinOp::Shr),
            BinaryOperator::ShiftRightZeroFill => Ok(BinOp::UShr),
            _ => Err(SmeltError::unsupported(
                self.span(start, end),
                "callback binary operator is not supported yet",
            )),
        }
    }

    /// Lower direct TypeScript `Array.prototype.indexOf` and `lastIndexOf`.
    pub(in crate::lowering) fn list_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => ListSearchOp::Find,
            "lastIndexOf" => ListSearchOp::RFind,
            _ => return Ok(None),
        };
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some((list_ty, item_ty)) = self.list_surface_type(list_ty) else {
            return Ok(None);
        };
        if Self::expr_ty(body, list) != list_ty {
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: list },
                ty: list_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        // JavaScript `indexOf`/`lastIndexOf` accept the search item plus an
        // optional `fromIndex`. The item argument is required; the second
        // argument is a numeric start/end position handled by the emitter.
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf require one item and an optional fromIndex argument",
            ));
        }
        let Some(item_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf require one item and an optional fromIndex argument",
            ));
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf argument must match the array element type",
            ));
        }
        let from_index = if let Some(from_index_argument) = call.arguments.get(1) {
            let from_index = self.argument(from_index_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, from_index)) {
                return Err(SmeltError::unsupported(
                    self.span(
                        from_index_argument.span().start,
                        from_index_argument.span().end,
                    ),
                    "array indexOf/lastIndexOf fromIndex must be numeric",
                ));
            }
            Some(from_index)
        } else {
            None
        };
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSearch {
                op,
                list,
                item,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.entries` calls.
    pub(in crate::lowering) fn list_entries_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "entries" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array entries requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let source_ty = Self::expr_ty(body, list);
        if !matches!(
            self.ctx.krate.types.get(source_ty),
            Some(Type::TypeParam { .. })
        ) {
            return Ok(None);
        }
        let list_ty = self.type_param_constraint_or_self(source_ty);
        let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *list_item_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let entry_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Tuple(vec![index_ty, item_ty]));
        let ty = self.ctx.krate.types.intern(Type::List(entry_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListEnumerate { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `.at(index)` on arrays and strings to optional HIR indexing.
    ///
    /// JavaScript `.at` accepts negative indexes, but out-of-range positions
    /// return `undefined` rather than raising. Model that with `OptionalIndex`
    /// so generated Rust uses checked normalized indexes for misses.
    pub(in crate::lowering) fn collection_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "at" {
            return Ok(None);
        }
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at requires exactly one numeric index",
            ));
        };
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let item_ty = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => *item_ty,
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            _ => return Ok(None),
        };
        let index = self.argument(index_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at index must be a number",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Optional(item_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalIndex { receiver, index },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
