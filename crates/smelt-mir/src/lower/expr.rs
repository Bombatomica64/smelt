//! Expression lowering: the core [`LoweringCtx::lower_expr`] dispatcher.
//!
//! [`LoweringCtx::lower_expr`] is the heart of HIR-to-MIR lowering: a single
//! exhaustive `match` over every [`ExprKind`] variant that translates each into
//! MIR operands, statements, and (for calls/awaits) block terminators. The vast
//! majority of arms follow one shape — build an [`Rvalue`] and hand it to
//! [`LoweringCtx::assign_temp`], which allocates a temporary, emits the
//! `Assign`, and returns an [`Operand`] reading it. The remaining helpers in
//! this module resolve call targets and class methods, and materialize
//! operands into MIR locals so they can serve as place bases or mutable
//! collection receivers.

use smelt_hir::{ExprId, ExprKind, Span, Symbol, Type, TypeId};

use crate::{
    BuiltinFn, Callee, Constant, FuncId, LocalId, Operand,
    Place, Rvalue, Statement, Terminator,
};

use super::context::LoweringCtx;
use crate::AbsentSpelling;
use super::transcribe::transcribed_op_pattern;
use super::{LoweredMutationReceiver, LowerError, MutationWriteback, lower_literal};

impl LoweringCtx<'_> {
    /// Lowers a HIR expression to an operand, allocating temporaries as needed.
    pub(super) fn lower_expr(&mut self, expr_id: ExprId) -> Result<Operand, LowerError> {
        if let Some(operand) = self.exprs.get(&expr_id) {
            return Ok(operand.clone());
        }

        let expr = self.hir_expr(expr_id)?.clone();
        let operand = match &expr.kind {
            ExprKind::Literal(literal) => Operand::Const(lower_literal(literal)),
            ExprKind::Local(local) => {
                if self.volatile_locals.contains(self.body.id, *local) {
                    // Not cached: each evaluation is its own read. The snapshot has
                    // the type of what it reads, so a read at a narrowed type is
                    // no conversion and forwarding can remove it.
                    let read = self.local_read(*local, &expr)?;
                    let ty = match read.local() {
                        Some(read_local) => self.mir_local(read_local)?.ty,
                        None => expr.ty,
                    };
                    return self.assign_temp(ty, expr.span, Rvalue::Use(read));
                }
                self.local_read(*local, &expr)?
            }
            ExprKind::Call { callee, args } => {
                let callee_id = self.lower_callee(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.terminate(Terminator::Call {
                    callee: callee_id,
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.switch_to(target);
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Item(_) => {
                return Err(self.error(
                    "item expressions can only be used as callees",
                    Some(expr.span),
                ));
            }
            ExprKind::GlobalGet { item } => {
                let global = self.global_index(*item, expr.span)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::GlobalGet { global })?
            }
            ExprKind::GlobalSet { item, value } => {
                let global = self.global_index(*item, expr.span)?;
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GlobalSet {
                        global,
                        value: value_operand,
                    },
                )?
            }
            ExprKind::BinOp { op, lhs, rhs } => {
                let lhs_operand = self.lower_expr(*lhs)?;
                let rhs_operand = self.lower_expr(*rhs)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::Binary {
                        op: *op,
                        lhs: lhs_operand,
                        rhs: rhs_operand,
                    },
                )?
            }
            ExprKind::UnaryOp { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::Unary {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond_operand = self.lower_expr(*cond)?;
                let dest = self.push_temp(expr.ty, expr.span);
                let then_block = self.function.push_block(expr.span);
                let else_block = self.function.push_block(expr.span);
                self.terminate(Terminator::Switch {
                    cond: cond_operand,
                    then_block,
                    else_block,
                })?;

                self.switch_to(then_block);
                let then_operand = self.lower_expr(*then_expr)?;
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(then_operand),
                });
                // Record where each arm falls through (if it does not already
                // diverge) so the shared `join_block` can be wired only after it
                // exists.
                let then_tail = self
                    .block()?
                    .terminator
                    .is_none()
                    .then_some(self.current_block);

                self.switch_to(else_block);
                let else_operand = self.lower_expr(*else_expr)?;
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(else_operand),
                });
                let else_tail = self
                    .block()?
                    .terminator
                    .is_none()
                    .then_some(self.current_block);

                // Allocate `join_block` *after* both arms are fully lowered so
                // its block id ranks above every block either arm introduced
                // (an arm may itself be a `Conditional`, e.g. `a || (b && c)`,
                // pushing nested blocks). The Rust emitter treats a `goto` to a
                // lower-id block as a loop back-edge; giving `join_block` the
                // highest id keeps every `goto join_block` a forward edge, so a
                // continuation that follows the conditional in the same source
                // block (e.g. a `const x = a || (b && c);` initializer followed
                // by more statements inside a `switch` case) is always emitted.
                // Allocating `join_block` before lowering the else arm (as
                // before) let the else arm's nested blocks outrank it, turning
                // their `goto join_block` into a spurious back-edge that the
                // emitter dropped along with the whole post-conditional tail —
                // the es-toolkit isEqualWith `||`-in-switch-case tail loss. This
                // mirrors the join-ordering fix already applied to `lower_match`.
                let join_block = self.function.push_block(expr.span);
                if let Some(tail) = then_tail {
                    self.switch_to(tail);
                    self.terminate(Terminator::Goto(join_block))?;
                }
                if let Some(tail) = else_tail {
                    self.switch_to(tail);
                    self.terminate(Terminator::Goto(join_block))?;
                }

                self.switch_to(join_block);
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(expr.ty, expr.span, Rvalue::List(lowered_items))?
            }
            ExprKind::SetLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Set(lowered_items))?
            }
            ExprKind::DictLit(entries) => {
                let lowered_entries = entries
                    .iter()
                    .map(|(key, value)| Ok((self.lower_expr(*key)?, self.lower_expr(*value)?)))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Dict(lowered_entries))?
            }
            ExprKind::TupleLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Tuple(lowered_items))?
            }
            ExprKind::Field { receiver, field } => {
                // Inside a `try`, a read through a GETTER is a call that can
                // throw, and the throw has to reach the handler: lowering it as
                // a place read gave the `try` no throwing call at all, so its
                // `catch` was dropped as unreachable and the getter's error
                // escaped (Hono's `try { c.executionCtx } catch {}`).
                if self.current_exception_handler().is_some()
                    && let Some(getter) = self.getter_function_of_field(*receiver, *field)
                {
                    let receiver_operand = self.lower_expr(*receiver)?;
                    let dest = self.push_temp(expr.ty, expr.span);
                    let target = self.function.push_block(expr.span);
                    self.terminate(Terminator::Call {
                        callee: Callee::Static(getter),
                        args: vec![receiver_operand],
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.switch_to(target);
                    return Ok(Operand::Copy(Place::Local(dest)));
                }
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                if let Some(bound) = self.type_param_bound(receiver_ty) {
                    return self.assign_temp(
                        expr.ty,
                        expr.span,
                        Rvalue::BoundField {
                            receiver: receiver_operand,
                            bound,
                            field: *field,
                        },
                    );
                }
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Operand::Copy(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let index_operand = self.lower_expr(*index)?;
                if matches!(self.krate.types.get(expr.ty), Some(Type::Optional(_))) {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::OptionalIndex {
                            receiver: receiver_operand,
                            index: index_operand,
                        },
                    });
                    return Ok(Operand::Copy(Place::Local(dest)));
                }
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Operand::Copy(Place::Index {
                    base,
                    index: Box::new(index_operand),
                    negative: self.negative_index_policy(expr.span),
                })
            }
            ExprKind::OptionalMethod {
                receiver,
                method,
                args,
            } => {
                if let Some(result) =
                    self.lower_optional_method_with_unwind(&super::optional_call::OptionalMethodCall {
                        receiver: *receiver,
                        method: *method,
                        args,
                        result_ty: expr.ty,
                        span: expr.span,
                    })?
                {
                    self.exprs.insert(expr_id, result.clone());
                    return Ok(result);
                }
                let receiver_operand = self.lower_expr(*receiver)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::OptionalMethod {
                        receiver: receiver_operand,
                        method: *method,
                        args: lowered_args,
                    },
                )?
            }
            ExprKind::OptionalCoalesce { optional, fallback } => {
                let lowered_optional = self.lower_expr(*optional)?;
                if let Some(result) = self.lower_short_circuit_coalesce(
                    &lowered_optional,
                    *fallback,
                    expr.ty,
                    expr.span,
                )? {
                    result
                } else {
                let lowered_fallback = self.lower_expr(*fallback)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::OptionalCoalesce {
                        optional: lowered_optional,
                        fallback: lowered_fallback,
                    },
                )?
                }
            }
            ExprKind::TypeAssert { value } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Use(lowered_value))?
            }
            ExprKind::ListPush { list, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListPush {
                        list: list_operand,
                        item: item_operand,
                    },
                    writeback,
                )?
            }
            ExprKind::GeneratorYield { value } => {
                let operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GeneratorYield {
                        value: operand,
                        unwind: self.current_exception_handler(),
                        cleanup: self.current_generator_cleanup(),
                    },
                )?
            }
            ExprKind::ListExtend { list, other } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let other_operand = self.lower_expr(*other)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListExtend {
                        list: list_operand,
                        other: other_operand,
                    },
                    writeback,
                )?
            }
            ExprKind::ListInsert { list, index, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let index_operand = self.lower_expr(*index)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListInsert {
                        list: list_operand,
                        index: index_operand,
                        item: item_operand,
                    },
                    writeback,
                )?
            }
            ExprKind::ListUnshift { list, items } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operands = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListUnshift {
                        list: list_operand,
                        items: item_operands,
                    },
                    writeback,
                )?
            }
            ExprKind::ListReverse { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListReverse { list: list_operand },
                    writeback,
                )?
            }
            ExprKind::ListClear { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListClear { list: list_operand },
                    writeback,
                )?
            }
            ExprKind::ListRemove { list, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListRemove {
                        list: list_operand,
                        item: item_operand,
                    },
                    writeback,
                )?
            }
            ExprKind::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let comparator_operand = comparator
                    .map(|comparator_expr| self.lower_expr(comparator_expr))
                    .transpose()?;
                let key_operand = key.map(|key_expr| self.lower_expr(key_expr)).transpose()?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListSort {
                        list: list_operand,
                        comparator: comparator_operand,
                        key: key_operand,
                        reverse: *reverse,
                    },
                    writeback,
                )?
            }
            ExprKind::ListPop { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListPop { list: list_operand },
                    writeback,
                )?
            }
            ExprKind::ListShift { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                self.assign_temp_with_writeback(
                    expr.ty,
                    expr.span,
                    Rvalue::ListShift { list: list_operand },
                    writeback,
                )?
            }
            ExprKind::Method {
                receiver,
                method,
                args,
            } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                if let Some(bound) = self.type_param_bound(receiver_ty) {
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.lower_expr(*arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.assign_temp(
                        expr.ty,
                        expr.span,
                        Rvalue::BoundMethod {
                            receiver: receiver_operand,
                            bound,
                            method: *method,
                            args: lowered_args,
                        },
                    )?
                } else if matches!(self.krate.types.get(receiver_ty), Some(Type::Union(_))) {
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.lower_expr(*arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.assign_temp(
                        expr.ty,
                        expr.span,
                        Rvalue::UnionMethod {
                            receiver: receiver_operand,
                            method: *method,
                            args: lowered_args,
                        },
                    )?
                } else {
                let base = self.materialize_operand_local(
                    receiver_operand.clone(),
                    receiver_ty,
                    expr.span,
                )?;
                let materialized_receiver_ty = self.mir_local(base)?.ty;
                let callee_id = self.resolve_method(materialized_receiver_ty, *method, expr.span)?;
                // A getter-backed receiver is evaluated once, into `base`
                // (see `receiver_reads_through_getter`).
                let receiver_arg = if self.receiver_reads_through_getter(*receiver) {
                    Operand::Copy(Place::Local(base))
                } else {
                    receiver_operand
                };
                let mut lowered_args = vec![receiver_arg];
                lowered_args.extend(
                    args.iter()
                        .map(|arg| self.lower_expr(*arg))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.terminate(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.switch_to(target);
                Operand::Copy(Place::Local(dest))
                }
            }
            ExprKind::New { class, args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let Some(callee_id) = self.resolve_constructor(*class, expr.span)? else {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::ExternalClassInstance {
                            class: *class,
                            args: lowered_args,
                        },
                    });
                    return Ok(Operand::Copy(Place::Local(dest)));
                };
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.terminate(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.switch_to(target);
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::BaseConstructorInit {
                base,
                receiver,
                args,
            } => {
                // The receiver leads the argument list, then the base
                // constructor's own arguments; the call answers the
                // (initialised) receiver, which the derived constructor's
                // `this = super-init(this, ..)` then stores.
                let mut lowered_args = vec![self.lower_expr(*receiver)?];
                for arg in args {
                    lowered_args.push(self.lower_expr(*arg)?);
                }
                let callee_id = self.resolve_base_initializer(*base, expr.span)?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.terminate(Terminator::Call {
                    callee: Callee::BaseInit(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.switch_to(target);
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Await(future) => {
                let lowered_future = self.lower_expr(*future)?;
                let dest = self.push_temp(expr.ty, expr.span);
                if self.current_exception_handler().is_some() {
                    let target = self.function.push_block(expr.span);
                    self.terminate(Terminator::Await {
                        future: lowered_future,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.switch_to(target);
                } else {
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::Await(lowered_future),
                    });
                }
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Closure(closure) => self.lower_closure_expr(closure, expr.ty, expr.span)?,
            ExprKind::ClosureCall { callee, args } => {
                let callee_ty = self.hir_expr(*callee)?.ty;
                let callee_operand = self.lower_expr(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                // A throwing callee is routed through the terminator call path so
                // its `?`/unwind is modeled. An optional callee (`f?.(..)` typed
                // `Option<Fn>`) whose inner function throws must take the same
                // path: the emitter's `optional_indirect_call_text_for_dest`
                // short-circuits an absent callee to `None` while propagating the
                // throw of a present one.
                let inner_throws = match self.krate.types.get(callee_ty) {
                    Some(Type::Function(function)) => function.may_throw,
                    Some(Type::Optional(inner)) => matches!(
                        self.krate.types.get(*inner),
                        Some(Type::Function(function)) if function.may_throw
                    ),
                    _ => false,
                };
                // A callee whose value is a promise does not reject at the call:
                // an async function returns its future normally and the rejection
                // surfaces at the `await`, which has its own unwind edge (see the
                // `ExprKind::Await` arm). Routing it through the terminator form
                // would attach the handler to the wrong point.
                let callee_returns_future = |ty| {
                    matches!(
                        self.krate.types.get(ty),
                        Some(Type::Function(function))
                            if function.is_async
                                || matches!(
                                    self.krate.types.get(function.return_ty),
                                    Some(Type::Future(_))
                                )
                    )
                };
                let callee_is_async = match self.krate.types.get(callee_ty) {
                    Some(Type::Optional(inner)) => callee_returns_future(*inner),
                    _ => callee_returns_future(callee_ty),
                };
                // Whenever a `try` is active the call needs the unwind-carrying
                // terminator form, not only when the callee type is statically
                // known to throw. A callback parameter of unknown provenance has
                // `may_throw == false` — yet the source wrapped the call in `try`
                // precisely *because* its throw behaviour is not statically
                // known. Taking the statement form there discarded the active
                // handler outright, so `try { cb(..) } catch { .. }` could never
                // observe a callback throw.
                let needs_unwind_edge =
                    !callee_is_async && self.current_exception_handler().is_some();
                if inner_throws || needs_unwind_edge {
                    let target = self.function.push_block(expr.span);
                    self.terminate(Terminator::Call {
                        callee: Callee::Indirect(callee_operand),
                        args: lowered_args,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.switch_to(target);
                    return Ok(Operand::Copy(Place::Local(dest)));
                }
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ClosureCall {
                        callee: callee_operand,
                        args: lowered_args,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Block(block_id) => {
                // Execute the block's statements in the current control flow,
                // then yield the block's tail expression (or `None`) as the
                // block-expression value. Used by comprehension lowering.
                let tail = self.hir_block(*block_id)?.tail;
                self.lower_block_stmts(*block_id)?;
                match tail {
                    Some(tail_expr) => self.lower_expr(tail_expr)?,
                    None => Operand::Const(Constant::None),
                }
            }
            ExprKind::Lambda { .. } => {
                return Err(self.error(
                    "expression kind is not implemented in MIR yet",
                    Some(expr.span),
                ));
            }
            transcribed_op_pattern!() => self.lower_transcribed_op(&expr)?,
        };

        // A place operand is read by whatever consumes it, after every sibling.
        let operand = if matches!(operand.place(), Some(Place::Field { .. } | Place::Index { .. }))
            && self.volatile_locals.is_early_read(self.body.id, expr_id)
        {
            self.assign_temp(expr.ty, expr.span, Rvalue::Use(operand))?
        } else {
            operand
        };
        self.exprs.insert(expr_id, operand.clone());
        Ok(operand)
    }

    /// The operand reading HIR local `local` at the type of `expr`.
    fn local_read(&mut self, local: smelt_hir::LocalId, expr: &smelt_hir::Expr) -> Result<Operand, LowerError> {
        let local_id = self.locals.get(&local).copied().ok_or_else(|| {
            self.error("local expression references an unknown local", Some(expr.span))
        })?;
        let operand = Operand::Copy(Place::Local(local_id));
        let local_ty = self.mir_local(local_id)?.ty;
        if matches!(self.krate.types.get(local_ty), Some(Type::Optional(inner)) if *inner == expr.ty) {
            let dest = self.push_temp(expr.ty, expr.span);
            self.block_mut()?.statements.push(Statement::Assign {
                dest,
                value: Rvalue::Use(operand),
            });
            return Ok(Operand::Copy(Place::Local(dest)));
        }
        Ok(operand)
    }

    /// Resolve a mutable-global HIR item to its `Mir::globals` index.
    fn global_index(&self, item: smelt_hir::ItemId, span: Span) -> Result<u32, LowerError> {
        self.global_ids.get(&item).copied().ok_or_else(|| {
            self.error(
                "mutable-global reference does not resolve to a lowered global",
                Some(span),
            )
        })
    }

    /// Lowers a callee expression to a MIR callee.
    pub(super) fn lower_callee(&self, expr_id: ExprId) -> Result<Callee, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        let ExprKind::Item(item_id) = expr.kind else {
            return Err(self.error(
                "only direct item calls can lower to MIR yet",
                Some(expr.span),
            ));
        };
        let item = self.hir_item(item_id)?;
        let smelt_hir::Item::Function(function) = item else {
            return Err(self.error(
                "only function item calls can lower to MIR yet",
                Some(expr.span),
            ));
        };
        let Some(name) = self.krate.symbols.get(function.name) else {
            return Err(self.error("callee has an unknown symbol", Some(expr.span)));
        };
        if name == smelt_hir::CONSOLE_LOG_SYMBOL {
            Ok(Callee::Builtin(BuiltinFn::ConsoleLog {
                absent: AbsentSpelling::of(self.source_language(expr.span)),
            }))
        } else if name == smelt_hir::CONSOLE_WRITE_SYMBOL {
            Ok(Callee::Builtin(BuiltinFn::ConsoleWrite))
        } else if name == smelt_hir::CONSOLE_ERROR_WRITE_SYMBOL {
            Ok(Callee::Builtin(BuiltinFn::ConsoleErrorWrite))
        } else if let Some(function_id) = self.item_functions.get(&item_id).copied() {
            Ok(Callee::Static(function_id))
        } else {
            Err(self.error(
                format!("function `{name}` is not resolvable to MIR yet"),
                Some(expr.span),
            ))
        }
    }

    /// Resolves a class constructor to its function ID.
    pub(super) fn resolve_constructor(&self, class: Symbol, span: Span) -> Result<Option<FuncId>, LowerError> {
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == class
            {
                if matches!(class_item.kind, smelt_hir::ClassKind::Abstract) {
                    let name = self.krate.symbols.get(class).unwrap_or("<unknown>");
                    return Err(self.error(
                        format!("abstract class `{name}` cannot be constructed"),
                        Some(span),
                    ));
                }
                if let Some(constructor) = class_item.constructor
                    && let Some(func) = self.item_functions.get(&constructor)
                {
                    return Ok(Some(*func));
                }
            }
        }
        if self
            .krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Class { name, .. } if *name == class))
        {
            return Ok(None);
        }
        let name = self.krate.symbols.get(class).unwrap_or("<unknown>");
        Err(self.error(
            format!("class `{name}` has no resolvable constructor"),
            Some(span),
        ))
    }

    /// Returns whether reading `receiver` runs a source getter.
    ///
    /// A member read `obj.x` where `x` is a class accessor (`get x() { .. }`,
    /// recorded as a [`smelt_hir::Descriptor`] with a getter on `obj`'s class
    /// or one of its bases) is a CALL, not a storage read: the place
    /// `obj.field(x)` is emitted as `obj.__smelt_get_x()` every time it is
    /// read. JavaScript evaluates a method-call receiver exactly once, so a
    /// caller that already materialized such a receiver must pass that local
    /// instead of re-reading the place — otherwise `c.req.header(..)` runs the
    /// getter twice and any side effect in it (a counter, a lazily-built
    /// cache) is observed twice.
    /// The instance getter function behind `object.field`, when the field is
    /// a getter-backed descriptor of the object's class or one of its bases.
    fn getter_function_of_field(&self, object: ExprId, field: Symbol) -> Option<FuncId> {
        let object_expr = self.hir_expr(object).ok()?;
        let Some(Type::Class { name, .. }) = self.krate.types.get(object_expr.ty) else {
            return None;
        };
        let mut class = Some(*name);
        let mut visited = std::collections::HashSet::new();
        while let Some(class_name) = class
            && visited.insert(class_name)
        {
            let class_item = self.krate.items.iter().find_map(|item| {
                if let smelt_hir::Item::Class(class_item) = item
                    && class_item.name == class_name
                {
                    Some(class_item)
                } else {
                    None
                }
            })?;
            if let Some(getter) = class_item.descriptors.iter().find_map(|descriptor| {
                (descriptor.name == field && !descriptor.is_static)
                    .then_some(descriptor.getter)
                    .flatten()
            }) {
                return self.item_functions.get(&getter).copied();
            }
            if class_item.fields.iter().any(|item| item.name == field) {
                return None;
            }
            class = class_item.base;
        }
        None
    }

    fn receiver_reads_through_getter(&self, receiver: ExprId) -> bool {
        let Ok(receiver_expr) = self.hir_expr(receiver) else {
            return false;
        };
        let ExprKind::Field {
            receiver: object,
            field,
        } = &receiver_expr.kind
        else {
            return false;
        };
        let Ok(object_expr) = self.hir_expr(*object) else {
            return false;
        };
        let Some(Type::Class { name, .. }) = self.krate.types.get(object_expr.ty) else {
            return false;
        };
        let mut class = Some(*name);
        let mut visited = std::collections::HashSet::new();
        while let Some(class_name) = class
            && visited.insert(class_name)
        {
            let Some(class_item) = self.krate.items.iter().find_map(|item| {
                if let smelt_hir::Item::Class(class_item) = item
                    && class_item.name == class_name
                {
                    Some(class_item)
                } else {
                    None
                }
            }) else {
                return false;
            };
            if class_item.descriptors.iter().any(|descriptor| {
                descriptor.name == *field && descriptor.getter.is_some() && !descriptor.is_static
            }) {
                return true;
            }
            if class_item.fields.iter().any(|item| item.name == *field) {
                return false;
            }
            class = class_item.base;
        }
        false
    }

    /// Resolves the constructor a derived `super(..)` runs as an initializer.
    ///
    /// Unlike [`Self::resolve_constructor`], an ABSTRACT base is accepted: it is
    /// never constructed on its own here, only its initialization runs over a
    /// concrete derived instance — exactly what `super(..)` to an abstract base
    /// does in TypeScript.
    pub(super) fn resolve_base_initializer(
        &self,
        base: Symbol,
        span: Span,
    ) -> Result<FuncId, LowerError> {
        self.krate
            .items
            .iter()
            .find_map(|item| match item {
                smelt_hir::Item::Class(class_item) if class_item.name == base => class_item
                    .constructor
                    .and_then(|constructor| self.item_functions.get(&constructor).copied()),
                _ => None,
            })
            .ok_or_else(|| {
                let name = self.krate.symbols.get(base).unwrap_or("<unknown>");
                self.error(
                    format!("base class `{name}` has no constructor to run for `super(..)`"),
                    Some(span),
                )
            })
    }

    /// The class or interface bounding `receiver_ty`, when it is a type
    /// parameter in scope with such a bound (see
    /// [`LoweringCtx::type_param_bounds`]).
    ///
    /// A member access on such a receiver is typed by the bound in HIR and
    /// lowers to `Rvalue::BoundField` / `Rvalue::BoundMethod`.
    fn type_param_bound(&self, receiver_ty: TypeId) -> Option<Symbol> {
        let Some(Type::TypeParam { name }) = self.krate.types.get(receiver_ty) else {
            return None;
        };
        self.type_param_bounds.get(name).copied()
    }

    /// Resolves a method call to its function ID based on the receiver type.
    pub(super) fn resolve_method(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
        span: Span,
    ) -> Result<FuncId, LowerError> {
        let Some(Type::Class { name, .. }) = self.krate.types.get(receiver_ty) else {
            return Err(self.error("method receiver must be a class value", Some(span)));
        };
        self.resolve_method_on_class(*name, method, span)
    }

    /// Resolves a method call by walking the single-inheritance chain.
    pub(super) fn resolve_method_on_class(
        &self,
        class: Symbol,
        method: Symbol,
        span: Span,
    ) -> Result<FuncId, LowerError> {
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == class
            {
                for method_item in &class_item.methods {
                    if let smelt_hir::Item::Function(function) = self.hir_item(*method_item)?
                        && function.name == method
                        && let Some(func) = self.item_functions.get(method_item)
                    {
                        return Ok(*func);
                    }
                }
                if let Some(base) = class_item.base {
                    return self.resolve_method_on_class(base, method, span);
                }
            }
        }
        let method_name = self.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(self.error(
            format!("class method `{method_name}` is not resolvable"),
            Some(span),
        ))
    }

    /// Materializes an `Rvalue` into a fresh temporary and returns an operand reading it.
    ///
    /// This captures the shape shared by the large majority of `lower_expr` arms:
    /// allocate a temp of the expression's type, push a `Statement::Assign` that
    /// stores `value` into it, and hand back `Operand::Copy` of the temp. Each arm
    /// only has to build its `Rvalue`; the temp bookkeeping lives here.
    /// Lower `optional ?? fallback` with the fallback evaluated only when it
    /// is selected, or answer `None` to keep the single-rvalue form.
    ///
    /// JavaScript's `??` short-circuits: `res ?? this.#notFoundHandler(c)`
    /// calls the handler only for a nullish `res`. The single
    /// [`Rvalue::OptionalCoalesce`] takes both operands already lowered, so
    /// every statement of the fallback ran unconditionally — Hono's dispatch
    /// rendered a full not-found response on every request it had already
    /// answered. When the fallback can do work
    /// ([`Self::hir_expr_is_eager_safe`] is false) this lowers
    ///
    /// ```text
    /// if optional == null { dest = OptionalCoalesce(optional, fallback) }
    /// else                { dest = optional }
    /// ```
    ///
    /// with the fallback lowered inside the nullish arm only. The nullish arm
    /// keeps the original rvalue, so the value it selects is rendered exactly
    /// as before; the present arm is a plain read, which the emitter narrows
    /// (`Optional<T>` to `T`, or the erased value as is). Only those two
    /// result shapes take the branch; any other (a widened union, an element
    /// read whose absence is not an `Option`) keeps the eager form.
    fn lower_short_circuit_coalesce(
        &mut self,
        optional: &Operand,
        fallback: ExprId,
        ty: TypeId,
        span: Span,
    ) -> Result<Option<Operand>, LowerError> {
        if self.hir_expr_is_eager_safe(fallback)? {
            return Ok(None);
        }
        // Only a local is read twice (by the test and by an arm) for free.
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = optional
        else {
            return Ok(None);
        };
        let optional_ty = self.mir_local(*local)?.ty;
        let optional_shape = self.krate.types.get(optional_ty);
        let shapes_agree = if let Some(Type::Optional(inner)) = optional_shape {
            *inner == ty
        } else {
            optional_shape == Some(&Type::Unknown)
                && self.krate.types.get(ty) == Some(&Type::Unknown)
        };
        if !shapes_agree {
            return Ok(None);
        }
        // Read by both the test and an arm, so never moved.
        let read = Operand::Copy(Place::Local(*local));
        let bool_ty = self.loop_bool_ty;
        let cond = self.assign_temp(
            bool_ty,
            span,
            Rvalue::Binary {
                op: smelt_hir::BinOp::Eq,
                lhs: read.clone(),
                rhs: Operand::Const(Constant::None),
            },
        )?;
        let dest = self.push_temp(ty, span);
        let nullish_block = self.function.push_block(span);
        let present_block = self.function.push_block(span);
        self.terminate(Terminator::Switch {
            cond,
            then_block: nullish_block,
            else_block: present_block,
        })?;

        self.switch_to(nullish_block);
        let lowered_fallback = self.lower_expr(fallback)?;
        self.block_mut()?.statements.push(Statement::Assign {
            dest,
            value: Rvalue::OptionalCoalesce {
                optional: read.clone(),
                fallback: lowered_fallback,
            },
        });
        let nullish_tail = self
            .block()?
            .terminator
            .is_none()
            .then_some(self.current_block);

        self.switch_to(present_block);
        self.block_mut()?.statements.push(Statement::Assign {
            dest,
            value: Rvalue::Use(read),
        });
        let present_tail = self.current_block;

        // Allocated after both arms, as `ExprKind::Conditional` does, so every
        // `goto join_block` is a forward edge for the Rust emitter.
        let join_block = self.function.push_block(span);
        if let Some(tail) = nullish_tail {
            self.switch_to(tail);
            self.terminate(Terminator::Goto(join_block))?;
        }
        self.switch_to(present_tail);
        self.terminate(Terminator::Goto(join_block))?;
        self.switch_to(join_block);
        Ok(Some(Operand::Copy(Place::Local(dest))))
    }

    /// Whether evaluating `expr_id` eagerly is unobservable: it reads values
    /// and builds literals, and calls nothing.
    ///
    /// The whitelist is deliberately small; anything outside it (a call, a
    /// method, a construction, a block) counts as work that a short-circuiting
    /// operator must not perform when its operand is not selected.
    fn hir_expr_is_eager_safe(&self, expr_id: ExprId) -> Result<bool, LowerError> {
        let kind = self.hir_expr(expr_id)?.kind.clone();
        let leaf_or_read = matches!(
            kind,
            ExprKind::Literal(_)
                | ExprKind::Local(_)
                | ExprKind::Item(_)
                | ExprKind::GlobalGet { .. }
                | ExprKind::Closure(_)
                | ExprKind::Lambda { .. }
                | ExprKind::Field { .. }
                | ExprKind::OptionalField { .. }
                | ExprKind::Index { .. }
                | ExprKind::OptionalIndex { .. }
                | ExprKind::TypeAssert { .. }
                | ExprKind::UnknownCast { .. }
                | ExprKind::ListLit(_)
                | ExprKind::TupleLit(_)
                | ExprKind::DictLit(_)
                | ExprKind::SetLit(_)
                | ExprKind::BinOp { .. }
                | ExprKind::UnaryOp { .. }
                | ExprKind::Conditional { .. }
                | ExprKind::OptionalCoalesce { .. }
                | ExprKind::Len { .. }
        );
        if !leaf_or_read {
            return Ok(false);
        }
        let mut children = Vec::new();
        // The mapper is used only to visit the children; its rebuilt kind is
        // discarded.
        drop(kind.try_map_child_exprs(&mut |child| {
            children.push(child);
            Ok::<_, ()>(child)
        }));
        for child in children {
            if !self.hir_expr_is_eager_safe(child)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Assign `value` to a fresh temporary of type `ty` in the current block
    /// and return an operand reading it.
    pub(super) fn assign_temp(
        &mut self,
        ty: TypeId,
        span: Span,
        value: Rvalue,
    ) -> Result<Operand, LowerError> {
        let dest = self.push_temp(ty, span);
        self.block_mut()?
            .statements
            .push(Statement::Assign { dest, value });
        Ok(Operand::Copy(Place::Local(dest)))
    }

    /// Like [`Self::assign_temp`] but flushes a mutable-receiver writeback afterwards.
    ///
    /// Collection-mutation arms (list push/pop/sort, and so on) lower their receiver
    /// through [`Self::lower_mutation_receiver`], which may hand back a `writeback`
    /// that must be replayed once the mutation statement has been emitted. Keeping
    /// that ordering here lets those arms stay a single call.
    pub(super) fn assign_temp_with_writeback(
        &mut self,
        ty: TypeId,
        span: Span,
        value: Rvalue,
        writeback: MutationWriteback,
    ) -> Result<Operand, LowerError> {
        let operand = self.assign_temp(ty, span, value)?;
        self.write_back_mutation_receiver(writeback)?;
        Ok(operand)
    }

    /// Lowers a mutable collection receiver and records non-local place writeback.
    pub(super) fn lower_mutation_receiver(
        &mut self,
        expr_id: ExprId,
    ) -> Result<LoweredMutationReceiver, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        // The mutation has to reach the local, so this is no read to snapshot.
        if let ExprKind::Local(local) = expr.kind {
            return Ok((self.local_read(local, &expr)?, None));
        }
        if !matches!(
            expr.kind,
            ExprKind::Field { .. }
                | ExprKind::Index { .. }
                | ExprKind::TupleIndex { .. }
                | ExprKind::TypeAssert { .. }
                | ExprKind::UnknownCast { .. }
        ) {
            return Ok((self.lower_expr(expr_id)?, None));
        }

        let (place, writebacks) = self.lower_place(expr_id)?;
        // A bare local is mutated in place -- unless the receiver ASSERTS it
        // at another type (`callbacks.push(..)` on an `Optional<List<T>>` local
        // proved present), in which case it takes the copy-and-write-back
        // route below like any projection, so the mutation runs on a value of
        // the asserted type.
        if let Place::Local(local) = place
            && usize::try_from(local.0)
                .ok()
                .and_then(|index| self.function.locals.get(index))
                .is_none_or(|decl| decl.ty == expr.ty)
        {
            return Ok((Operand::Copy(place), None));
        }

        let local = self.push_temp(expr.ty, expr.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: local,
            value: Rvalue::Use(Operand::Copy(place.clone())),
        });
        // The receiver's OWN base may itself have been a projection copied into
        // a temporary (`a.b.list.push(x)`); those inner levels commit first, so
        // this receiver's writeback lands in a base that is still live. The
        // mutation arms replay the returned entry after the mutation statement.
        self.write_back_place_receivers(writebacks)?;
        Ok((Operand::Copy(Place::Local(local)), Some((place, local))))
    }

    /// Commits every copied projection of a place write back through its own
    /// place, INNERMOST FIRST.
    ///
    /// `PlaceWritebacks` explains why the copies exist and why they must be
    /// committed. Innermost first is what makes a nested receiver work: the
    /// entries were pushed outermost-to-innermost as the projection was walked
    /// down, and committing `a.b.c` before `a.b` would store a stale `a.b`.
    pub(super) fn write_back_place_receivers(
        &mut self,
        writebacks: super::PlaceWritebacks,
    ) -> Result<(), LowerError> {
        for (place, local) in writebacks.into_iter().rev() {
            self.block_mut()?.statements.push(Statement::AssignPlace {
                place,
                value: Rvalue::Use(Operand::Copy(Place::Local(local))),
            });
        }
        Ok(())
    }

    /// Writes a mutated temporary collection back through its original place.
    pub(super) fn write_back_mutation_receiver(
        &mut self,
        writeback: MutationWriteback,
    ) -> Result<(), LowerError> {
        if let Some((place, local)) = writeback {
            self.block_mut()?.statements.push(Statement::AssignPlace {
                place,
                value: Rvalue::Use(Operand::Copy(Place::Local(local))),
            });
        }
        Ok(())
    }

    /// Materializes an operand into a local so it can be used as a MIR place base.
    pub(super) fn materialize_operand_local(
        &mut self,
        operand: Operand,
        ty: TypeId,
        span: Span,
    ) -> Result<LocalId, LowerError> {
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand {
            return Ok(local);
        }
        let local = self.push_temp(ty, span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: local,
            value: Rvalue::Use(operand),
        });
        Ok(local)
    }
}
