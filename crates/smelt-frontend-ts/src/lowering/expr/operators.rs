//! Operator expressions: binary, unary, `in`, `delete` and `void`.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::js_text::js_str_text;
use smelt_hir::{ErasureReason, PropertyLookup};
use crate::lowering::{
    BinOp, BinaryOperator, Body, Expr, ExprKind, Expression, Item, Literal, ModuleBuilder,
    PrimitiveCastOp, SmeltError, Span, Stmt, Type, UnaryOp, UnaryOperator,
};
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Lower a binary expression.
    pub(in crate::lowering) fn binary_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `instanceof` and `in` are not arithmetic/comparison operators that map
        // onto a `BinOp`; they have dedicated lowering (predicate and key-membership
        // respectively).
        if binary.operator == BinaryOperator::Instanceof {
            return self.instanceof_expression(binary, body);
        }
        if binary.operator == BinaryOperator::In {
            return self.in_expression(binary, body);
        }
        if binary.operator == BinaryOperator::Exponential {
            let base = self.expression(&binary.left, body)?;
            let exponent = self.expression(&binary.right, body)?;
            let span = self.span(binary.span.start, binary.span.end);
            if let Some(power) = self.bigint_power(base, exponent, span, body) {
                return Ok(power);
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::NumericPow { base, exponent },
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let op = match binary.operator {
            BinaryOperator::Addition => BinOp::Add,
            BinaryOperator::Subtraction => BinOp::Sub,
            BinaryOperator::Multiplication => BinOp::Mul,
            BinaryOperator::Division => BinOp::Div,
            BinaryOperator::Remainder => BinOp::Rem,
            // `===`/`!==` keep JS reference semantics (`JsStrictEq`); `==`/`!=`
            // stay structural (`Eq`).
            BinaryOperator::StrictEquality => BinOp::JsStrictEq,
            BinaryOperator::Equality => BinOp::Eq,
            BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
            BinaryOperator::Inequality => BinOp::NotEq,
            BinaryOperator::LessThan => BinOp::Lt,
            BinaryOperator::LessEqualThan => BinOp::Lte,
            BinaryOperator::GreaterThan => BinOp::Gt,
            BinaryOperator::GreaterEqualThan => BinOp::Gte,
            BinaryOperator::ShiftLeft => BinOp::Shl,
            BinaryOperator::ShiftRight => BinOp::Shr,
            BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
            BinaryOperator::BitwiseAnd => BinOp::BitAnd,
            BinaryOperator::BitwiseOR => BinOp::BitOr,
            BinaryOperator::BitwiseXOR => BinOp::BitXor,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    format!("binary operator is not lowered yet: {:?}", binary.operator),
                ));
            }
        };
        let mut lhs = self.expression(&binary.left, body)?;
        let mut rhs = self.expression(&binary.right, body)?;
        // JavaScript `===`/`!==` on erased values compares the ORIGINAL erased
        // `SmeltUnknown` (reference identity for objects/arrays/functions, value
        // for primitives). A `typeof`-guard narrows an operand to a concrete
        // type (e.g. `Function`), which lowers the reference as an
        // `UnknownCast` that re-materializes the value through an adapter — for
        // a function that means a FRESH `Rc` whose `Rc::ptr_eq` never matches,
        // so `f === f` wrongly yields `false`. The narrowing carries no benefit
        // for a bare strict-equality operand, so peel it back to the pre-cast
        // erased value and let both sides compare as the untouched originals.
        if matches!(op, BinOp::JsStrictEq | BinOp::JsStrictNotEq) {
            lhs = self.peel_narrowing_cast_for_identity(body, lhs);
            rhs = self.peel_narrowing_cast_for_identity(body, rhs);
        }
        (lhs, rhs) = self.null_absent_concat_operands(op, lhs, rhs, binary, body);

        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        // Erased operands say nothing about `+`; the checker may know it concatenates.
        let ty = if op == BinOp::Add && self.checker_types_as_string(binary.span.start, binary.span.end) {
            self.ctx.krate.types.intern(Type::String)
        } else {
            self.binary_result_type(op, lhs_ty, rhs_ty)
        };
        let span = self.span(binary.span.start, binary.span.end);
        if let Some(call) = self.bigint_fallible_binary(op, lhs, rhs, span, body) {
            return Ok(call);
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Peel a narrowing [`ExprKind::UnknownCast`] off a strict-equality operand.
    ///
    /// A `typeof`/`instanceof` guard narrows an erased local to a concrete type
    /// via an `UnknownCast` whose inner value is still an erased `SmeltUnknown`.
    /// For a bare `===`/`!==` operand the narrowed concrete type is discarded
    /// anyway (the comparison lowers through `js_strict_eq` on the erased
    /// representation), and materializing the concrete shape can re-wrap the
    /// value through an adapter that breaks JS reference identity (a function
    /// becomes a fresh `Rc`, so `f === f` is `false`). Returning the pre-cast
    /// erased value keeps the comparison on the untouched original. Only peels
    /// when the inner value is genuinely erased, so ordinary casts are left
    /// intact.
    pub(in crate::lowering) fn peel_narrowing_cast_for_identity(
        &self,
        body: &mut Body,
        expr: smelt_hir::ExprId,
    ) -> smelt_hir::ExprId {
        let Some(Expr {
            kind: ExprKind::UnknownCast { value, .. },
            span,
            ..
        }) = body.expr_at(expr)
        else {
            return expr;
        };
        let inner = *value;
        let cast_span = *span;
        // The narrowing cast rewraps a `Local` read at the narrowed type; recover
        // the local and re-read it at its erased base type so the comparison sees
        // the untouched original `SmeltUnknown`.
        let Some(Expr {
            kind: ExprKind::Local(local),
            ..
        }) = body.expr_at(inner)
        else {
            return expr;
        };
        let source_local = *local;
        let base_ty = Self::local_ty(body, source_local);
        if matches!(
            self.ctx.krate.types.get(base_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            return body.push_expr(Expr {
                kind: ExprKind::Local(source_local),
                ty: base_ty,
                span: cast_span,
            });
        }
        expr
    }

    /// Lower a TypeScript non-null assertion while preserving the narrowed type.
    pub(in crate::lowering) fn non_null_assertion_expression(
        &mut self,
        expression: &Expression<'_>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        Ok(self.non_null_assertion_value(value, span, body))
    }

    /// Apply non-null assertion narrowing to an already-lowered expression.
    pub(in crate::lowering) fn non_null_assertion_value(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        let Some(non_null_ty) = self.non_nullish_type(value_ty) else {
            return value;
        };
        if non_null_ty == value_ty {
            return value;
        }
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: non_null_ty,
            span,
        })
    }

    /// Fold a `"<key>" in <global-alias>` feature probe to a literal.
    ///
    /// The receiver must be a recognized global alias (bare `globalThis` /
    /// `global` / `self`, or a local known to alias the global object) and the key
    /// must be a string literal — a dynamic key is on the erasure denylist and
    /// stays a runtime check. The presence answer is derived from the
    /// recognition registries via [`smelt_stdlib::global_member_presence`], so an
    /// unmodeled key (`Unknown`) is *not* folded: it returns `None` and falls
    /// through to ordinary lowering instead of guessing.
    pub(in crate::lowering) fn global_contains_key_probe(
        &mut self,
        key: &Expression<'_>,
        receiver: &Expression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !self.expr_is_global_alias(receiver) {
            return None;
        }
        let Expression::StringLiteral(key_lit) = key else {
            return None;
        };
        let presence = smelt_stdlib::global_member_presence(&js_str_text(key_lit.value));
        let value = match presence {
            smelt_stdlib::GlobalPresence::Present => true,
            smelt_stdlib::GlobalPresence::Absent => false,
            // `Unknown` (and any future undecided presence) must not fold.
            _ => return None,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty: bool_ty,
            span: self.span(key.span().start, receiver.span().end.max(key.span().end)),
        }))
    }

    /// Lower JavaScript `key in object` checks for dictionaries and static objects.
    ///
    /// Static object constants are often erased to reusable metadata before a
    /// function body is lowered. For those, membership is a pure key-set test,
    /// so emitting string equality checks keeps the generated Rust independent
    /// from a runtime object allocation.
    /// Lower `void operand`: evaluate the operand for its effects, then yield
    /// `undefined`.
    ///
    /// The operand is not optional in JavaScript: `void handle(req, res)` is
    /// the idiom for starting a promise and deliberately discarding it, and
    /// `void` is only about the RESULT. Answering the literal without lowering
    /// the operand dropped the call, so a `node:http` handler written that
    /// way never answered a single request. The operand runs as an expression
    /// statement of the block the `void` is lowered into; `result` is the
    /// literal the caller's context spells `undefined` with.
    pub(in crate::lowering) fn void_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
        result: Literal,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand = self.expression(&unary.argument, body)?;
        let effect = Stmt::Expr(operand);
        self.push_stmt_to_sink(body, effect);
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(result),
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    pub(in crate::lowering) fn in_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(binary.span.start, binary.span.end);
        self.key_in_expression(&binary.left, &binary.right, span, body)
    }

    /// Lower `key in receiver`: whether `receiver` (or its prototype chain) has
    /// the property `key`. Shared by the `in` operator and `Reflect.has`.
    pub(in crate::lowering) fn key_in_expression(
        &mut self,
        key_expr: &Expression<'_>,
        receiver_expr: &Expression<'_>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);

        if let Some(expr) = self.global_contains_key_probe(key_expr, receiver_expr, body) {
            return Ok(expr);
        }

        // A `<key> in <global-alias>` membership test that the registry-derived
        // probe above could not fold (an unknown/undecided member, or a
        // non-literal key) must stay an honest blocker. The global object now
        // resolves to a marker host-object value (see
        // `global_object_value_expression`), so without this guard the test
        // would silently evaluate against the empty marker record and answer
        // `false` for members the real global actually has. Presence of the
        // global object as a value does not make its full key set known.
        if self.expr_is_global_alias(receiver_expr) {
            return Err(SmeltError::unsupported(
                span,
                "`in` on the global object is only lowered for registry-decidable string-literal keys",
            ));
        }

        if let Expression::Identifier(receiver_ident) = &receiver_expr
            && let Some(object_const) = self.consts.object(receiver_ident.name.as_str())
                .cloned()
        {
            let mut key = self.expression(key_expr, body)?;
            if Self::expr_ty(body, key) != string_ty {
                key = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: key },
                    ty: string_ty,
                    span: self.span(key_expr.span().start, key_expr.span().end),
                });
            }
            let mut condition = None;
            for entry in object_const.entries {
                let rhs = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(entry.key)),
                    ty: string_ty,
                    span,
                });
                let equals_key = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Eq,
                        lhs: key,
                        rhs,
                    },
                    ty: bool_ty,
                    span,
                });
                condition = Some(condition.map_or(equals_key, |previous| {
                    body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Or,
                            lhs: previous,
                            rhs: equals_key,
                        },
                        ty: bool_ty,
                        span,
                    })
                }));
            }
            return Ok(condition.unwrap_or_else(|| {
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty: bool_ty,
                    span,
                })
            }));
        }

        let receiver = self.expression(receiver_expr, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let mut key = self.expression(key_expr, body)?;
        // An iterator result's own properties are fixed by the protocol, so a
        // literal key is answered without erasing the typed result.
        if matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::GeneratorResult { .. }))
            && let Expression::StringLiteral(name) = &key_expr
        {
            let present = name.value == "done" || name.value == "value";
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(present)),
                ty: bool_ty,
                span,
            }));
        }
        // `"field" in unionLocal` over a union of concrete member shapes can be
        // answered by a static discriminant test in codegen rather than erasing
        // the value into a runtime object map. Keeping the receiver at its union
        // type lets `dict_contains_key_text` emit `matches!(x, Union::Mi(_) ..)`
        // for the arms that carry `field`. This only fires for a string-literal
        // key; dynamic keys stay on the erased path below. Nullish/dynamic
        // boundaries are untouched because concrete-union eligibility (checked in
        // codegen) excludes `Optional`/`unknown` members.
        //
        // A single CLASS receiver is the one-arm case of the same rule: its
        // declared instance surface (fields, methods, accessors, inherited
        // members, or a host class's spec surface) answers the test
        // statically. Erasing it first lost everything but the data fields,
        // so `'method' in instance` answered `false`.
        if matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Union(_) | Type::Class { .. })
        ) && matches!(&key_expr, Expression::StringLiteral(_))
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DictContainsKey {
                    dict: receiver,
                    key,
                    lookup: PropertyLookup::PrototypeChain,
                },
                ty: bool_ty,
                span,
            }));
        }
        let Some(Type::Dict(receiver_key_ty, _)) = self.ctx.krate.types.get(receiver_ty) else {
            if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown)
                || self.erased_or_union_surface(receiver_ty)
                || matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    // A union of concrete members (`string | string[]` after a
                    // typeof guard Smelt's erased locals do not re-type) is a
                    // dynamic surface for `in` just like an erased union.
                    Some(
                        Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::List(_)
                            | Type::Tuple(_)
                            | Type::String
                            | Type::Union(_)
                            | Type::GeneratorResult { .. }
                    )
                )
            {
                let receiver = if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown) {
                    receiver
                } else {
                    let ty = self.erase_with(ErasureReason::DynamicOperator, "in");
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: receiver },
                        ty,
                        span,
                    })
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey {
                        dict: receiver,
                        key,
                        lookup: PropertyLookup::PrototypeChain,
                    },
                    ty: bool_ty,
                    span,
                }));
            }
            return Err(SmeltError::unsupported(
                span,
                "`in` checks require a static object, record, map, or unknown receiver",
            ));
        };
        let key_ty = *receiver_key_ty;
        if Self::expr_ty(body, key) != key_ty
            && self.is_string_compatible_type(Self::expr_ty(body, key))
        {
            key = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: key },
                ty: key_ty,
                span,
            });
        }
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                span,
                "`in` check key must match the record or map key type",
            ));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: receiver,
                key,
                lookup: PropertyLookup::PrototypeChain,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Lower a unary expression.
    pub(in crate::lowering) fn unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if unary.operator == UnaryOperator::Delete {
            return self.delete_unary_expression(unary, body);
        }
        if unary.operator == UnaryOperator::Typeof {
            return self.typeof_expression(unary, body);
        }
        if unary.operator == UnaryOperator::Void {
            return self.void_expression(unary, body, Literal::Undefined);
        }
        let op = match unary.operator {
            UnaryOperator::LogicalNot => UnaryOp::Not,
            UnaryOperator::UnaryNegation => UnaryOp::Neg,
            UnaryOperator::UnaryPlus => {
                let operand = self.expression(&unary.argument, body)?;
                let operand_ty = Self::expr_ty(body, operand);
                if self.is_numeric_like_type(operand_ty) {
                    return Ok(operand);
                }
                if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                    || self.is_date_constructor_arg_type(operand_ty)
                {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::PrimitiveCast {
                            op: PrimitiveCastOp::ToJsNumber,
                            operand,
                        },
                        ty,
                        span: self.span(unary.span.start, unary.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    "unary plus requires a numeric or DateArg-compatible operand",
                ));
            }
            UnaryOperator::BitwiseNot => return self.bitwise_not_expression(unary, body),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    format!("unary operator is not lowered yet: {:?}", unary.operator),
                ));
            }
        };
        // `!(a && b)` observes only the TRUTHINESS of the logical operand,
        // which distributes over `&&`/`||`: lower it as a condition (a
        // boolean) rather than as the operand-selecting value, whose type is
        // the union of both operands and erases when they differ.
        if matches!(op, UnaryOp::Not)
            && matches!(
                peel(&unary.argument, Wrappers::Parens),
                Expression::LogicalExpression(_)
            )
        {
            let operand = self.condition_expression(&unary.argument, body)?;
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnaryOp { op, operand },
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        let operand = self.expression(&unary.argument, body)?;
        let operand = if matches!(op, UnaryOp::Not) {
            let span = self.span(unary.argument.span().start, unary.argument.span().end);
            self.optional_known_date_presence_condition(operand, span, body)
                .or_else(|| self.keyed_read_presence_condition(operand, span, body))
                .unwrap_or(operand)
        } else {
            operand
        };
        let ty = match op {
            UnaryOp::Not => self.ctx.krate.types.intern(Type::Bool),
            UnaryOp::Neg => Self::expr_ty(body, operand),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower JavaScript `delete object[key]` to a dictionary key removal.
    pub(in crate::lowering) fn delete_unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (object, key) = match &unary.argument {
            Expression::ComputedMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let key = self.expression(&member.expression, body)?;
                (object, key)
            }
            Expression::StaticMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(member.property.name.to_string())),
                    ty: string_ty,
                    span: self.span(member.property.span.start, member.property.span.end),
                });
                (object, key)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.argument.span().start, unary.argument.span().end),
                    "delete is only lowered for object keys",
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictRemoveKey { dict: object, key },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower a static member access expression.
    pub(in crate::lowering) fn namespace_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((namespace, member_name)) = self.namespace_member_name(member) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        if let Some(value) = self.consts.literal(member_name).cloned() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(value.literal),
                ty: value.ty,
                span,
            })));
        }
        if let Some(value) = self.consts.object(member_name).cloned() {
            return Ok(Some(self.object_const_expression(
                &value,
                member.span.start,
                member.span.end,
                body,
            )));
        }
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            if self.imports.is_namespace(namespace) {
                let ty = self.erase_with(ErasureReason::UntypedPackage, "namespace member");
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span,
                })));
            }
            return Err(SmeltError::unsupported(
                span,
                format!("namespace import has no exported member `{member_name}`"),
            ));
        };
        match self.item_ref(item).clone() {
            Item::Function(_) => Ok(Some(self.item_function_closure_expression(
                item,
                member.span.start,
                member.span.end,
                body,
            )?)),
            Item::Const(const_item) => Ok(Some(self.const_item_expression(
                &const_item,
                member.span.start,
                member.span.end,
                body,
            )?)),
            _ => {
                let ty = self.item_expr_type(item, span)?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Item(item),
                    ty,
                    span,
                })))
            }
        }
    }
}
