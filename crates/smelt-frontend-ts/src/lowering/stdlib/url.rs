//! Lowering for the WHATWG `URL` class.
//!
//! A `URL` is a mutable reference object, and that is the whole reason it is a
//! modeled class here rather than its `href` string. Source code reads its
//! components (`url.pathname`), writes them (`url.pathname = '/x'`,
//! `url.pathname += '/'`), and hands the same object on (`new Request(url)`);
//! every alias observes a write, and `url.searchParams` is a live view whose
//! mutations rewrite the query. Lowering `new URL(x)` to its `href` string, as
//! this frontend used to, kept only the first `new URL(x).field` read: a
//! component read through a local answered `null` and a component write was
//! silently dropped.
//!
//! The rules here are stated on the receiver's lowered TYPE (the modeled `URL`
//! class, unless a user class of that name shadows it), never on a spelling:
//!
//! * `new URL(input, base?)` is [`ExprKind::UrlNew`], typed `URL`. Its
//!   operands are strings; a `URL` operand is read as its `href`, the
//!   stringification the constructor performs.
//! * A component read is [`UrlOp::Get`], `searchParams` is
//!   [`UrlOp::SearchParams`], and `toString()`/`toJSON()` read `href`.
//! * A component write, plain or compound, is [`UrlOp::Set`]. The value it
//!   stores is computed by the shared assignment rule, so `+=` and `||=` mean
//!   what they mean everywhere else.

use oxc::ast::ast::{Argument, Expression};
use crate::SmeltError;
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Type, UrlField, UrlOp};

