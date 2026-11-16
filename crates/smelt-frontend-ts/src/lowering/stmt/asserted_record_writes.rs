//! Keyed writes through an assertion to a typed record:
//! `(record as any)[key] = value` and `delete (record as any)[key]` (or any
//! chain of assertions to a dynamic keyed view, `as unknown as Record<..>`).
//!
//! JavaScript writes such a key on the record object itself, so every later
//! read of `record` sees it. A typed record is a concrete Rust struct, and the
//! assertion erases it into a COPY: lowered as an ordinary erased index write,
//! the write landed in that temporary and was lost (date-fns'
//! `setDefaultOptions` merged its options into `const result: DefaultOptions =
//! {}` this way and always produced `{}`).
//!
//! The write is lowered against the struct instead: the key is compared with
//! each of the record's own fields, and the matching field is assigned the
//! value converted to that field's type (a `delete` clears an optional field).
//! The record keeps its concrete type throughout.
//!
//! What this cannot do: a key that names none of the record's fields would add
//! a new property in JavaScript; a struct has no slot for it, so that write
//! has no effect.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::support::source_spelling;
use oxc::ast::ast::{AssignmentTarget, Expression, TSType, UnaryOperator};
use oxc::span::GetSpan;
use smelt_hir::{
    BinOp, Body, ErasureReason, Expr, ExprKind, Literal, LocalDecl, Pattern, Stmt, Type,
};

/// A keyed write's target: the record local and its declared fields.
struct AssertedRecord {
    /// The local holding the record.
    local: smelt_hir::LocalId,
    /// The record's type.
    ty: smelt_hir::TypeId,
    /// Its own fields, as the record type declares them.
    fields: Vec<smelt_hir::Field>,
}

