//! The `Date` global: `Date.now`, `Date.UTC`, construction and instance methods.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use oxc::ast::ast::{CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, DatePart, ErasureReason, Expr, ExprKind, PrimitiveCastOp, Stmt, Type};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Return true when a timestamp-backed expression is statically known to be a JavaScript Date.
    ///
    /// Date values use numeric timestamps in generated Rust, so runtime Rust
    /// type inspection cannot distinguish them from arbitrary source numbers.
    /// TypeScript still guarantees `Date` and `T extends Date` values satisfy
    /// `instanceof Date`, and direct Date constructors retain that provenance
    /// until this predicate is lowered.
    pub(in crate::lowering) fn type_is_known_date_value(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Class { name, .. }) => self.ctx.krate.symbols.get(*name) == Some("Date"),
            Some(Type::Optional(inner)) => self.type_is_known_date_value(*inner),
            Some(Type::Union(items)) => {
                let values = items
                    .iter()
                    .copied()
                    .filter(|item| self.ctx.krate.types.get(*item) != Some(&Type::None))
                    .collect::<Vec<_>>();
                !values.is_empty()
                    && values
                        .into_iter()
                        .all(|item| self.type_is_known_date_value(item))
            }
            _ => false,
        }
    }

    /// Return true when an expression carries JavaScript `Date` identity despite timestamp storage.
    pub(in crate::lowering) fn expression_is_known_date_value(
        &self,
        value: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        let Some(expr) = body.expr_at(value) else {
            return false;
        };
        if self.type_is_known_date_value(expr.ty) {
            return true;
        }
        match &expr.kind {
            ExprKind::DateFromParts { .. } | ExprKind::DateFromValue { .. } => true,
            ExprKind::Local(local) => self.scope.is_date_value(*local),
            ExprKind::TypeAssert {
                value: asserted_value,
            } => self.expression_is_known_date_value(*asserted_value, body),
            ExprKind::Call { callee, .. } => body
                .expr_at(*callee)
                .and_then(|callee| match callee.kind {
                    ExprKind::Item(item) => Some(item),
                    _ => None,
                })
                .is_some_and(|item| self.ctx.date_returning_functions.contains(&item)),
            _ => false,
        }
    }

    /// Lower supported TypeScript `Date` calls.
    pub(in crate::lowering) fn date_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if matches!(&call.callee, Expression::StaticMemberExpression(_))
            && self.recognized_call(call) == Some(RuleId::TsDateNow)
        {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.now() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateNow,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if let Some(expr) = self.date_utc_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.date_member_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if self.recognized_call(call) != Some(RuleId::TsDateToIsoString) {
            return Ok(None);
        }
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if !self.names_host(callee.name.as_str(), "Date") {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.toISOString() does not accept arguments",
            ));
        }
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateToIsoString { timestamp_ms },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Date.UTC(year, month, ...)` into Smelt's timestamp-from-parts form.
    pub(in crate::lowering) fn date_utc_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Date") || member.property.name != "UTC" {
            return Ok(None);
        }
        if !(2..=7).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.UTC requires between two and seven numeric arguments",
            ));
        }
        let mut parts = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let part = self.argument(argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, part)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.UTC arguments must be numeric",
                ));
            }
            parts.push(part);
        }
        let ty = self.ctx.krate.types.intern(Type::Int);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromParts { parts },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower supported `new Date(...)` expressions to a timestamp value.
    pub(in crate::lowering) fn new_date_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::DateFromValue {
                value: timestamp_ms,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower `new (date.constructor as DateCtor)(value)` while retaining Date identity.
    pub(in crate::lowering) fn dynamic_date_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_date_constructor_member_reference(&new_expr.callee, body) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor calls require exactly one value argument",
            ));
        };
        let value = self.argument(value, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromValue { value },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Return whether a `.constructor` callee is read from a statically typed Date local.
    fn is_date_constructor_member_reference(
        &self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> bool {
        let member = match expression {
            Expression::StaticMemberExpression(member) => member,
            Expression::ParenthesizedExpression(parenthesized) => {
                return self.is_date_constructor_member_reference(&parenthesized.expression, body);
            }
            Expression::TSAsExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            Expression::TSSatisfiesExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            Expression::TSNonNullExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            _ => return false,
        };
        if member.property.name != "constructor" {
            return false;
        }
        let Expression::Identifier(receiver) = &member.object else {
            return false;
        };
        let Some(local) = self.scope.lookup(receiver.name.as_str()) else {
            return false;
        };
        let Some(decl) = body.local_at(local) else {
            return false;
        };
        matches!(
            self.ctx.krate.types.get(decl.ty),
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(*name) == Some("Date")
        )
    }

    /// Lower guarded dynamic Date constructor identifiers such as `new constructor(0)`.
    pub(in crate::lowering) fn dynamic_identifier_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "constructor" || !self.scope.is_bound(callee.name.as_str()) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor identifiers require exactly one value argument",
            ));
        };
        Ok(Some(self.argument(value, body)?))
    }

    /// Return the timestamp expression represented by a supported `new Date(...)`.
    pub(in crate::lowering) fn date_constructor_timestamp(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() >= 2 {
            if new_expr.arguments.len() > 7 {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Date(year, month, ...) supports at most seven numeric arguments",
                ));
            }
            let mut parts = Vec::with_capacity(new_expr.arguments.len());
            for argument in &new_expr.arguments {
                let part = self.argument(argument, body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, part)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "Date constructor parts must be numeric",
                    ));
                }
                parts.push(part);
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromParts { parts },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        let [timestamp_arg] = new_expr.arguments.as_slice() else {
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateNow,
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let timestamp_ms = self.argument(timestamp_arg, body)?;
        let timestamp_ty = Self::expr_ty(body, timestamp_ms);
        if matches!(
            self.ctx.krate.types.get(timestamp_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(timestamp_ms);
        }
        if self.is_date_constructor_arg_type(timestamp_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromValue {
                    value: timestamp_ms,
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(new_expr.span.start, new_expr.span.end),
            "new Date(timestamp) requires a numeric or DateArg-compatible timestamp",
        ))
    }

    /// Return true for types accepted by JavaScript's one-argument Date constructor.
    pub(in crate::lowering) fn is_date_constructor_arg_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. },
            ) => true,
            Some(Type::Optional(item)) => self.is_date_constructor_arg_type(*item),
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
                matches!(self.ctx.krate.types.get(item), Some(Type::None))
                    || self.is_date_constructor_arg_type(item)
            }),
            _ => false,
        }
    }

    /// Lower supported Date receiver methods using Smelt's timestamp Date model.
    pub(in crate::lowering) fn date_member_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let method = member.property.name.as_str();
        if method == "toISOString" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.toISOString() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToIsoString {
                    timestamp_ms: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // `toUTCString` (and its legacy alias `toGMTString`) is the HTTP date
        // form; `Expires=` in a cookie and a `Date` header carry it.
        if matches!(method, "toUTCString" | "toGMTString") && call.arguments.is_empty() {
            let receiver = self.expression(&member.object, body)?;
            let timestamp_ms = self.date_receiver_timestamp(receiver, member, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToUtcString { timestamp_ms },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "getTime" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTime() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "setTime" {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let receiver_ty = Self::expr_ty(body, receiver);
            if !self.is_date_constructor_arg_type(receiver_ty) {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "Date.setTime() receiver must be a timestamp or Date-like value",
                ));
            }
            let Some(argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            };
            let value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value = if self.is_date_constructor_arg_type(value_ty) {
                value
            } else if self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty))
            {
                self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                )
            } else {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.setTime() argument must be numeric",
                ));
            };
            if let Expression::Identifier(identifier) = &member.object
                && let Some(local) = self.scope.lookup(identifier.name.as_str())
            {
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: receiver_ty,
                    span: self.span(identifier.span.start, identifier.span.end),
                });
                self.push_stmt_to_sink(body, Stmt::Assign { target, value });
            }
            return Ok(Some(value));
        }
        if method == "getTimezoneOffset" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTimezoneOffset() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateTimezoneOffset,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let getter_part = match method {
            "getFullYear" | "getUTCFullYear" => Some(DatePart::FullYear),
            "getMonth" | "getUTCMonth" => Some(DatePart::Month),
            "getDate" | "getUTCDate" => Some(DatePart::Date),
            "getDay" | "getUTCDay" => Some(DatePart::Day),
            "getHours" | "getUTCHours" => Some(DatePart::Hour),
            "getMinutes" | "getUTCMinutes" => Some(DatePart::Minute),
            "getSeconds" | "getUTCSeconds" => Some(DatePart::Second),
            "getMilliseconds" | "getUTCMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        if let Some(part) = getter_part {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Date.{method}() does not accept arguments"),
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let timestamp_ms = self.date_receiver_timestamp(receiver, member, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateGetPart { part, timestamp_ms },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let setter_part = match method {
            "setFullYear" | "setUTCFullYear" => Some(DatePart::FullYear),
            "setMonth" | "setUTCMonth" => Some(DatePart::Month),
            "setDate" | "setUTCDate" => Some(DatePart::Date),
            "setHours" | "setUTCHours" => Some(DatePart::Hour),
            "setMinutes" | "setUTCMinutes" => Some(DatePart::Minute),
            "setSeconds" | "setUTCSeconds" => Some(DatePart::Second),
            "setMilliseconds" | "setUTCMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        let Some(part) = setter_part else {
            return Ok(None);
        };
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Date.{method}() requires at least one numeric argument"),
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let mut values = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let mut value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value_is_numeric = matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. })
            );
            let narrowed_numeric = self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty));
            if !value_is_numeric && !narrowed_numeric {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    format!("Date.{method}() arguments must be numeric"),
                ));
            }
            if !value_is_numeric {
                value = self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                );
            }
            values.push(value);
        }
        let ty = Self::expr_ty(body, receiver);
        let value = body.push_expr(Expr {
            kind: ExprKind::DateSetPart {
                part,
                timestamp_ms: receiver,
                values,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        });
        if let Expression::Identifier(identifier) = &member.object
            && let Some(local) = self.scope.lookup(identifier.name.as_str())
        {
            let target = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(identifier.span.start, identifier.span.end),
            });
            self.push_stmt_to_sink(body, Stmt::Assign { target, value });
        }
        Ok(Some(value))
    }

    /// Convert a Date-like receiver into the timestamp expression used by Date operations.
    pub(in crate::lowering) fn date_receiver_timestamp(
        &mut self,
        receiver: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver_ty = Self::expr_ty(body, receiver);
        if matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(receiver);
        }
        if self.is_date_constructor_arg_type(receiver_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(member.object.span().start, member.object.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(member.object.span().start, member.object.span().end),
            "Date method receiver must be a timestamp or Date-like value",
        ))
    }

    /// Lower `tz(zone)` from `@date-fns/tz` to its date-context function value.
    pub(in crate::lowering) fn date_fns_timezone_context_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.host_module_export_path(&call.callee)
            != Some((smelt_stdlib::HostModuleId::DateFnsTz, "tz".to_owned()))
        {
            return Ok(None);
        }
        let [timezone] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "@date-fns/tz tz(...) requires exactly one time zone string",
            ));
        };
        let timezone = self.argument(timezone, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, timezone)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "@date-fns/tz tz(...) requires a time zone string",
            ));
        }
        let unknown = self.erase_with(ErasureReason::UntypedPackage, "@date-fns/tz");
        let ty = self
            .ctx
            .krate
            .types
            .intern(Type::Function(smelt_hir::FunctionType {
                params: vec![unknown],
                rest: None,
                required_params: Some(1),
                mutable_params: Vec::new(),
                return_ty: unknown,
                is_async: false,
                may_throw: false,
            }));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateTimezoneContext { timezone },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