use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Return the modeled `URL` class type.
    pub(in crate::lowering) fn url_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("URL");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `URL` class.
    pub(in crate::lowering) fn is_url_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::Url)
            && !self.user_class_shadows("URL")
    }

    /// Lower `new URL(input, base?)` to a parsed URL value.
    ///
    /// The constructor throws a `TypeError` for text that is not a URL, which
    /// is why the node lowers to a fallible builtin call in MIR.
    pub(in crate::lowering) fn url_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let (input, base) = match new_expr.arguments.as_slice() {
            [input] => (input, None),
            [input, base] => (input, Some(base)),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "new URL() takes a URL and an optional base",
                ));
            }
        };
        let input = self.url_string_argument(input, body, new_expr.span)?;
        let base = match base {
            Some(base) => Some(self.url_string_argument(base, body, new_expr.span)?),
            None => None,
        };
        let ty = self.url_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::UrlNew { input, base },
            ty,
            span,
        }))
    }

    /// Lower a `URL` constructor operand to the string it is parsed from.
    ///
    /// A string is used as is, a `URL` is read as its `href`, and any other
    /// string-compatible value is cast to a string.
    pub(in crate::lowering) fn url_string_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        url_span: oxc::span::Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let url = self.argument(argument, body)?;
        let span = self.span(argument.span().start, argument.span().end);
        if let Some(href) = self.url_href_of(url, span, body) {
            return Ok(href);
        }
        let url_ty = Self::expr_ty(body, url);
        if self.ctx.krate.types.get(url_ty) == Some(&Type::String) {
            return Ok(url);
        }
        if self.is_string_compatible_type(url_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: url,
                    target: string_ty,
                },
                ty: string_ty,
                span,
            }));
        }
        Err(SmeltError::unsupported(
            self.span(url_span.start, url_span.end),
            "new URL(text) requires a string URL argument",
        ))
    }

    /// Read a `URL`-typed expression as its `href`, or `None` for any other
    /// type.
    ///
    /// The stringification every string-taking WHATWG API applies to a `URL`
    /// argument (`new URL(url)`, `new Request(url)`, `fetch(url)`).
    pub(in crate::lowering) fn url_href_of(
        &mut self,
        value: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !self.is_url_type(Self::expr_ty(body, value)) {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Some(body.push_expr(Expr {
            kind: ExprKind::UrlOp {
                op: UrlOp::Get(UrlField::Href),
                url: value,
                args: Vec::new(),
            },
            ty,
            span,
        }))
    }

    /// Lower a component read (`url.pathname`) or `url.searchParams` on a
    /// concrete `URL` receiver.
    ///
    /// The property name gates before the receiver is lowered, for the reason
    /// `http_property_read` gives: lowering pushes into the body, so only the
    /// names this class owns pay for asking the receiver's type.
    pub(in crate::lowering) fn url_property_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let property = member.property.name.as_str();
        let op = match UrlField::from_property(property) {
            Some(field) => UrlOp::Get(field),
            None if property == "searchParams" => UrlOp::SearchParams,
            None => return Ok(None),
        };
        if self.binding_is_known_not_url(&member.object, body) {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let (receiver, receiver_ty) = if member.optional {
            (receiver, Self::expr_ty(body, receiver))
        } else {
            self.present_receiver(receiver, span, body)
        };
        if !self.is_url_type(receiver_ty) {
            return Ok(None);
        }
        let ty = match op {
            UrlOp::SearchParams => self.url_search_params_type(),
            UrlOp::Get(_) | UrlOp::Set(_) => self.ctx.krate.types.intern(Type::String),
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlOp {
                op,
                url: receiver,
                args: Vec::new(),
            },
            ty,
            span,
        })))
    }

    /// Lower `url.toJSON()` on a concrete `URL`: it answers the `href`.
    ///
    /// Registered in the builtin call-handler chain. `toString()` is the same
    /// read, claimed by `number_to_string_call`, the handler that already owns
    /// every `toString()` receiver.
    pub(in crate::lowering) fn url_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toJSON" || !call.arguments.is_empty() {
            return Ok(None);
        }
        if self
            .receiver_class_hint(&member.object, body)
            .is_some_and(|class| class != smelt_stdlib::StdlibClass::Url)
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let span = self.span(call.span.start, call.span.end);
        let (receiver, _) = self.present_receiver(receiver, span, body);
        Ok(self.url_href_of(receiver, span, body))
    }

    /// Lower `url.<component> = value` (or `op=`) into a component WRITE.
    ///
    /// Returns `Ok(None)` for any target that is not a settable component of a
    /// concrete `URL`, so ordinary member assignment proceeds. A write to
    /// `origin` (a getter alone) is a `TypeError` in strict-mode code, which a
    /// module is; it is reported rather than lowered.
    ///
    /// A compound operator reads the component back, so the receiver is
    /// evaluated by the read and by the write. That is the source's own
    /// evaluation only for a receiver without side effects, so a compound write
    /// is lowered only through one.
    pub(in crate::lowering) fn try_url_assignment_expression(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let oxc::ast::ast::AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(None);
        };
        let Some(field) = UrlField::from_property(member.property.name.as_str()) else {
            return Ok(None);
        };
        if self.binding_is_known_not_url(&member.object, body) {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_url_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(assign.span.start, assign.span.end);
        if !field.is_settable() {
            return Err(SmeltError::unsupported(
                span,
                "`URL.origin` is read-only; assigning it throws a TypeError",
            ));
        }
        if assign.operator != oxc::ast::ast::AssignmentOperator::Assign
            && !Self::is_side_effect_free_receiver(&member.object)
        {
            return Err(SmeltError::unsupported(
                span,
                "a compound `URL` component write needs a receiver without side effects",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let read = body.push_expr(Expr {
            kind: ExprKind::UrlOp {
                op: UrlOp::Get(field),
                url: receiver,
                args: Vec::new(),
            },
            ty: string_ty,
            span: self.span(member.span.start, member.span.end),
        });
        let value = self.assignment_value(assign, read, body)?;
        // The setter takes a string. TypeScript types every setter `string`,
        // so a value of another type has already been asserted by the source.
        let value = Self::callback_coerce_to_string(value, string_ty, body, span);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlOp {
                op: UrlOp::Set(field),
                url: receiver,
                args: vec![value],
            },
            // The assignment evaluates to the ASSIGNED value, not to the
            // component the setter normalized it to.
            ty: string_ty,
            span,
        })))
    }

    /// Whether `object` is a binding whose type is already known and is not
    /// `URL`, so a component name on it belongs to some other type.
    ///
    /// Checked BEFORE the receiver is lowered: the component names (`port`,
    /// `host`, `hash`, `search`) are common record fields, and lowering a
    /// receiver only to decline leaves a discarded expression in the body.
    fn binding_is_known_not_url(&self, object: &Expression<'_>, body: &Body) -> bool {
        let Expression::Identifier(identifier) = object else {
            return false;
        };
        let name = identifier.name.as_str();
        let Some(local) = self.scope.lookup(name) else {
            return false;
        };
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let ty = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        !matches!(self.ctx.krate.types.get(ty), Some(Type::Unknown | Type::TypeParam { .. }))
            && !self.is_url_type(ty)
    }

    /// Whether evaluating a receiver twice is evaluating it once: a binding or
    /// `this`. A member chain is not, because a getter in it is a call.
    fn is_side_effect_free_receiver(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::Identifier(_) | Expression::ThisExpression(_) => true,
            Expression::ParenthesizedExpression(inner) => {
                Self::is_side_effect_free_receiver(&inner.expression)
            }
            _ => false,
        }
    }
}