impl ModuleBuilder<'_> {
    /// Lower `(record as any)[key] = value` in statement position against the
    /// record's fields (see the module docs). `false`, having lowered nothing,
    /// for any other assignment.
    pub(in crate::lowering) fn try_asserted_record_key_write(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if assign.operator != oxc::ast::ast::AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::ComputedMemberExpression(member) = &assign.left else {
            return Ok(false);
        };
        let Some(record) = self.asserted_record(&member.object, body) else {
            return Ok(false);
        };
        let Some(key_local) = self.asserted_record_key(&member.expression, body, block)? else {
            return Ok(false);
        };
        // The value is evaluated once, after the key, as JavaScript does, and
        // held at the erased carrier each branch converts from: which field it
        // lands in, and so at which type, is decided by the key at run time.
        let span = self.span(assign.span.start, assign.span.end);
        let value = self.expression(&assign.right, body)?;
        let erased = self.erase_with(ErasureReason::DynamicOperator, "keyed record write value");
        let value = Self::erase_to_unknown(value, erased, span, body);
        let value_local =
            self.asserted_record_temp("__smelt_value", erased, value, span, body, block);
        for field in record.fields.clone() {
            let stored = body.push_expr(Expr {
                kind: ExprKind::Local(value_local),
                ty: erased,
                span,
            });
            let converted = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: stored,
                    target: field.ty,
                },
                ty: field.ty,
                span,
            });
            self.asserted_record_branch(&record, &field, key_local, converted, span, body, block);
        }
        Ok(true)
    }

    /// Lower `delete (record as any)[key]` in statement position: the
    /// matching OPTIONAL field becomes absent. `false` for any other `delete`.
    pub(in crate::lowering) fn try_asserted_record_key_delete(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if unary.operator != UnaryOperator::Delete {
            return Ok(false);
        }
        let Expression::ComputedMemberExpression(member) = peel(&unary.argument, Wrappers::Parens)
        else {
            return Ok(false);
        };
        let Some(record) = self.asserted_record(&member.object, body) else {
            return Ok(false);
        };
        let Some(key_local) = self.asserted_record_key(&member.expression, body, block)? else {
            return Ok(false);
        };
        let span = self.span(unary.span.start, unary.span.end);
        for field in record.fields.clone() {
            // A required field cannot be absent in its struct; JavaScript would
            // remove it, which the record's type rules out.
            if !matches!(self.ctx.krate.types.get(field.ty), Some(Type::Optional(..))) {
                continue;
            }
            let absent = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty: field.ty,
                span,
            });
            self.asserted_record_branch(&record, &field, key_local, absent, span, body, block);
        }
        Ok(true)
    }

    /// The record a keyed write targets: `local` seen through one or more
    /// assertions to a dynamic keyed view (`as any`, `as unknown`, `as
    /// Record<string, unknown>`, `as unknown as Record<..>`), where `local`
    /// holds a declared record type with fields. `None` for anything else.
    fn asserted_record(&mut self, object: &Expression<'_>, body: &Body) -> Option<AssertedRecord> {
        let mut inner = peel(object, Wrappers::Parens);
        let mut asserted = false;
        loop {
            let (view, expression) = match inner {
                Expression::TSAsExpression(assertion) => {
                    (&assertion.type_annotation, &assertion.expression)
                }
                Expression::TSTypeAssertion(assertion) => {
                    (&assertion.type_annotation, &assertion.expression)
                }
                _ => break,
            };
            if !Self::is_dynamic_keyed_view(view) {
                return None;
            }
            asserted = true;
            inner = peel(expression, Wrappers::Parens);
        }
        if !asserted {
            return None;
        }
        let Expression::Identifier(ident) = peel(inner, Wrappers::Parens) else {
            return None;
        };
        let local = self.scope.lookup(ident.name.as_str())?;
        let ty = Self::local_ty(body, local);
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(ty).cloned() else {
            return None;
        };
        // Only a declared class or interface is a struct the assertion copies.
        // A type alias of an intersection or mapped type (date-fns'
        // `DefaultOptions`) names fields too, but its values are erased objects,
        // whose ordinary erased index write already lands on the shared object.
        if self.class_by_symbol(name).is_none() && self.find_interface(name).is_none() {
            return None;
        }
        let fields = self.json_class_fields(name, &args)?;
        let fields = fields
            .into_iter()
            .filter(|field| field.visibility.is_own_property())
            .collect::<Vec<_>>();
        (!fields.is_empty()).then_some(AssertedRecord { local, ty, fields })
    }

    /// Whether an assertion's target type is a view that any string key may
    /// index: `any`, `unknown`, `Record<..>` or an index-signature literal.
    fn is_dynamic_keyed_view(view: &TSType<'_>) -> bool {
        match view {
            TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_) => true,
            TSType::TSTypeReference(reference) => matches!(
                &reference.type_name,
                oxc::ast::ast::TSTypeName::IdentifierReference(name) if name.name == "Record"
            ),
            TSType::TSTypeLiteral(literal) => literal
                .members
                .iter()
                .any(|member| matches!(member, oxc::ast::ast::TSSignature::TSIndexSignature(_))),
            _ => false,
        }
    }

    /// Lower the key of a keyed record write into a string local, evaluated
    /// once. `None`, having lowered nothing, when the key is not a string.
    fn asserted_record_key(
        &mut self,
        key: &Expression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<Option<smelt_hir::LocalId>, SmeltError> {
        let mark = self.speculation_mark(body);
        let key_expr = self.expression(key, body)?;
        let key_ty = Self::expr_ty(body, key_expr);
        if !matches!(self.ctx.krate.types.get(key_ty), Some(Type::String)) {
            self.roll_back_to(mark, body);
            return Ok(None);
        }
        let span = self.span(key.span().start, key.span().end);
        Ok(Some(self.asserted_record_temp(
            "__smelt_key",
            key_ty,
            key_expr,
            span,
            body,
            block,
        )))
    }

    /// Bind `value` to a fresh local named `name` in `block`.
    fn asserted_record_temp(
        &mut self,
        name: &str,
        ty: smelt_hir::TypeId,
        value: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> smelt_hir::LocalId {
        let symbol = self.intern_synthesized_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat,
                ty,
                value: Some(value),
            },
        );
        local
    }

    /// Push `if (key === "<field>") record.<field> = value;` into `block`.
    fn asserted_record_branch(
        &mut self,
        record: &AssertedRecord,
        field: &smelt_hir::Field,
        key_local: smelt_hir::LocalId,
        value: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        // The property's source spelling (`weekStartsOn`), which is what a
        // runtime key holds, not the symbol's internal rendering.
        let key_text = source_spelling(&self.ctx.krate, field.name)
            .unwrap_or_default()
            .to_owned();
        let key = body.push_expr(Expr {
            kind: ExprKind::Local(key_local),
            ty: string_ty,
            span,
        });
        let field_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(key_text)),
            ty: string_ty,
            span,
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::Eq,
                lhs: key,
                rhs: field_key,
            },
            ty: bool_ty,
            span,
        });
        let receiver = body.push_expr(Expr {
            kind: ExprKind::Local(record.local),
            ty: record.ty,
            span,
        });
        let target = body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver,
                field: field.name,
            },
            ty: field.ty,
            span,
        });
        let then_block = body.push_block(span);
        body.push_stmt_to_block(then_block, Stmt::Assign { target, value });
        body.push_stmt_to_block(
            block,
            Stmt::If {
                cond,
                then_block,
                else_block: None,
            },
        );
    }
}
