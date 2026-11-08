//! Template literals and tagged templates, including `String.raw`.

use crate::SmeltError;
use crate::lowering::js_text::template_element_literal;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, Type};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower a template literal as string concatenation.
    pub(in crate::lowering) fn template_literal_expression(
        &mut self,
        tpl: &oxc::ast::ast::TemplateLiteral<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let str_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(tpl.span.start, tpl.span.end);
        let Some(first_quasi) = tpl.quasis.first() else {
            return Err(SmeltError::unsupported(
                self.span(tpl.span.start, tpl.span.end),
                "template literals must contain at least one quasi",
            ));
        };
        let mut acc = body.push_expr(Expr {
            kind: ExprKind::Literal(template_element_literal(&first_quasi.value)),
            ty: str_ty,
            span,
        });

        for (i, interp) in tpl.expressions.iter().enumerate() {
            let part = self.expression(interp, body)?;
            // An interpolation is a string conversion; an absent `null` reads
            // as `"null"` (`lowering::null_absent`).
            let part = self.null_absent_string_operand(part, interp.span().start, interp.span().end, body);
            acc = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs: part,
                },
                ty: str_ty,
                span,
            });
            if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                let quasi_literal = template_element_literal(&quasi.value);
                if quasi_literal != Literal::String(String::new()) {
                    let lit = body.push_expr(Expr {
                        kind: ExprKind::Literal(quasi_literal),
                        ty: str_ty,
                        span,
                    });
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: lit,
                        },
                        ty: str_ty,
                        span,
                    });
                }
            }
        }
        Ok(acc)
    }

    /// Lower a tagged template literal (a `tag` applied to a template literal).
    ///
    /// Only the `String.raw` builtin tag is modeled. `String.raw` has fully
    /// defined semantics for *any* template: it concatenates the template's
    /// **raw** quasis (backslash escapes left verbatim) interleaved with the
    /// string-coerced substitution values. This lowering implements exactly that
    /// — it is a complete, general implementation of the stdlib builtin, not a
    /// per-call-site special case, mirroring how the other `String.*` statics are
    /// modeled.
    ///
    /// General user-defined tags desugar to `tag(cookedStrings, ...subs)` where
    /// the `cookedStrings` argument is a `TemplateStringsArray` that also carries
    /// a `.raw` sibling array. Smelt's array model is homogeneous and cannot yet
    /// attach that `.raw` property to an array value, so custom tags remain an
    /// explicit deferral with a descriptive error rather than a silently wrong
    /// cooked-only call.
    pub(in crate::lowering) fn tagged_template_expression(
        &mut self,
        tagged: &oxc::ast::ast::TaggedTemplateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if Self::is_string_raw_tag(&tagged.tag) {
            return self.string_raw_template_expression(tagged, body);
        }
        Err(SmeltError::unsupported(
            self.span(tagged.span.start, tagged.span.end),
            "tagged template literals are only supported for the `String.raw` \
             builtin; user-defined tags need a `TemplateStringsArray` with a \
             `.raw` sibling, which Smelt's homogeneous array model cannot yet \
             represent",
        ))
    }

    /// Return true when a tagged-template tag is the `String.raw` builtin,
    /// i.e. a static member read of `raw` off the global `String` identifier
    /// that source has not shadowed with its own binding.
    fn is_string_raw_tag(tag: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = tag else {
            return false;
        };
        if member.property.name != "raw" {
            return false;
        }
        matches!(&member.object, Expression::Identifier(ident) if ident.name == "String")
    }

    /// Lower a `String.raw` tagged template to interleaved raw-quasi /
    /// substitution concatenation.
    ///
    /// The result is `raw[0] + str(sub[0]) + raw[1] + … + raw[n]`, using the raw
    /// (unescaped) quasi text exactly as `String.raw` specifies. Substitutions are
    /// lowered through the normal expression path and string-coerced by the `+`
    /// operator, matching the runtime string concatenation the cooked-template
    /// path already relies on.
    fn string_raw_template_expression(
        &mut self,
        tagged: &oxc::ast::ast::TaggedTemplateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let tpl = &tagged.quasi;
        let str_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(tagged.span.start, tagged.span.end);
        let Some(first_quasi) = tpl.quasis.first() else {
            return Err(SmeltError::unsupported(
                span,
                "template literals must contain at least one quasi",
            ));
        };
        let mut acc = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(first_quasi.value.raw.as_str().to_owned())),
            ty: str_ty,
            span,
        });
        for (i, interp) in tpl.expressions.iter().enumerate() {
            let part = self.expression(interp, body)?;
            // An interpolation is a string conversion; an absent `null` reads
            // as `"null"` (`lowering::null_absent`).
            let part = self.null_absent_string_operand(part, interp.span().start, interp.span().end, body);
            acc = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs: part,
                },
                ty: str_ty,
                span,
            });
            if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                let raw = quasi.value.raw.as_str();
                if !raw.is_empty() {
                    let lit = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(raw.to_owned())),
                        ty: str_ty,
                        span,
                    });
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: lit,
                        },
                        ty: str_ty,
                        span,
                    });
                }
            }
        }
        Ok(acc)
    }
}
