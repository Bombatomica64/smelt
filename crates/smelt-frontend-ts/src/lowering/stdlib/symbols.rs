//! `Symbol(..)` and `Symbol.for(..)` calls and their runtime spellings.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::{has_lone_surrogate, js_str_text};
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{Body, Expr, ExprKind, Literal, Span, Type};

/// Render the opaque runtime string that stands for a *unique* `Symbol(...)`.
///
/// A unique symbol has fresh identity per evaluation site, so the spelling is
/// keyed by the call's source SITE — its module and offset, packed into one
/// decimal number (`file << 32 | offset`) so the spelling keeps the
/// `Symbol(d)@<digits>` shape the runtime key derivation parses. The module is
/// part of the identity because two modules routinely declare
/// `export const A: unique symbol = Symbol()` at the same offset, and those are
/// two symbols; module 0 keeps its bare offset. The description is carried along because
/// two spellings of the same symbol are compared as plain strings at runtime
/// (`js_strict_eq`), so a description-less variant would compare unequal to the
/// described one for the very same source symbol. Every path that materializes a
/// unique symbol — the ordinary call-lowering path and the const-initializer
/// folding path — must go through here so the two cannot drift apart.
pub(in crate::lowering) fn unique_symbol_spelling(description: &str, site: Span) -> String {
    let site = (u64::from(site.file.0) << 32_u32) | u64::from(site.start);
    format!("Symbol({description})@{site}")
}

/// Render the opaque runtime string that stands for a registry `Symbol.for(...)`.
///
/// Registry symbols are globally interned by description, so the spelling omits
/// the source offset and two `Symbol.for("k")` sites share one string.
pub(in crate::lowering) fn registry_symbol_spelling(description: &str) -> String {
    format!("Symbol.for({description})")
}

impl ModuleBuilder<'_> {
    /// Lower JavaScript `Symbol(description)` and `Symbol.for(key)` calls.
    ///
    /// The value is a `symbol` (`Type::Symbol`) whose identity is its
    /// spelling: a registry symbol is keyed by its description, a unique one
    /// by its creation site (see [`unique_symbol_spelling`]).
    pub(in crate::lowering) fn symbol_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && object.name == "Symbol"
            && member.property.name == "for"
        {
            return self.symbol_like_call(call, body, true);
        }
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Symbol" {
            return Ok(None);
        }
        self.symbol_like_call(call, body, false)
    }

    /// Lower a supported symbol-producing call to its `symbol` literal.
    pub(in crate::lowering) fn symbol_like_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        global: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) supports at most one description argument",
            ));
        }
        let description_arg = call.arguments.first();
        let checked_description = description_arg
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::Symbol);
        if let Some(description) = checked_description
            && !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, description)),
                Some(Type::String)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) description must be a string",
            ));
        }
        if let Some(Argument::StringLiteral(literal)) = description_arg
            && has_lone_surrogate(literal.value)
        {
            return Err(SmeltError::unsupported(
                self.span(literal.span.start, literal.span.end),
                "a Symbol description holding a lone surrogate is not supported",
            ));
        }
        let description = description_arg
            .and_then(|argument| match argument {
                Argument::StringLiteral(literal) => Some(js_str_text(literal.value).into_owned()),
                _ => None,
            })
            .unwrap_or_default();
        let value = if global {
            registry_symbol_spelling(&description)
        } else {
            unique_symbol_spelling(&description, self.span(call.span.start, call.span.end))
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Symbol(value)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// `sym.description` on a `symbol`: the text it was created with, or
    /// `undefined`, so the read is an optional string.
    ///
    /// Only a receiver typed `symbol` itself; an optional receiver keeps the
    /// ordinary member paths.
    pub(in crate::lowering) fn symbol_member_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if member.property.name != "description"
            || !matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Symbol))
        {
            return None;
        }
        let string = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::Optional(string));
        Some(body.push_expr(Expr {
            kind: ExprKind::SymbolDescription { operand: receiver },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }
}
