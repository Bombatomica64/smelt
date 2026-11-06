//! `forEach` over lists, maps and sets, lowered as a loop over the receiver with the
//! callback body inlined.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::arrow_block_statements;
use oxc::ast::ast::{Argument, BindingPattern, Expression, Statement};
use oxc::span::GetSpan;
use smelt_hir::{
    BinOp, Body, DictProjectionOp, ErasureReason, Expr, ExprKind, Literal, LocalDecl, Pattern,
    SetProjectionOp, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// The key and value types of a KEYED collection, for a `forEach` loop.
    ///
    /// `Some` for the collections whose `forEach` callback receives a key as
    /// its second argument rather than a numeric index: a record, a `Map`, and
    /// the modeled host collections. One rule, so a host collection's callback
    /// is typed from its own surface — `FormData`'s entry value is the same
    /// `string | File` union every other member of that surface answers.
    fn for_each_keyed_entry_types(
        &mut self,
        iter_ty: smelt_hir::TypeId,
    ) -> Option<(smelt_hir::TypeId, smelt_hir::TypeId)> {
        if let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.ctx.krate.types.get(iter_ty).cloned()
        {
            return Some((key_ty, value_ty));
        }
        if self.is_modeled_class(iter_ty, smelt_stdlib::StdlibClass::FormData) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.form_data_value_type();
            return Some((string_ty, value_ty));
        }
        None
    }

    /// Project a keyed collection into its `[key, value]` entries list.
    ///
    /// The projection each collection already models, so the loop iterates the
    /// same insertion-ordered list its own `entries()` answers.
    fn for_each_entries_projection(
        &self,
        receiver: smelt_hir::ExprId,
        iter_ty: smelt_hir::TypeId,
        entries_ty: smelt_hir::TypeId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if self.is_modeled_class(iter_ty, smelt_stdlib::StdlibClass::FormData) {
            return body.push_expr(Expr {
                kind: ExprKind::FormDataOp {
                    op: smelt_hir::FormDataOp::Entries,
                    form: receiver,
                    args: Vec::new(),
                },
                ty: entries_ty,
                span,
            });
        }
        body.push_expr(Expr {
            kind: ExprKind::DictProjection {
                op: DictProjectionOp::Entries,
                dict: receiver,
            },
            ty: entries_ty,
            span,
        })
    }

    /// Lower side-effecting `array.forEach((item) => { ... })` as a normal loop.
    pub(in crate::lowering) fn for_each_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if member.property.name != "forEach" {
            return Ok(false);
        }
        let [Argument::ArrowFunctionExpression(arrow)] = call.arguments.as_slice() else {
            return Ok(false);
        };
        let Some(item_param) = arrow.params.items.first() else {
            // A `forEach` whose callback has no fixed item parameter — a bare
            // `() => ...` side effect or a rest-only `(...args) => ...` collector
            // — is not modeled by this statement-loop shortcut. Decline so the
            // general callback lowering (which supports rest parameters through
            // the closure-body path) handles it instead of failing here.
            return Ok(false);
        };
        // A parameter default only the closure-body path applies.
        if arrow.params.items.iter().any(|param| param.initializer.is_some()) {
            return Ok(false);
        }
        let mut iter = self.expression(&member.object, body)?;
        let iter_ty = Self::expr_ty(body, iter);
        // A KEYED collection's `forEach` receives the *key* as its second
        // argument, not a numeric index: iterate the `[key, value]` entries
        // list and bind the callback parameters from each entry tuple. Which
        // collections are keyed comes from
        // [`Self::for_each_keyed_entry_types`], so a `Map`, a record and a
        // modeled host collection such as `FormData` share one rule — Hono's
        // `formData.forEach((value, key) => key.endsWith('[]'))` used to fall
        // into the ARRAY path below, where the parameters got no type at all
        // and `key.endsWith` reported "string prefix/suffix methods require
        // string receiver and argument" for a key the surface has always known
        // to be a string.
        if let Some((key_ty, value_ty)) = self.for_each_keyed_entry_types(iter_ty)
            && arrow.params.items.len() >= 2
        {
            let span = self.span(call.span.start, call.span.end);
            let entry_ty = self
                .ctx
                .krate
                .types
                .intern(Type::Tuple(vec![key_ty, value_ty]));
            let entries_ty = self.ctx.krate.types.intern(Type::List(entry_ty));
            // The receiver is bound to a local ONLY when the callback declares
            // the spec's third parameter (the collection itself): the binding
            // exists so that parameter can be forwarded without evaluating the
            // receiver expression twice — `getForm().forEach(..)` must call
            // `getForm` once — and a callback that does not name it should not
            // pay for a local nobody reads (an erased map's extra binding is
            // two more `SmeltUnknown` mentions in the generated crate, which
            // is a ratchet regression for a value that did not change).
            let source_local = if arrow.params.items.len() >= 3 {
                let source_symbol = self.ctx.krate.symbols.intern("__for_each_source");
                let source_local = body.push_local(LocalDecl {
                    name: Some(source_symbol),
                    ty: iter_ty,
                    mutable: false,
                    span,
                });
                let source_pat = body.push_pattern(Pattern::Binding(source_local));
                body.push_stmt_to_block(
                    block,
                    Stmt::Let {
                        pat: source_pat,
                        ty: iter_ty,
                        value: Some(iter),
                    },
                );
                Some(source_local)
            } else {
                None
            };
            let projected = source_local.map_or(iter, |local| {
                body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: iter_ty,
                    span,
                })
            });
            let entries =
                self.for_each_entries_projection(projected, iter_ty, entries_ty, span, body);
            let entry_symbol = self.ctx.krate.symbols.intern("__for_each_entry");
            let entry_local = body.push_local(LocalDecl {
                name: Some(entry_symbol),
                ty: entry_ty,
                mutable: false,
                span,
            });
            let entry_pat = body.push_pattern(Pattern::Binding(entry_local));
            let loop_body = body.push_block(self.arrow_body_span(arrow));
            let mut param_names = Vec::new();
            for param in arrow.params.items.iter().take(3) {
                Self::binding_pattern_names(&param.pattern, &mut param_names);
            }
            let saved_locals = param_names
                .iter()
                .map(|name| (name.clone(), self.scope.lookup(name)))
                .collect::<Vec<_>>();
            for (param_index, param) in arrow.params.items.iter().take(3).enumerate() {
                // Callback order is `(value, key, collection)`; entries are
                // `[key, value]`. The third parameter is the collection, which
                // JavaScript passes to every `forEach` callback and which was
                // simply left unbound before — a callback that declared it
                // reported an unresolved identifier for its own parameter.
                if param_index == 2 {
                    let Some(local) = source_local else {
                        continue;
                    };
                    let source_read = body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty: iter_ty,
                        span,
                    });
                    self.binding_declaration(
                        &param.pattern,
                        Some(source_read),
                        Some(iter_ty),
                        false,
                        body,
                        loop_body,
                    )?;
                    continue;
                }
                let (tuple_index, ty) = if param_index == 0 {
                    (1_usize, value_ty)
                } else {
                    (0_usize, key_ty)
                };
                let entry_read = body.push_expr(Expr {
                    kind: ExprKind::Local(entry_local),
                    ty: entry_ty,
                    span,
                });
                let extracted = body.push_expr(Expr {
                    kind: ExprKind::TupleIndex {
                        tuple: entry_read,
                        index: tuple_index,
                    },
                    ty,
                    span,
                });
                self.binding_declaration(
                    &param.pattern,
                    Some(extracted),
                    Some(ty),
                    false,
                    body,
                    loop_body,
                )?;
            }
            self.for_each_callback_body(arrow, body, loop_body)?;
            for (name, prior) in saved_locals {
                match prior {
                    Some(local) => {
                        self.scope.bind(name, local);
                    }
                    None => {
                        self.scope.unbind(name.as_str());
                    }
                }
            }
            body.push_stmt_to_block(
                block,
                Stmt::For {
                    pat: entry_pat,
                    iter: entries,
                    body: loop_body,
                },
            );
            return Ok(true);
        }
        let item_ty = match self.ctx.krate.types.get(iter_ty).cloned() {
            Some(Type::List(item_ty)) => item_ty,
            // `set.forEach(value => …)` iterates the set's values in
            // insertion order; project them into a list and reuse the array
            // loop below.
            Some(Type::Set(item_ty)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: iter,
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            // A modeled host collection with one callback parameter iterates
            // its VALUES, the same answer the record arm below gives. Placed
            // before it because a host collection is a `Type::Class`, which no
            // structural arm matches.
            Some(Type::Class { .. }) if self.is_modeled_class(iter_ty, smelt_stdlib::StdlibClass::FormData) => {
                let value_ty = self.form_data_value_type();
                let list_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::FormDataOp {
                        op: smelt_hir::FormDataOp::Values,
                        form: iter,
                        args: Vec::new(),
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                value_ty
            }
            Some(Type::Dict(_, value_ty)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Values,
                        dict: iter,
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                value_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                // An already-erased receiver was recorded where it was decided.
                let item_ty = if self.ctx.krate.types.get(iter_ty) == Some(&Type::Unknown) {
                    self.unknown_type()
                } else {
                    self.erase(ErasureReason::ByName)
                };
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(
                            Type::List(_)
                                | Type::Unknown
                                | Type::TypeParam { .. }
                                | Type::Class { .. }
                        )
                    )
                }) =>
            {
                let item_ty = self.erase(ErasureReason::ByName);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "array forEach statement receiver must be an array",
                ));
            }
        };
        let item_binding = match &item_param.pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding),
            _ => None,
        };
        let item_symbol = if let Some(binding) = item_binding {
            self.intern_source_name(binding.name.as_str())
        } else {
            self.ctx.krate.symbols.intern("__for_each_item")
        };
        let item_local = body.push_local(LocalDecl {
            name: Some(item_symbol),
            ty: item_ty,
            mutable: false,
            span: self.span(item_param.span.start, item_param.span.end),
        });
        let saved_item_local =
            item_binding.map(|binding| self.scope.bind(binding.name.to_string(), item_local));
        let item_pat = body.push_pattern(Pattern::Binding(item_local));
        let index_binding = arrow
            .params
            .items
            .get(1)
            .map(|index_param| {
                let BindingPattern::BindingIdentifier(index_binding) = &index_param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(index_param.span.start, index_param.span.end),
                        "array forEach statement index parameters must be identifiers",
                    ));
                };
                Ok(index_binding)
            })
            .transpose()?;
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let index_counter = index_binding.is_some().then(|| {
            let counter_symbol = self.ctx.krate.symbols.intern("__for_each_index");
            let counter_local = body.push_local(LocalDecl {
                name: Some(counter_symbol),
                ty: index_ty,
                mutable: true,
                span: self.span(call.span.start, call.span.end),
            });
            let zero = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: index_ty,
                span: self.span(call.span.start, call.span.end),
            });
            let counter_pat = body.push_pattern(Pattern::Binding(counter_local));
            body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat: counter_pat,
                    ty: index_ty,
                    value: Some(zero),
                },
            );
            counter_local
        });
        let loop_body = body.push_block(self.arrow_body_span(arrow));
        if item_binding.is_none() {
            let item_value = body.push_expr(Expr {
                kind: ExprKind::Local(item_local),
                ty: item_ty,
                span: self.span(item_param.span.start, item_param.span.end),
            });
            self.binding_declaration(
                &item_param.pattern,
                Some(item_value),
                Some(item_ty),
                false,
                body,
                loop_body,
            )?;
        }
        let saved_index_local =
            if let (Some(index_binding), Some(counter)) = (index_binding, index_counter) {
                let index_symbol = self.intern_source_name(index_binding.name.as_str());
                let index_local = body.push_local(LocalDecl {
                    name: Some(index_symbol),
                    ty: index_ty,
                    mutable: false,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let counter_value = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let index_pat = body.push_pattern(Pattern::Binding(index_local));
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::Let {
                        pat: index_pat,
                        ty: index_ty,
                        value: Some(counter_value),
                    },
                );
                let one = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(1.0)),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let next = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Add,
                        lhs: counter_value,
                        rhs: one,
                    },
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::Assign {
                        target,
                        value: next,
                    },
                );
                self.scope.bind(index_binding.name.to_string(), index_local)
            } else {
                None
            };
        self.for_each_callback_body(arrow, body, loop_body)?;
        if let Some(index_binding) = index_binding {
            if let Some(prior) = saved_index_local {
                self.scope.bind(index_binding.name.to_string(), prior);
            } else {
                self.scope.unbind(index_binding.name.as_str());
            }
        }
        if let Some(item_binding) = item_binding {
            if let Some(Some(prior)) = saved_item_local {
                self.scope.bind(item_binding.name.to_string(), prior);
            } else {
                self.scope.unbind(item_binding.name.as_str());
            }
        }
        body.push_stmt_to_block(
            block,
            Stmt::For {
                pat: item_pat,
                iter,
                body: loop_body,
            },
        );
        Ok(true)
    }

    /// Lower a `forEach` callback body, whichever form the arrow uses.
    ///
    /// Since oxc 0.147 a concise arrow (`value => total += value`) carries its
    /// body as an expression rather than a block holding one
    /// `ExpressionStatement`, so iterating block statements alone silently
    /// dropped the whole body.
    pub(in crate::lowering) fn for_each_callback_body(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if let Some(body_expression) = arrow.get_expression() {
            return self.for_each_callback_expression(body_expression, body, block);
        }
        for statement in arrow_block_statements(arrow) {
            self.for_each_callback_statement(statement, body, block)?;
        }
        Ok(())
    }

    /// Lower one expression appearing in `forEach` callback statement position.
    ///
    /// An assignment needs the callback-specific treatment (global writes,
    /// negative-index and length writes, array destructuring, and the
    /// captured-const rejection); anything else lowers as a plain expression
    /// statement.
    pub(in crate::lowering) fn for_each_callback_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let Expression::AssignmentExpression(assign) = expression else {
            let value = self.expression(expression, body)?;
            body.push_stmt_to_block(block, Stmt::Expr(value));
            return Ok(());
        };
        if let Some(set) = self.try_global_assignment_expression(assign, body)? {
            body.push_stmt_to_block(block, Stmt::Expr(set));
            return Ok(());
        }
        if self.try_lower_negative_bracket_write_statement(assign, body, block)? {
            return Ok(());
        }
        if self.try_lower_list_length_assignment_statement(assign, body, block)? {
            return Ok(());
        }
        if self.array_destructuring_assignment_statement(assign, body, block)? {
            return Ok(());
        }
        let (target, value) = self.assignment_parts(assign, body)?;
        if let Some(local_decl) = usize::try_from(target.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .and_then(|expr| match expr.kind {
                ExprKind::Local(local) => usize::try_from(local.0)
                    .ok()
                    .and_then(|index| body.locals.get(index)),
                _ => None,
            })
            && !local_decl.mutable
        {
            return Err(SmeltError::unsupported(
                self.span(assign.span.start, assign.span.end),
                "callback assignment to captured const local is not supported",
            ));
        }
        body.push_stmt_to_block(block, Stmt::Assign { target, value });
        Ok(())
    }

    /// Lower one statement of a callback body into `block`.
    ///
    /// The active statement block is swapped for `block` around the lowering
    /// and restored afterwards, so nested callbacks each append to their own
    /// block rather than to whichever one was current when lowering started.
    pub(in crate::lowering) fn for_each_callback_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        self.with_stmt_block(block, |this| {
            this.for_each_callback_statement_in_sink(statement, body, block)
        })
    }

    /// [`Self::for_each_callback_statement`] with the statement sink already
    /// pointing at `block`.
    fn for_each_callback_statement_in_sink(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match statement {
            Statement::ExpressionStatement(expr_stmt) => {
                if matches!(&expr_stmt.expression, Expression::AssignmentExpression(_)) {
                    self.for_each_callback_expression(&expr_stmt.expression, body, block)
                } else {
                    self.statement_in_block(statement, body, block)
                }
            }
            Statement::ReturnStatement(_) => {
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.for_each_callback_statement(child, body, block)?;
                }
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.expression(&if_stmt.test, body)?;
                let then_block = body.push_block(self.statement_span(&if_stmt.consequent));
                self.for_each_callback_statement(&if_stmt.consequent, body, then_block)?;
                let else_block = if_stmt
                    .alternate
                    .as_ref()
                    .map(|alternate| {
                        let else_block = body.push_block(self.statement_span(alternate));
                        self.for_each_callback_statement(alternate, body, else_block)?;
                        Ok(else_block)
                    })
                    .transpose()?;
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                Ok(())
            }
            _ => self.statement_in_block(statement, body, block),
        }
    }
}
