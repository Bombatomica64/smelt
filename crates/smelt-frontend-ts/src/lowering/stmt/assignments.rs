//! Assignments: targets, global writes, updates, logical assignment, properties
//! assigned onto callable locals, and what an assignment does to narrowing.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use oxc::ast::ast::{AssignmentTarget, Expression, SimpleAssignmentTarget};
use oxc::span::GetSpan;
use oxc::syntax::operator::{AssignmentOperator, UpdateOperator};
use smelt_hir::{
    BinOp, Body, ErasureReason, Expr, ExprKind, Item, Literal, LocalDecl, Pattern, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Return the mutable-global item id an assignment target names, if any.
    ///
    /// A function-local or test-body binding with the same name shadows the
    /// module global, so a registered local always wins over the global path.
    fn assignment_target_mutable_global(
        &self,
        target: &AssignmentTarget<'_>,
    ) -> Option<smelt_hir::ItemId> {
        if let AssignmentTarget::StaticMemberExpression(member) = target {
            return self.static_field_write_storage(member);
        }
        let AssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return None;
        };
        if self.scope.is_bound(identifier.name.as_str()) {
            return None;
        }
        self.mutable_global_item(identifier.name.as_str())
    }

    /// Desugar an assignment to a lifted mutable global into a `GlobalSet`.
    ///
    /// Reuses [`Self::assignment_parts`] to compute the stored value (which
    /// reads the current value through `GlobalGet` for compound and logical
    /// operators), then wraps it in a `GlobalSet` that evaluates to the stored
    /// value so `x = e` / `x += e` compose as expressions. Returns `Ok(None)`
    /// when the target is not a lifted global so ordinary lowering proceeds.
    pub(in crate::lowering) fn try_global_assignment_expression(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // `globalThis.<Name> = ...` for a reassigned modeled host constructor
        // desugars to a `HostGlobalWrite` slot store; it must intercept before
        // the lifted-mutable-global path and the ordinary member-assignment path.
        if let Some(write) = self.try_host_global_write_expression(assign, body)? {
            return Ok(Some(write));
        }
        // `res.statusCode = 200` is a status-line WRITE on a modeled receiver,
        // not a field store, so it intercepts here alongside the other
        // desugared member assignments.
        if let Some(write) = self.try_server_response_assignment_expression(assign, body)? {
            return Ok(Some(write));
        }
        // `url.pathname = '/x'` is a component WRITE on a modeled receiver, for
        // the same reason.
        if let Some(write) = self.try_url_assignment_expression(assign, body)? {
            return Ok(Some(write));
        }
        let Some(item) = self.assignment_target_mutable_global(&assign.left) else {
            return Ok(None);
        };
        let (_target, value) = self.assignment_parts(assign, body)?;
        let ty = Self::expr_ty(body, value);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::GlobalSet { item, value },
            ty,
            span: self.span(assign.span.start, assign.span.end),
        })))
    }

    /// Desugar an increment/decrement of a lifted mutable global.
    ///
    /// Prefix `++x` evaluates to the stored (new) value, so the `GlobalSet`
    /// itself is returned. Postfix `x++` must evaluate to the old value while
    /// still storing the incremented one; when `keep_old_value` is set (an
    /// expression-value position) the old value is captured in a temporary
    /// before the store is emitted as a side statement. In statement/loop
    /// positions the result is discarded, so the `GlobalSet` is returned
    /// directly. Returns `Ok(None)` when the target is not a lifted global.
    pub(in crate::lowering) fn try_global_update_expression(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
        keep_old_value: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // `Class.count++` on a mutable static updates its live storage.
        let (item, name) = match &update.argument {
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                let Some(item) = self.static_field_write_storage(member) else {
                    return Ok(None);
                };
                (item, member.property.name.as_str())
            }
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                // A same-named local (function body or re-lowered test setup)
                // shadows the module global; the ordinary local update path
                // handles it.
                if self.scope.is_bound(identifier.name.as_str()) {
                    return Ok(None);
                }
                let Some(item) = self.mutable_global_item(identifier.name.as_str()) else {
                    return Ok(None);
                };
                (item, identifier.name.as_str())
            }
            _ => return Ok(None),
        };
        let span = self.span(update.span.start, update.span.end);
        let cell_ty = match self.item_ref(item) {
            Item::MutableGlobal(global_item) => global_item.ty,
            _ => self.erase_at(ErasureReason::ByName, span, name),
        };
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        // A cell that may hold `undefined` (`static n: number;`) counts in
        // `number`: `ToNumber(undefined)` is `NaN`, and the update evaluates
        // to a number either way.
        let ty = match self.ctx.krate.types.get(cell_ty) {
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::Float)) =>
            {
                *inner
            }
            _ => cell_ty,
        };
        let step = self.unit_step_literal(ty);
        let types = &self.ctx.krate.types;
        let build_set = |target_body: &mut Body| {
            let current = target_body.push_expr(Expr {
                kind: ExprKind::GlobalGet { item },
                ty: cell_ty,
                span,
            });
            let current = Self::optional_number_operand_in(types, current, target_body);
            let one = target_body.push_expr(Expr {
                kind: ExprKind::Literal(step.clone()),
                ty,
                span,
            });
            let next = target_body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: current,
                    rhs: one,
                },
                ty,
                span,
            });
            target_body.push_expr(Expr {
                kind: ExprKind::GlobalSet { item, value: next },
                ty,
                span,
            })
        };
        if update.prefix || !keep_old_value {
            let set = build_set(body);
            return Ok(Some(set));
        }
        // Postfix in an expression-value position: capture the old value in a
        // temporary, emit the store as a side statement, then evaluate to the
        // captured old value.
        let old = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item },
            ty: cell_ty,
            span,
        });
        let old = Self::optional_number_operand_in(&self.ctx.krate.types, old, body);
        let temp_local = body.push_local(LocalDecl {
            name: Some(
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new("__smelt_global_old")),
            ),
            ty,
            mutable: false,
            span,
        });
        let temp_pat = body.push_pattern(Pattern::Binding(temp_local));
        let set = build_set(body);
        self.push_stmt_to_sink(
            body,
            Stmt::Let {
                pat: temp_pat,
                ty,
                value: Some(old),
            },
        );
        self.push_stmt_to_sink(body, Stmt::Expr(set));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Local(temp_local),
            ty,
            span,
        })))
    }

    /// Extract target and value from assignment expression.
    pub(in crate::lowering) fn assignment_parts(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.assignment_target_expr(&assign.left, body)?;
        let value = self.assignment_value(assign, target, body)?;
        Ok((target, value))
    }

    /// Lower the value an assignment stores, given its already-lowered target.
    ///
    /// `target` is the READ of the assigned place: a compound operator
    /// (`x op= y`) combines it with the right-hand side, and a plain `=` only
    /// uses its type as the right-hand side's hint. Split from
    /// [`Self::assignment_parts`] so a desugared member write whose read is an
    /// operation rather than a place (`url.pathname += '/'`) shares every
    /// operator's semantics with the ordinary path.
    pub(in crate::lowering) fn assignment_value(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        target: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let target_ty = Self::expr_ty(body, target);
        let right_ty_hint = match assign.operator {
            AssignmentOperator::Assign => Some(target_ty),
            AssignmentOperator::LogicalNullish => self.non_nullish_type(target_ty),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd => Some(target_ty),
            _ => None,
        };
        let right = self.expression_with_hint(&assign.right, body, right_ty_hint)?;
        let value = match assign.operator {
            AssignmentOperator::Assign => right,
            AssignmentOperator::LogicalNullish => body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce {
                    optional: target,
                    fallback: right,
                },
                ty: self.non_nullish_type(target_ty).unwrap_or(target_ty),
                span: self.span(assign.span.start, assign.span.end),
            }),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd => {
                let span = self.span(assign.span.start, assign.span.end);
                let cond = self.lowered_condition_expression(target, span, body)?;
                let (then_expr, else_expr) = if assign.operator == AssignmentOperator::LogicalOr {
                    (target, right)
                } else {
                    (right, target)
                };
                body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty: target_ty,
                    span,
                })
            }
            // Every compound assignment whose operator has a `BinOp` is the
            // same rule: `x op= y` is `x = x op y`, and the operator's own
            // semantics — JS int32 truncation for the bitwise and shift forms,
            // string concatenation for `+=` — come from the identical `BinOp`
            // the BINARY path lowers, so there is nothing per-operator to
            // decide here. Only `%=` and the six bitwise/shift forms were
            // missing, which is why Hono's `out |= aChar ^ bChar`
            // (`utils/buffer.ts`, constant-time string compare) was a blocker
            // while `out = out | (aChar ^ bChar)` next to it was not.
            // `**=` stays out because `BinOp` has no exponentiation arm; the
            // binary `**` lowers through its own path.
            AssignmentOperator::Addition
            | AssignmentOperator::Subtraction
            | AssignmentOperator::Multiplication
            | AssignmentOperator::Division
            | AssignmentOperator::Remainder
            | AssignmentOperator::ShiftLeft
            | AssignmentOperator::ShiftRight
            | AssignmentOperator::ShiftRightZeroFill
            | AssignmentOperator::BitwiseAnd
            | AssignmentOperator::BitwiseOR
            | AssignmentOperator::BitwiseXOR => {
                let op = match assign.operator {
                    AssignmentOperator::Addition => BinOp::Add,
                    AssignmentOperator::Subtraction => BinOp::Sub,
                    AssignmentOperator::Multiplication => BinOp::Mul,
                    AssignmentOperator::Division => BinOp::Div,
                    AssignmentOperator::Remainder => BinOp::Rem,
                    AssignmentOperator::ShiftLeft => BinOp::Shl,
                    AssignmentOperator::ShiftRight => BinOp::Shr,
                    AssignmentOperator::ShiftRightZeroFill => BinOp::UShr,
                    AssignmentOperator::BitwiseAnd => BinOp::BitAnd,
                    AssignmentOperator::BitwiseOR => BinOp::BitOr,
                    AssignmentOperator::BitwiseXOR => BinOp::BitXor,
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            format!("assignment operator is not lowered yet: {other:?}"),
                        ));
                    }
                };
                let span = self.span(assign.span.start, assign.span.end);
                let (target, right) = if self.binary_operands_are_numeric(op, target, right, body) {
                    (
                        self.optional_number_operand(target, body),
                        self.optional_number_operand(right, body),
                    )
                } else {
                    (target, right)
                };
                let ty = Self::expr_ty(body, target);
                self.bigint_fallible_binary(op, target, right, span, body)
                    .unwrap_or_else(|| {
                        body.push_expr(Expr {
                            kind: ExprKind::BinOp {
                                op,
                                lhs: target,
                                rhs: right,
                            },
                            ty,
                            span,
                        })
                    })
            }
            // `**=` is the one compound operator `BinOp` has no arm for, so it
            // takes the lowering the binary `**` uses: the fallible bigint
            // power when both sides are bigints. The `number` form is still not
            // lowered here.
            AssignmentOperator::Exponential => {
                let span = self.span(assign.span.start, assign.span.end);
                let Some(power) = self.bigint_power(target, right, span, body) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "assignment operator is not lowered yet: Exponential",
                    ));
                };
                power
            }
        };
        self.apply_assignment_observed_type(&assign.left, Self::expr_ty(body, value), body);
        Ok(value)
    }

    /// Try to claim a straight-line `fn.prop = value` write onto a function-typed
    /// local for later callable-object construction.
    ///
    /// A function local carries no data fields, so a static-member write onto it
    /// only appears in the callable-object construction pattern: a local function
    /// (`debounced`) that receives `debounced.schedule = …`, `debounced.cancel =
    /// …` writes and is then returned at a callable-interface type. Instead of
    /// lowering the (fieldless) write, this collects `(property, value)` pairs in
    /// the local scope's callable-prop registry so the eventual callable-interface
    /// coercion can synthesize a typed `CallableObjectAssign` struct. The RHS is
    /// lowered into a fresh compiler local via `Stmt::Let` so its evaluation order
    /// and side effects are preserved exactly where the write appears, even though
    /// the write itself is deferred into the struct construction.
    ///
    /// Two shapes are documented punts the collection cannot claim:
    /// * a write outside the straight-line function body block (inside an `if`,
    ///   loop, or other nested block), because the field would need to become
    ///   `Optional` to model the maybe-unwritten case — this returns `Ok(false)`
    ///   and falls through to normal (pre-feature) assignment lowering, which
    ///   lowers the fieldless static-member write as a discarded statement; and
    /// * a write after the local has escaped (been read for anything other than
    ///   the consuming coercion), because the escaped view never saw the props —
    ///   this stays an `unsupported` diagnostic, guarding the construction path
    ///   from a silently mis-constructed value.
    ///
    /// Returns `Ok(true)` when the write was claimed (no statement is emitted),
    /// `Ok(false)` when normal assignment lowering should proceed.
    pub(in crate::lowering) fn try_collect_callable_local_prop(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(false);
        };
        if member.optional {
            return Ok(false);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(false);
        };
        let Some(local) = self.scope.lookup(object.name.as_str()) else {
            return Ok(false);
        };
        let Some(local_ty) = Self::local_ty_checked(body, local) else {
            return Ok(false);
        };
        if !matches!(self.ctx.krate.types.get(local_ty), Some(Type::Function(_))) {
            return Ok(false);
        }
        let span = self.span(assign.span.start, assign.span.end);
        if block != body.root {
            // Conditional / nested-block write: the feature only claims
            // straight-line writes in the body root block, because modeling a
            // maybe-unwritten field would require weakening it to `Optional`.
            // Rather than blocking the whole crate, fall through to normal
            // assignment lowering — the pre-feature behavior where a fieldless
            // static-member write is lowered as a discarded statement. This
            // matches the design's "conditionals fall through + diagnostic"
            // punt (see specs/plans/callable-object-construction.md §5) and is
            // safe: the hard construction guard below only fires once props are
            // actually consumed into a typed callable-interface struct.
            return Ok(false);
        }
        if self.scope.callable_local_escaped(local) {
            return Err(SmeltError::unsupported(
                span,
                "property writes onto a callable local after it escapes are not lowered yet",
            ));
        }
        let value = self.expression(&assign.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        let value_local = body.push_local(LocalDecl {
            name: Some(
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new("__smelt_callable_prop")),
            ),
            ty: value_ty,
            mutable: false,
            span,
        });
        let value_pat = body.push_pattern(Pattern::Binding(value_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: value_pat,
                ty: value_ty,
                value: Some(value),
            },
        );
        let value_read = body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: value_ty,
            span,
        });
        let prop = self.intern_source_name(member.property.name.as_str());
        // Last write wins, in source order: the registry method owns that rule.
        self.scope
            .record_callable_prop(local, prop, value_read, span);
        Ok(true)
    }

    /// Turn property writes this body collected but never consumed into errors.
    ///
    /// [`Self::try_collect_callable_local_prop`] removes a `fn.prop = value`
    /// statement from the stream on the promise that a later
    /// callable-interface coercion rebuilds it as a struct field. When that
    /// coercion never fires the promise is broken: the write is gone from the
    /// output and nothing says so. That silent drop is exactly how a
    /// mis-lowered static property stays invisible, so a body that finishes
    /// with pending writes reports one diagnostic per collecting local rather
    /// than emitting a value that is missing its properties.
    ///
    /// Called at the end of every body that can collect writes, with the body's
    /// own registry still installed (before `restore_callable_prop_writes`
    /// hands the enclosing body's registry back).
    pub(in crate::lowering) fn report_unconsumed_callable_props(
        &self,
        errors: &mut Vec<SmeltError>,
    ) {
        for (span, props) in self.scope.unconsumed_callable_props() {
            let names = props
                .iter()
                .map(|prop| {
                    self.ctx
                        .krate
                        .symbols
                        .get(*prop)
                        .unwrap_or("<unknown>")
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(SmeltError::unsupported(
                span,
                format!(
                    "property writes onto a callable local ({names}) were collected but never \
                     reached a callable-interface-typed position, so they would be dropped; \
                     declare the value at a named interface that carries those members"
                ),
            ));
        }
    }

    /// Synthesize a typed `CallableObjectAssign` when a callable local coerces to
    /// a callable-interface class.
    ///
    /// Consumes the property writes collected by
    /// [`Self::try_collect_callable_local_prop`] for the named function-typed
    /// local and bundles them with the base callable into an expression typed at
    /// the interface class `type_hint`. Returns `Ok(None)` (leaving the local's
    /// collected props intact) when there is no hint, the hint is not a callable
    /// interface, or the local collected no writes — so an ordinary identifier
    /// read proceeds normally.
    pub(in crate::lowering) fn try_consume_callable_local(
        &mut self,
        ident_name: &str,
        start: u32,
        end: u32,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(local) = self.scope.lookup(ident_name) else {
            return Ok(None);
        };
        // The position's own type hint decides the struct where there is one; a
        // position with no hint (a `return` from a function whose return type is
        // inferred) falls back to the callable interface the local was
        // *declared* at, which says the same thing the annotation said.
        let hint = match type_hint {
            Some(hint) => {
                if !self.type_is_callable_interface(hint) {
                    return Ok(None);
                }
                hint
            }
            None => match self.scope.callable_local_declared_interface(local) {
                Some(declared) if self.type_is_callable_interface(declared) => declared,
                _ => return Ok(None),
            },
        };
        let Some(props) = self.scope.take_callable_props(local) else {
            return Ok(None);
        };
        if props.is_empty() {
            // The local is *declared* at a callable interface but collected no
            // property writes, so there is nothing to bundle: the ordinary
            // identifier read (and whatever adapter the position applies) stays
            // in charge, exactly as before any write was collected.
            return Ok(None);
        }
        let base_ty = Self::local_ty_checked(body, local)
            .unwrap_or_else(|| self.erase_with(ErasureReason::MissingAnnotation, "callable local"));
        let callable = body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty: base_ty,
            span: self.span(start, end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::CallableObjectAssign {
                callable,
                props,
                spreads: Vec::new(),
            },
            ty: hint,
            span: self.span(start, end),
        })))
    }

    /// Return whether a type is a callable-interface class (a class whose
    /// interface declaration carries the synthetic `__smelt_call` field).
    pub(in crate::lowering) fn type_is_callable_interface(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        self.find_interface(*name).is_some_and(|interface| {
            interface
                .fields
                .iter()
                .any(|field| self.ctx.krate.symbols.get(field.name) == Some("__smelt_call"))
        })
    }

    /// Try to lower a statement-level `list.length = value` assignment.
    ///
    /// Assigning to a JavaScript array's `length` resizes it: when the new length
    /// is smaller than the current one the array is truncated to that many
    /// elements; when it is larger the array grows with empty (`undefined`) slots.
    /// The truncation case is modeled here as an in-place splice that removes
    /// every element from the new length onward (`arr.splice(new_len)`), reusing
    /// the existing list-splice machinery rather than inventing a new op.
    ///
    /// Growing a list past its current length is intentionally *not* modeled: a
    /// Smelt list is a homogeneous `Vec<T>`, so padding it with `undefined` holes
    /// has no representation for a non-optional element type. That case (rare in
    /// practice — `length` writes overwhelmingly shrink) lowers to a no-op growth,
    /// which is an explicit deferral of JavaScript's hole-padding semantics.
    ///
    /// Returns `Ok(true)` when the target matched a list `.length` write and a
    /// statement was emitted, `Ok(false)` when normal assignment lowering should
    /// proceed.
    pub(in crate::lowering) fn try_lower_list_length_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        // Only a plain `=` resize is modeled; a compound `arr.length += n` would
        // first read the current length and is left to normal lowering.
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(false);
        };
        if member.property.name != "length" {
            return Ok(false);
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(false);
        }
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let start = self.expression_with_hint(&assign.right, body, Some(float_ty))?;
        let span = self.span(assign.span.start, assign.span.end);
        // `splice(new_len)` with no delete count removes everything from the new
        // length to the end, truncating the list in place. The splice yields the
        // removed elements (typed as the list); discarded as an expression stmt.
        let splice = body.push_expr(Expr {
            kind: ExprKind::ListSplice {
                list,
                start,
                delete_count: None,
                items: Vec::new(),
                mutate: true,
            },
            ty: list_ty,
            span,
        });
        body.push_stmt_to_block(block, Stmt::Expr(splice));
        Ok(true)
    }

    /// Lower a plain array destructuring assignment statement.
    ///
    /// JavaScript evaluates the right-hand side before writing any targets, so
    /// Smelt stores that value in a compiler local and then emits one ordinary
    /// assignment per destructured element. This keeps swaps like
    /// `[data[i], data[j]] = [data[j], data[i]]` from observing their own writes.
    pub(in crate::lowering) fn array_destructuring_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::ArrayAssignmentTarget(array) = &assign.left else {
            return Ok(false);
        };
        let value = self.expression(&assign.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        let value_local = body.push_local(LocalDecl {
            name: Some(
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new("__smelt_destructure")),
            ),
            ty: value_ty,
            mutable: false,
            span: self.span(assign.right.span().start, assign.right.span().end),
        });
        let value_pat = body.push_pattern(Pattern::Binding(value_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: value_pat,
                ty: value_ty,
                value: Some(value),
            },
        );

        for (index, target) in array.elements.iter().enumerate() {
            let Some(target) = target else {
                continue;
            };
            let target = self.assignment_maybe_default_target_expr(target, body)?;
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(value_local),
                ty: value_ty,
                span: self.span(assign.right.span().start, assign.right.span().end),
            });
            let index_ty = self.ctx.krate.types.intern(Type::Float);
            let index_value = u32::try_from(index).map_or(f64::INFINITY, f64::from);
            let index_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(index_value)),
                ty: index_ty,
                span: self.span(assign.left.span().start, assign.left.span().end),
            });
            let (kind, ty) = match self.ctx.krate.types.get(value_ty) {
                Some(Type::Tuple(items)) => {
                    let Some(ty) = items.get(index).copied() else {
                        return Err(SmeltError::unsupported(
                            self.span(assign.left.span().start, assign.left.span().end),
                            "array assignment index is outside tuple type",
                        ));
                    };
                    (
                        ExprKind::TupleIndex {
                            tuple: receiver,
                            index,
                        },
                        ty,
                    )
                }
                _ => (
                    ExprKind::Index {
                        receiver,
                        index: index_expr,
                    },
                    self.index_type(value_ty)?,
                ),
            };
            let element_value = body.push_expr(Expr {
                kind,
                ty,
                span: self.span(assign.left.span().start, assign.left.span().end),
            });
            body.push_stmt_to_block(
                block,
                Stmt::Assign {
                    target,
                    value: element_value,
                },
            );
        }
        Ok(true)
    }

    /// Record the observed type produced by assigning into an unknown local.
    ///
    /// TypeScript flow analysis narrows a variable's observed type after direct
    /// assignment even when its declaration started as `unknown` in Smelt's
    /// no-`any` model. Keeping this as a narrowing fact preserves the local's
    /// declared storage type while allowing later reads in the same flow to be
    /// lowered with the assigned value type.
    pub(in crate::lowering) fn apply_assignment_observed_type(
        &mut self,
        target: &AssignmentTarget<'_>,
        observed_ty: smelt_hir::TypeId,
        body: &Body,
    ) {
        let AssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return;
        };
        let name = identifier.name.as_str();
        let Some(local) = self.scope.lookup(name) else {
            return;
        };
        let base_ty = Self::local_ty(body, local);
        // Writing through a narrowed local invalidates the prior flow fact: a new
        // value may inhabit a different union arm (or leave the narrowed subset
        // entirely), so any stale narrowing must be reconciled before later reads
        // project it into a concrete arm. When the observed value's type is a
        // member of the narrowed union (or matches it exactly) the fact is
        // refined to the observed type; otherwise it is reset to the declared
        // storage type so codegen falls back to the erased/full-union shape.
        if let Some(current) = self.narrowed_type(name)
            && current != observed_ty
        {
            if self.type_is_narrowing_of(observed_ty, current) {
                self.apply_narrowing(name.to_owned(), observed_ty);
            } else {
                self.invalidate_narrowing(name, base_ty);
            }
            return;
        }
        // Assigning a provably non-null value into an optional local narrows it
        // to the non-optional inner type for later reads in this flow. This is
        // the `x = x ?? default` / `x = x!` defaulting idiom: the declared
        // storage stays `Optional`, but subsequent reads (`let i = x; i + 1`)
        // must see the concrete inner type instead of `Option<T>`. A later
        // assignment of a possibly-null value reconciles through the
        // narrowed-type branch above, so the fact cannot outlive its flow.
        if let Some(inner) = self.optional_inner_for_narrowing(base_ty, observed_ty) {
            self.apply_narrowing(name.to_owned(), inner);
            return;
        }
        // TypeScript's assignment narrowing, for a union local: assigning a
        // value of one arm narrows later reads in this flow to that arm, so
        // `input = input.toString()` makes `input` a `string` until the next
        // write or the end of the branch, as `tsc` types it. The storage keeps
        // the declared union; a narrowed read projects the arm.
        if observed_ty != base_ty
            && matches!(self.ctx.krate.types.get(base_ty), Some(Type::Union(_)))
            && self.type_is_narrowing_of(observed_ty, base_ty)
        {
            self.apply_narrowing(name.to_owned(), observed_ty);
            return;
        }
        if self.ctx.krate.types.get(base_ty) != Some(&Type::Unknown) {
            return;
        }
        if self.ctx.krate.types.get(observed_ty) == Some(&Type::Unknown) {
            return;
        }
        // A local declared with an explicit `any` annotation stays pinned to the
        // erased `Unknown` boundary by source spelling. Narrowing it to a concrete
        // assignment's type would let later writes through the boundary (e.g.
        // `obj.b = obj` where `obj` is a self-referential `any`) demand a concrete
        // record value type that the erased shape cannot supply.
        if self.scope.is_explicit_any(local) {
            return;
        }
        self.apply_narrowing(name.to_owned(), observed_ty);
    }

    /// Return the non-optional inner type when an assignment removes optionality.
    ///
    /// Applies when the declared storage is `Optional(inner)` and the assigned
    /// value's type is provably non-null and inhabits `inner` — either an exact
    /// match, or (when `inner` is itself a union) a member of it. Returns `None`
    /// when the value could still be null/undefined (the local keeps its
    /// optional storage type) so this never hides a real optional value.
    fn optional_inner_for_narrowing(
        &self,
        base_ty: smelt_hir::TypeId,
        observed_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Some(Type::Optional(inner)) = self.ctx.krate.types.get(base_ty) else {
            return None;
        };
        let inner_ty = *inner;
        if matches!(
            self.ctx.krate.types.get(observed_ty),
            Some(Type::Optional(_) | Type::None | Type::Unknown)
        ) {
            return None;
        }
        if observed_ty == inner_ty {
            return Some(inner_ty);
        }
        if matches!(
            self.ctx.krate.types.get(inner_ty),
            Some(Type::Union(items)) if items.contains(&observed_ty)
        ) {
            return Some(observed_ty);
        }
        None
    }

    /// Return whether `candidate` is compatible with a `current` narrowed type.
    ///
    /// Assignment refinement only keeps a narrowing when the assigned value is
    /// provably still inside the narrowed set: an exact type match, or a union
    /// member of the current narrowing. Anything else is treated as escaping the
    /// narrowed subset and triggers invalidation.
    fn type_is_narrowing_of(
        &self,
        candidate: smelt_hir::TypeId,
        current: smelt_hir::TypeId,
    ) -> bool {
        if candidate == current {
            return true;
        }
        matches!(
            self.ctx.krate.types.get(current),
            Some(Type::Union(items)) if items.contains(&candidate)
        )
    }

    /// Drop the active narrowing for `name` by pinning `base_ty` in this scope.
    ///
    /// The narrowing stack is a stack of branch scopes and `narrowed_type` reads
    /// the innermost hit. Pinning the declared storage type in the *current*
    /// scope shadows any narrowing recorded here or in an enclosing scope, so
    /// later reads in this flow see the widened (erased/full-union) shape. The
    /// shadow is scoped: when the branch pops, an enclosing-scope narrowing that
    /// legitimately still holds on the other control-flow path is restored.
    fn invalidate_narrowing(&mut self, name: &str, base_ty: smelt_hir::TypeId) {
        self.apply_narrowing(name.to_owned(), base_ty);
    }

    /// Extract target and value from increment/decrement expression.
    pub(in crate::lowering) fn update_parts(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.simple_assignment_target_expr(&update.argument, body)?;
        let one_ty = Self::expr_ty(body, target);
        let one = body.push_expr(Expr {
            kind: ExprKind::Literal(self.unit_step_literal(one_ty)),
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let value = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op,
                lhs: target,
                rhs: one,
            },
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        Ok((target, value))
    }

    /// Lower prefix and postfix increment/decrement used as expression values.
    ///
    /// JavaScript returns the old value for postfix updates and the updated
    /// value for prefix updates. Variable initializers defer postfix assignments
    /// until after their binding statement so the initializer observes the old
    /// value; other expression contexts emit in their owning statement block.
    pub(in crate::lowering) fn update_expression(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A `++`/`--` of a lifted mutable global desugars to a `GlobalSet` that
        // preserves JavaScript's prefix (new value) / postfix (old value)
        // result semantics.
        if let Some(expr) = self.try_global_update_expression(update, body, true)? {
            return Ok(expr);
        }
        let (target, value) = self.update_parts(update, body)?;
        let span = self.span(update.span.start, update.span.end);

        // Postfix in a variable-initializer position defers its store until after
        // the binding statement runs, so a lazy re-read of `target` still observes
        // the pre-increment value. This is the only case that may safely return the
        // bare `target` read.
        if !update.prefix {
            match self.defer_postfix_update(Stmt::Assign { target, value }) {
                Ok(()) => return Ok(target),
                // Updates are immediate here: the same store is emitted below,
                // after the old value is snapshotted.
                Err(_immediate) => {}
            }
        }

        // Otherwise the store must run inside the current statement block, *before*
        // the enclosing expression finishes evaluating (e.g. `arr[k++] = v` or
        // `y = x++`). A lazy re-read of `target` after the store would observe the
        // already-mutated value and pick up JavaScript's wrong result, so snapshot
        // the correct result value into a temporary and evaluate to it: postfix
        // yields the pre-increment value, prefix the post-increment value.
        let result_ty = Self::expr_ty(body, target);
        // Snapshot expression captured before the store: postfix keeps the old
        // `target` read; prefix keeps the incremented `value`.
        let snapshot = if update.prefix { value } else { target };
        let temp_local = body.push_local(LocalDecl {
            name: Some(
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new("__smelt_update_tmp")),
            ),
            ty: result_ty,
            mutable: false,
            span,
        });
        let temp_pat = body.push_pattern(Pattern::Binding(temp_local));
        // For prefix the store writes the snapshotted new value (a fresh read of
        // the temp) so the increment is not recomputed; for postfix the store
        // writes `value`, which reads the still-old `target` at store time and
        // increments it.
        let store_value = if update.prefix {
            body.push_expr(Expr {
                kind: ExprKind::Local(temp_local),
                ty: result_ty,
                span,
            })
        } else {
            value
        };
        let let_stmt = Stmt::Let {
            pat: temp_pat,
            ty: result_ty,
            value: Some(snapshot),
        };
        let assign_stmt = Stmt::Assign {
            target,
            value: store_value,
        };
        let result = body.push_expr(Expr {
            kind: ExprKind::Local(temp_local),
            ty: result_ty,
            span,
        });
        Ok(self.evaluated_in_place(body, span, [let_stmt, assign_stmt], result))
    }

    /// `statements` then `value`, as one expression evaluated where it stands.
    ///
    /// A store inside an expression runs between its siblings: in `f(a, a++)` the
    /// first argument reads `a` before the store. The statement sink runs ahead of
    /// the whole enclosing statement, which would put the store before that read.
    pub(in crate::lowering) fn evaluated_in_place(
        &mut self,
        body: &mut Body,
        span: Span,
        statements: impl IntoIterator<Item = Stmt>,
        value: smelt_hir::ExprId,
    ) -> smelt_hir::ExprId {
        let block = body.push_block(span);
        self.with_stmt_block(block, |this| {
            for statement in statements {
                this.push_stmt_to_sink(body, statement);
            }
        });
        if let Some(entry) = body.block_at_mut(block) {
            entry.tail = Some(value);
        }
        let ty = Self::expr_ty(body, value);
        body.push_expr(Expr {
            kind: ExprKind::Block(block),
            ty,
            span,
        })
    }

    /// Whether an assignment operator is one of the three LOGICAL assignments,
    /// which store conditionally and therefore lower through
    /// [`Self::lower_logical_assignment`] rather than through
    /// [`Self::assignment_parts`].
    pub(in crate::lowering) const fn is_logical_assignment_operator(
        operator: AssignmentOperator,
    ) -> bool {
        matches!(
            operator,
            AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalAnd
                | AssignmentOperator::LogicalNullish
        )
    }

    /// The current-value read a logical assignment tests, with an absent
    /// record key reading as `undefined`.
    ///
    /// `SmeltRecord::get` already answers `Option<V>`; a read whose declared
    /// type is `V` is the one that appends `unwrap_or(<default>)`. So a record
    /// element read for `Record<string, number>` answered `0` for a key that
    /// was never written, and `rec[key] ??= 2` saw a non-nullish `0` and never
    /// assigned. Declaring the read `Optional(V)` is what makes absence say
    /// `undefined`, which is what JavaScript reads there.
    ///
    /// Only the read the OPERATOR tests changes; the plain read keeps its
    /// declared type, so no other expression in the crate moves.
    fn logical_assignment_current_read(
        &mut self,
        target: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let target_ty = Self::expr_ty(body, target);
        if matches!(self.ctx.krate.types.get(target_ty), Some(Type::Optional(_))) {
            return target;
        }
        let Some(target_expr) = body.expr_at(target) else {
            return target;
        };
        let span = target_expr.span;
        let ExprKind::Index { receiver, index } = target_expr.kind else {
            return target;
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        // A record key and an array index both read `undefined` when absent
        // (`queue[n] ||= []` on a sparse `T[][]`, Hono's trie-router
        // `curNodesQueue`), so the current value is the element read's
        // OPTIONAL form for either receiver. At the declared element type an
        // always-truthy element (`T[]`) folded the test to "keep" and the
        // store never ran.
        let Some(&Type::Dict(_, value_ty) | &Type::List(value_ty)) =
            self.ctx.krate.types.get(receiver_ty)
        else {
            return target;
        };
        let optional_ty = self.ctx.krate.types.intern(Type::Optional(value_ty));
        body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty: optional_ty,
            span,
        })
    }

    /// Lower `t ||= v`, `t &&= v` and `t ??= v` as the CONDITIONAL store the
    /// language specifies, in statement or expression position.
    ///
    /// `t ||= v` is `t || (t = v)`: the store runs only when the test fails, so
    /// a logical assignment to an absent record key writes the key for `||=`
    /// and `??=` and leaves it absent for `&&=`. Lowering all three as the
    /// unconditional `t = (test ? t : v)` instead made `rec[key] &&= 9` CREATE
    /// the key it was supposed to leave alone, and together with a
    /// default-valued read of an absent key it made `rec[key] ??= 2` store
    /// nothing at all.
    ///
    /// One rule for all three operators, both positions, and every target
    /// shape: locals, fields and record elements. `want_value` says whether the
    /// surrounding expression needs the assignment's value; a statement
    /// discards it, and then neither the else arm nor the result temporary is
    /// emitted.
    ///
    /// The right-hand side is lowered INSIDE the then block, so its own
    /// statements and side effects belong to the branch that stores: a logical
    /// assignment does not evaluate its right side when the test keeps the
    /// current value.
    ///
    /// The caller must have checked [`Self::is_logical_assignment_operator`].
    pub(in crate::lowering) fn lower_logical_assignment(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        target_block: Option<smelt_hir::BlockId>,
        want_value: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(assign.span.start, assign.span.end);
        let target = self.assignment_target_expr(&assign.left, body)?;
        let target_ty = Self::expr_ty(body, target);
        let current = self.logical_assignment_current_read(target, body);
        let current_ty = Self::expr_ty(body, current);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        // Whether to STORE, which is the negation of "keep the current value":
        // `||=` stores when the current value is falsy, `&&=` when it is truthy,
        // `??=` when it is `null` or `undefined` (`== null`, the one comparison
        // true for both).
        let should_store = match assign.operator {
            AssignmentOperator::LogicalNullish => {
                let none = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: current_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Eq,
                        lhs: current,
                        rhs: none,
                    },
                    ty: bool_ty,
                    span,
                })
            }
            AssignmentOperator::LogicalAnd => {
                self.lowered_condition_expression(current, span, body)?
            }
            _ => {
                let truthy = self.lowered_condition_expression(current, span, body)?;
                body.push_expr(Expr {
                    kind: ExprKind::UnaryOp {
                        op: smelt_hir::UnaryOp::Not,
                        operand: truthy,
                    },
                    ty: bool_ty,
                    span,
                })
            }
        };
        let right_hint = match assign.operator {
            AssignmentOperator::LogicalNullish => self.non_nullish_type(target_ty),
            _ => Some(target_ty),
        };
        // The VALUE of `x ??= v` is never nullish: either the store happened and
        // the value is `v`, or it did not and `x` was already non-nullish. So
        // the expression's type is `NonNullable<typeof x>` joined with
        // `typeof v`, which is what TypeScript gives it — not the target's own
        // optional surface. Keeping the optional there made
        // `cond ? headers : (maybeHeaders ??= new Headers())` a join of
        // `Headers` and `Headers | undefined`, which the ternary's type chain
        // then had to reconcile; with the nullish arm removed the two branches
        // are simply the same type. `||=` and `&&=` keep the target type,
        // because their value CAN be the falsy/short-circuited original.
        let result_ty = if matches!(assign.operator, AssignmentOperator::LogicalNullish) {
            self.non_nullish_type(target_ty).unwrap_or(target_ty)
        } else {
            target_ty
        };
        // The result temporary is declared before the branch and assigned in
        // both arms, which is the shape the emitter already renders for a
        // conditionally produced value (`let mut _smelt_tmp_N: T;`).
        let result_local = want_value.then(|| {
            body.push_local(LocalDecl {
                name: Some(self.ctx.krate.symbols.intern_synthesized(
                    &smelt_hir::SynthesizedName::new(format!(
                        "__smelt_logical_{}",
                        body.locals.len()
                    )),
                )),
                ty: result_ty,
                mutable: true,
                span,
            })
        });
        let then_block = body.push_block(span);
        let right = self.with_stmt_block(then_block, |this| {
            this.expression_with_hint(&assign.right, body, right_hint)
        })?;
        let right_ty = Self::expr_ty(body, right);
        // The stored value is bound once: the right side may construct, and
        // writing it to both the target and the result temporary would build it
        // twice.
        let right_local = body.push_local(LocalDecl {
            name: Some(self.ctx.krate.symbols.intern_synthesized(
                &smelt_hir::SynthesizedName::new(format!(
                    "__smelt_logical_value_{}",
                    body.locals.len()
                )),
            )),
            ty: right_ty,
            mutable: false,
            span,
        });
        let right_pat = body.push_pattern(Pattern::Binding(right_local));
        body.push_stmt_to_block(
            then_block,
            Stmt::Let {
                pat: right_pat,
                ty: right_ty,
                value: Some(right),
            },
        );
        let stored = body.push_expr(Expr {
            kind: ExprKind::Local(right_local),
            ty: right_ty,
            span,
        });
        body.push_stmt_to_block(
            then_block,
            Stmt::Assign {
                target,
                value: stored,
            },
        );
        let else_block = result_local.map(|result_local| {
            let stored_result = body.push_expr(Expr {
                kind: ExprKind::Local(result_local),
                ty: target_ty,
                span,
            });
            let stored_again = body.push_expr(Expr {
                kind: ExprKind::Local(right_local),
                ty: right_ty,
                span,
            });
            body.push_stmt_to_block(
                then_block,
                Stmt::Assign {
                    target: stored_result,
                    value: stored_again,
                },
            );
            // The kept value is the target's own declared-type read: this arm
            // runs only when the test kept the current value, so it is there.
            let else_block = body.push_block(span);
            let kept_result = body.push_expr(Expr {
                kind: ExprKind::Local(result_local),
                ty: target_ty,
                span,
            });
            body.push_stmt_to_block(
                else_block,
                Stmt::Assign {
                    target: kept_result,
                    value: target,
                },
            );
            else_block
        });
        if let Some(result_local) = result_local {
            let pat = body.push_pattern(Pattern::Binding(result_local));
            let declare = Stmt::Let {
                pat,
                ty: target_ty,
                value: None,
            };
            self.push_logical_assignment_stmt(body, target_block, declare);
        }
        let branch = Stmt::If {
            cond: should_store,
            then_block,
            else_block,
        };
        self.push_logical_assignment_stmt(body, target_block, branch);
        Ok(result_local.map(|result_local| {
            body.push_expr(Expr {
                kind: ExprKind::Local(result_local),
                ty: result_ty,
                span,
            })
        }))
    }

    /// Push one statement of a lowered logical assignment into the block that
    /// owns it: the caller's block when it named one, otherwise the block the
    /// surrounding expression is being lowered into.
    fn push_logical_assignment_stmt(
        &self,
        body: &mut Body,
        target_block: Option<smelt_hir::BlockId>,
        stmt: Stmt,
    ) {
        let block = target_block.unwrap_or_else(|| self.stmt_sink_block(body));
        body.push_stmt_to_block(block, stmt);
    }

    /// Lower an assignment used as an expression VALUE, store included.
    ///
    /// JavaScript's assignment is an expression whose value is the assigned
    /// value and whose effect is the store - `const child = (rec[key] ||= new
    /// Node())` both writes the record and evaluates to the child. Both halves
    /// matter in expression position, for every operator and every target.
    ///
    /// The store must run inside the current statement block, before the
    /// enclosing expression finishes evaluating, and the result must be a value
    /// snapshotted BEFORE the store: a lazy re-read of the target after the
    /// store would evaluate the target's own subexpressions a second time. This
    /// is the same shape [`Self::update_expression`] uses for `x++`, which is
    /// where the correct handling already lived.
    pub(in crate::lowering) fn assignment_expression_value(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `||=`, `&&=` and `??=` store CONDITIONALLY, so they have their own
        // rule; every other operator stores unconditionally and evaluates to
        // the stored value.
        if Self::is_logical_assignment_operator(assign.operator)
            && let Some(value) = self.lower_logical_assignment(assign, body, None, true)?
        {
            return Ok(value);
        }
        let (target, value) = self.assignment_parts(assign, body)?;
        // The stored value expression is what the surrounding expression
        // evaluates to. One expression id, referenced by the store and by the
        // result: MIR lowers an expression once into its own temporary, so this
        // neither re-evaluates the right-hand side nor needs a temporary of its
        // own here.
        let span = self.span(assign.span.start, assign.span.end);
        let assign_stmt = Stmt::Assign { target, value };
        Ok(self.evaluated_in_place(body, span, [assign_stmt], value))
    }

    /// Convert assignment target to expression.
    pub(in crate::lowering) fn assignment_target_expr(
        &mut self,
        target: &AssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            AssignmentTarget::StaticMemberExpression(member) => {
                self.static_member_assignment_target(member, body)
            }
            AssignmentTarget::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let Some(Some(first)) = array.elements.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array assignment targets require at least one element",
                    ));
                };
                self.assignment_maybe_default_target_expr(first, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "assignment target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert an array destructuring assignment element to its target expression.
    pub(in crate::lowering) fn assignment_maybe_default_target_expr(
        &mut self,
        target: &oxc::ast::ast::AssignmentTargetMaybeDefault<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            oxc::ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                self.assignment_target_expr(&default.binding, body)
            }
            _ => self.assignment_target_expr(target.to_assignment_target(), body),
        }
    }

    /// Convert simple assignment target to expression.
    pub(in crate::lowering) fn simple_assignment_target_expr(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => self
                .identifier_expression(ident.name.as_str(), ident.span.start, ident.span.end, body),
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.static_member_assignment_target(member, body)
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "update target must be a local, field, or index expression",
            )),
        }
    }

    /// Lower access to a private class field through the same field HIR as public fields.
    pub(in crate::lowering) fn private_field_member(
        &mut self,
        object: &Expression<'_>,
        field_name: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver = self.expression(object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        // A private name is not the property of the same spelling; see
        // `intern_private_name`.
        let field = self.intern_private_name(field_name);
        let ty = self.class_field_type(receiver_ty, field)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(span.start, span.end),
        }))
    }
}
