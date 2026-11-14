//! `Number` statics and methods, and `parseInt`/`parseFloat`.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{
    Body, Expr, ExprKind, Literal, NumericFormatMethod, NumericPredicateOp, PrimitiveCastOp, Type,
};

impl ModuleBuilder<'_> {
    /// Lower direct TypeScript numeric predicate calls.
    pub(in crate::lowering) fn number_predicate_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // `coerces`: the global `isNaN` converts its argument to a number; the
        // `Number.*` predicates never do.
        let (op, source_name, coerces) = match &call.callee {
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return Ok(None);
                };
                if !self.names_host(object.name.as_str(), "Number") {
                    return Ok(None);
                }
                let op = match member.property.name.as_str() {
                    "isFinite" => NumericPredicateOp::IsFinite,
                    "isInteger" => NumericPredicateOp::IsInteger,
                    "isSafeInteger" => NumericPredicateOp::IsSafeInteger,
                    "isNaN" => NumericPredicateOp::IsNaN,
                    _ => return Ok(None),
                };
                (op, format!("Number.{}", member.property.name), false)
            }
            Expression::Identifier(identifier)
                if self.names_host(identifier.name.as_str(), "isNaN") =>
            {
                // A value import, module item, or local binding of `isNaN`
                // shadows the global predicate (es-toolkit's own
                // `import { isNaN } from './isNaN'` accepts `any`). Defer to the
                // ordinary call path so the shadowing binding is called instead
                // of forcing the numeric-global predicate onto its argument.
                if self.builtin_call_identifier_is_shadowed(identifier.name.as_str()) {
                    return Ok(None);
                }
                (NumericPredicateOp::IsNaN, "isNaN".to_owned(), true)
            }
            _ => return Ok(None),
        };
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires exactly one number argument"),
            ));
        };
        let mut operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        // `Number.isSafeInteger` does not coerce: ECMAScript answers `false` for
        // every non-Number argument. Asserting an erased operand to `number`
        // here would make `Number.isSafeInteger('1')` answer `true`, so the
        // erased operand is carried through unchanged and the codegen tests its
        // runtime tag instead.
        // The `Number.*` predicates never coerce either: an absent value is
        // not a number, so `Number.isNaN(x)` over `x: number | undefined` is
        // `false` when `x` is absent (remeda `when((x) => Number.isNaN(x) ||
        // x === undefined, ..)` asserted presence and panicked). An optional
        // operand is carried through and tested where present.
        let non_coercing_optional = !coerces
            && matches!(
                self.ctx.krate.types.get(operand_ty),
                Some(Type::Optional(inner))
                    if matches!(self.ctx.krate.types.get(*inner), Some(Type::Int | Type::Float))
            );
        if non_coercing_optional
            || matches!(op, NumericPredicateOp::IsSafeInteger)
                && matches!(
                    self.ctx.krate.types.get(operand_ty),
                    Some(
                        Type::Unknown
                            | Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::Optional(_)
                            | Type::Union(_)
                    )
                )
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::NumericPredicate { op, operand },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::Int | Type::Float)
        ) {
            if ((source_name == "isNaN" || source_name == "Number.isNaN")
                && self.is_date_constructor_arg_type(operand_ty))
                || matches!(
                    self.ctx.krate.types.get(operand_ty),
                    Some(
                        Type::Unknown
                            | Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::Optional(_)
                            | Type::Union(_)
                    )
                )
            {
                let ty = self.ctx.krate.types.intern(Type::Float);
                operand = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty,
                    span: self.span(argument.span().start, argument.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a number argument"),
                ));
            }
        }
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires a number argument"),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPredicate { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Number.parseFloat(...)` calls.
    pub(in crate::lowering) fn number_parse_float_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Number") || member.property.name != "parseFloat"
        {
            return Ok(None);
        }
        let operand = self.parse_float_operand("Number.parseFloat", call, body)?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ParseFloat,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower one JavaScript `parseFloat` operand through its `ToString` step.
    ///
    /// ECMAScript parses the string representation of its input rather than
    /// requiring the runtime value to already be a string. Keeping this as an
    /// explicit nested primitive cast preserves erased `any`/`unknown` values
    /// until the existing string coercion boundary instead of asserting that
    /// their runtime representation is a Rust `String`.
    pub(in crate::lowering) fn parse_float_operand(
        &mut self,
        source_name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} requires exactly one argument"),
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) == Some(&Type::String) {
            return Ok(operand);
        }
        if !self.primitive_cast_accepts_operand(PrimitiveCastOp::ToString, operand_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{source_name} argument cannot be coerced to a string"),
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand,
            },
            ty: string_ty,
            span: self.span(argument.span().start, argument.span().end),
        }))
    }

    /// Lower direct TypeScript `Number.parseInt(...)` calls.
    pub(in crate::lowering) fn number_parse_int_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Number") || member.property.name != "parseInt" {
            return Ok(None);
        }
        let (operand, radix) = self.parse_int_operand("Number.parseInt", call, body)?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ParseIntRadix { operand, radix },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower and validate TypeScript `parseInt` string and radix arguments.
    ///
    /// Returns the string operand and the numeric radix, in source evaluation
    /// order. An absent radix is the literal `0`, which `parseInt` treats like
    /// `undefined` (decimal, with a `0x` prefix switching to hexadecimal), so
    /// every `parseInt` lowers to the one `ParseIntRadix` op. A present radix
    /// is asserted to `Float` when it is erased.
    pub(in crate::lowering) fn parse_int_operand(
        &mut self,
        source_name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let (argument, radix) = match call.arguments.as_slice() {
            [argument] => (argument, None),
            [argument, radix] => (argument, Some(radix)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a string argument and optional numeric radix"),
                ));
            }
        };
        let operand = self.argument(argument, body)?;
        let operand = match self.ctx.krate.types.get(Self::expr_ty(body, operand)) {
            Some(Type::String) => operand,
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::String)) =>
            {
                operand
            }
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty: string_ty,
                    span: self.span(argument.span().start, argument.span().end),
                })
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a string argument"),
                ));
            }
        };
        let Some(radix) = radix else {
            let int_ty = self.ctx.krate.types.intern(Type::Int);
            let default_radix = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Int(0)),
                ty: int_ty,
                span: self.span(call.span.start, call.span.end),
            });
            return Ok((operand, default_radix));
        };
        let radix_expr = self.argument(radix, body)?;
        let radix_expr = match self.ctx.krate.types.get(Self::expr_ty(body, radix_expr)) {
            Some(Type::Int | Type::Float) => radix_expr,
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: radix_expr },
                    ty: float_ty,
                    span: self.span(radix.span().start, radix.span().end),
                })
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{source_name} requires a numeric radix argument"),
                ));
            }
        };
        Ok((operand, radix_expr))
    }

    /// Lower direct TypeScript `.toString()` calls with an optional radix argument.
    pub(in crate::lowering) fn number_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toString" {
            return Ok(None);
        }
        // `ns.toString(value)` on a utility *namespace* object (a `import * as _`
        // star import or a registered object namespace) is the lodash/es-toolkit
        // free-function `toString`, whose first argument is the value being
        // stringified, not a `Number.prototype.toString` radix. Defer to the
        // generic namespace member-call path instead of treating the value as a
        // radix and rejecting a non-numeric one.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let operand = self.expression(&member.object, body)?;
        // `URL.prototype.toString()` is its `href`. Claimed here rather than
        // by a later handler because this one has already lowered the
        // receiver, and a second lowering would evaluate it twice.
        if call.arguments.is_empty() {
            let span = self.span(call.span.start, call.span.end);
            if let Some(href) = self.url_href_of(operand, span, body) {
                return Ok(Some(href));
            }
        }
        // A modeled host class defines its own `toString`: a
        // `URLSearchParams` serializes to `application/x-www-form-urlencoded`,
        // not to `"[object Object]"`. The generic cast below accepts ANY
        // class-typed receiver, so it has to decline those and leave them to
        // the concern that models them (`stdlib::fetch_types`).
        if self.type_defines_its_own_to_string(Self::expr_ty(body, operand)) {
            return Ok(None);
        }
        if call.arguments.is_empty() && self.expression_is_known_date_value(operand, body) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToString {
                    timestamp_ms: operand,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let operand_ty = Self::expr_ty(body, operand);
        let stringifiable = matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(
                Type::Int
                    | Type::Float
                    | Type::BigInt
                    | Type::String
                    | Type::Unknown
                    | Type::TypeParam { .. }
                    | Type::Class { .. }
            )
        ) || (call.arguments.is_empty()
            && self.union_arms_stringify_faithfully(operand_ty));
        if !stringifiable {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        if let Some(radix_argument) = call.arguments.first() {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "number.toString accepts at most one radix argument",
                ));
            }
            let radix = self.argument(radix_argument, body)?;
            let radix_ty = Self::expr_ty(body, radix);
            if matches!(self.ctx.krate.types.get(radix_ty), Some(Type::String))
                && !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, operand)),
                    Some(Type::Int | Type::Float)
                )
            {
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToString,
                        operand,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if !matches!(
                self.ctx.krate.types.get(radix_ty),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(radix_argument.span().start, radix_argument.span().end),
                    "number.toString radix argument must be numeric",
                ));
            }
            let span = self.span(call.span.start, call.span.end);
            if let Some(call) = self.bigint_to_string_radix(operand, radix, span, body) {
                return Ok(Some(call));
            }
            if matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, operand)),
                Some(Type::Int | Type::Float)
            ) {
                return Ok(Some(self.numeric_format_expr(
                    NumericFormatMethod::Radix,
                    operand,
                    Some(radix),
                    span,
                    body,
                )));
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::NumericToStringRadix { operand, radix },
                ty,
                span,
            })));
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "number.toString radix arguments are not supported yet",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Whether `ty` is a union whose every arm's `toString()` is its string
    /// conversion as the emitter renders it: a primitive, or a host class whose
    /// own `toString` the emitter models (`URL`, `URLSearchParams`).
    ///
    /// For such a union `x.toString()` is `String(x)`, arm by arm. Any other
    /// arm (a user class with its own `toString`, a host class whose tag the
    /// erased coercion does not spell) keeps the call on the general path.
    fn union_arms_stringify_faithfully(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Union(arms)) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        arms.iter().all(|arm| {
            matches!(
                self.ctx.krate.types.get(*arm),
                Some(Type::Int | Type::Float | Type::BigInt | Type::String | Type::Bool)
            ) || matches!(
                self.stdlib_class_of_type(*arm),
                Some(smelt_stdlib::StdlibClass::Url | smelt_stdlib::StdlibClass::UrlSearchParams)
            ) && !self.source_shadows_global("URL")
                && !self.source_shadows_global("URLSearchParams")
        })
    }

    /// Lower `.toFixed(digits)`, `.toPrecision(digits)` and
    /// `.toExponential(digits)` on a numeric receiver.
    ///
    /// Each is one [`ExprKind::NumericFormat`]; other receivers are left for
    /// later dispatch (an erased receiver reaches the same runtime formatter
    /// through its `Number.prototype` member read).
    pub(in crate::lowering) fn number_format_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Some(method) = NumericFormatMethod::from_js_name(&member.property.name) else {
            return Ok(None);
        };
        let operand = self.expression(&member.object, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(None);
        }
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("number.{} accepts at most one argument", method.js_name()),
            ));
        }
        let digits = match call.arguments.first() {
            Some(argument) => {
                let digits = self.argument(argument, body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, digits)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        format!("number.{} argument must be numeric", method.js_name()),
                    ));
                }
                Some(digits)
            }
            None => None,
        };
        let span = self.span(call.span.start, call.span.end);
        Ok(Some(
            self.numeric_format_expr(method, operand, digits, span, body),
        ))
    }

    /// Push a [`ExprKind::NumericFormat`] over a numeric `operand`.
    ///
    /// The op is fallible exactly when the argument may fall outside the
    /// method's range after `ToIntegerOrInfinity`: an absent argument and a
    /// numeric literal inside the range cannot throw, so `x.toFixed(2)` stays a
    /// plain expression and only a computed digit count carries the
    /// `RangeError` edge to an enclosing `try`.
    pub(in crate::lowering) fn numeric_format_expr(
        &mut self,
        method: NumericFormatMethod,
        operand: smelt_hir::ExprId,
        digits: Option<smelt_hir::ExprId>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let (low, high) = method.argument_range();
        let digits_checked = digits.is_some_and(|digits| {
            let constant = match body.expr_at(digits).map(|expr| &expr.kind) {
                Some(ExprKind::Literal(Literal::Float(value))) => Some(*value),
                // An integer outside `i32` is outside every method's range.
                Some(ExprKind::Literal(Literal::Int(value))) => {
                    i32::try_from(*value).ok().map(f64::from)
                }
                _ => None,
            };
            !constant.is_some_and(|value| (low..=high).contains(&value.trunc()))
        });
        let ty = self.ctx.krate.types.intern(Type::String);
        body.push_expr(Expr {
            kind: ExprKind::NumericFormat {
                method,
                operand,
                digits,
                digits_checked,
            },
            ty,
            span,
        })
    }
}
