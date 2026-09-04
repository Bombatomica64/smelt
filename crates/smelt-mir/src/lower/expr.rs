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
    BuiltinFn, Callee, Constant, FuncId, LocalId, MirListSpliceItem, Operand,
    Place, Rvalue, Statement, Terminator,
};

use super::context::LoweringCtx;
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
                let local_id = self.locals.get(local).copied().ok_or_else(|| {
                    self.error(
                        "local expression references an unknown local",
                        Some(expr.span),
                    )
                })?;
                let operand = Operand::Copy(Place::Local(local_id));
                let local_ty = self.mir_local(local_id)?.ty;
                if matches!(self.krate.types.get(local_ty), Some(Type::Optional(inner)) if *inner == expr.ty)
                {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::Use(operand),
                    });
                    Operand::Copy(Place::Local(dest))
                } else {
                    operand
                }
            }
            ExprKind::Call { callee, args } => {
                let callee_id = self.lower_callee(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: callee_id,
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
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
                self.set_terminator(Terminator::Switch {
                    cond: cond_operand,
                    then_block,
                    else_block,
                })?;

                self.current_block = then_block;
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

                self.current_block = else_block;
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
                    self.current_block = tail;
                    self.set_terminator(Terminator::Goto(join_block))?;
                }
                if let Some(tail) = else_tail {
                    self.current_block = tail;
                    self.set_terminator(Terminator::Goto(join_block))?;
                }

                self.current_block = join_block;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::FunctionTableLookup { key, cases } => {
                let lowered_key = self.lower_expr(*key)?;
                let lowered_cases = cases
                    .iter()
                    .map(|(case_key, case_expr)| {
                        self.lower_expr(*case_expr)
                            .map(|operand| (case_key.clone(), operand))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::FunctionTableLookup {
                        key: lowered_key,
                        cases: lowered_cases,
                    },
                )?
            }
            ExprKind::InstanceOf { value, class } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::InstanceOf {
                        value: lowered_value,
                        class: *class,
                    },
                )?
            }
            ExprKind::InstanceOfValue { value, target } => {
                let lowered_value = self.lower_expr(*value)?;
                let lowered_target = self.lower_expr(*target)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::InstanceOfValue {
                        value: lowered_value,
                        target: lowered_target,
                    },
                )?
            }
            ExprKind::Construct { callee, args } => {
                let callee_operand = self.lower_expr(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::Construct {
                        callee: callee_operand,
                        args: lowered_args,
                    },
                )?
            }
            ExprKind::UnknownIs { value, kind } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::UnknownIs {
                        value: lowered_value,
                        kind: *kind,
                    },
                )?
            }
            ExprKind::TypeofValue { value } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TypeofValue {
                        value: lowered_value,
                    },
                )?
            }
            ExprKind::PrototypeSentinel {
                value,
                own_slot_shadows,
            } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::PrototypeSentinel {
                        value: lowered_value,
                        own_slot_shadows: *own_slot_shadows,
                    },
                )?
            }
            ExprKind::BoxPrimitive { value } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::BoxPrimitive {
                        value: lowered_value,
                    },
                )?
            }
            ExprKind::DefineProperties {
                target,
                descriptors,
            } => {
                let lowered_target = self.lower_expr(*target)?;
                let lowered_descriptors = self.lower_expr(*descriptors)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DefineProperties {
                        target: lowered_target,
                        descriptors: lowered_descriptors,
                    },
                )?
            }
            ExprKind::ObjectFromPrototype { prototype } => {
                let lowered_prototype = self.lower_expr(*prototype)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ObjectFromPrototype {
                        prototype: lowered_prototype,
                    },
                )?
            }
            ExprKind::UnknownCast { value, target } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::UnknownCast {
                        value: lowered_value,
                        target: *target,
                    },
                )?
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
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Operand::Copy(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::OptionalField { receiver, field } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::OptionalField {
                        receiver: receiver_operand,
                        field: *field,
                    },
                )?
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
            ExprKind::OptionalIndex { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let index_operand = self.lower_expr(*index)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::OptionalIndex {
                        receiver: receiver_operand,
                        index: index_operand,
                    },
                )?
            }
            ExprKind::OptionalMethod {
                receiver,
                method,
                args,
            } => {
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
            ExprKind::TypeAssert { value } => {
                let lowered_value = self.lower_expr(*value)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Use(lowered_value))?
            }
            ExprKind::Len { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::Len(lowered_operand))?
            }
            ExprKind::NumericAbs { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::NumericAbs(lowered_operand))?
            }
            ExprKind::NumericRound { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericRound {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::NumericExtrema { op, args, spread } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let lowered_spread = spread.map(|s| self.lower_expr(s)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericExtrema {
                        op: *op,
                        args: lowered_args,
                        spread: lowered_spread,
                    },
                )?
            }
            ExprKind::NumericHypot { args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(expr.ty, expr.span, Rvalue::NumericHypot { args: lowered_args })?
            }
            ExprKind::NumericPredicate { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericPredicate {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::NumericUnaryFunc { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericUnaryFunc {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::NumericPow { base, exponent } => {
                let base_operand = self.lower_expr(*base)?;
                let exponent_operand = self.lower_expr(*exponent)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericPow {
                        base: base_operand,
                        exponent: exponent_operand,
                    },
                )?
            }
            ExprKind::NumericAtan2 { y, x } => {
                let y_operand = self.lower_expr(*y)?;
                let x_operand = self.lower_expr(*x)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericAtan2 {
                        y: y_operand,
                        x: x_operand,
                    },
                )?
            }
            ExprKind::NumericRandom => {
                self.assign_temp(expr.ty, expr.span, Rvalue::NumericRandom)?
            }
            ExprKind::NumericRandomInt { start, end } => {
                let start_operand = self.lower_expr(*start)?;
                let end_operand = self.lower_expr(*end)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericRandomInt {
                        start: start_operand,
                        end: end_operand,
                    },
                )?
            }
            ExprKind::NumericToStringRadix { operand, radix } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let lowered_radix = self.lower_expr(*radix)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericToStringRadix {
                        operand: lowered_operand,
                        radix: lowered_radix,
                    },
                )?
            }
            ExprKind::NumericToFixed { operand, digits } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let lowered_digits = self.lower_expr(*digits)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::NumericToFixed {
                        operand: lowered_operand,
                        digits: lowered_digits,
                    },
                )?
            }
            ExprKind::ParseIntRadix { operand, radix } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let lowered_radix = self.lower_expr(*radix)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ParseIntRadix {
                        operand: lowered_operand,
                        radix: lowered_radix,
                    },
                )?
            }
            ExprKind::PrimitiveCast { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::PrimitiveCast {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StringCase { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringCase {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StringNormalize { form, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringNormalize {
                        form: *form,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StringLocaleCompare { left, right } => {
                let lowered_left = self.lower_expr(*left)?;
                let lowered_right = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringLocaleCompare {
                        left: lowered_left,
                        right: lowered_right,
                    },
                )?
            }
            ExprKind::UriEncode { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::UriEncode {
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::ObjectToStringTag { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ObjectToStringTag {
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StructuredClone { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StructuredClone {
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StringTrim { side, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringTrim {
                        side: *side,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::StringAffix {
                op,
                haystack,
                needle,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringAffix {
                        op: *op,
                        haystack: haystack_operand,
                        needle: needle_operand,
                    },
                )?
            }
            ExprKind::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let from_index_operand =
                    from_index.map(|value| self.lower_expr(value)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringSearch {
                        op: *op,
                        haystack: haystack_operand,
                        needle: needle_operand,
                        from_index: from_index_operand,
                    },
                )?
            }
            ExprKind::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let pattern_operand = self.lower_expr(*pattern)?;
                let replacement_operand = self.lower_expr(*replacement)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringReplace {
                        op: *op,
                        haystack: haystack_operand,
                        pattern: pattern_operand,
                        replacement: replacement_operand,
                    },
                )?
            }
            ExprKind::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let affix_operand = self.lower_expr(*affix)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringRemoveAffix {
                        op: *op,
                        haystack: haystack_operand,
                        affix: affix_operand,
                    },
                )?
            }
            ExprKind::StringRepeat { operand, count } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let count_operand = self.lower_expr(*count)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringRepeat {
                        operand: lowered_operand,
                        count: count_operand,
                    },
                )?
            }
            ExprKind::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let target_len_operand = self.lower_expr(*target_len)?;
                let pad_operand = self.lower_expr(*pad)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringPad {
                        op: *op,
                        operand: lowered_operand,
                        target_len: target_len_operand,
                        pad: pad_operand,
                    },
                )?
            }
            ExprKind::StringPredicate { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringPredicate {
                        op: *op,
                        operand: lowered_operand,
                    },
                )?
            }
            ExprKind::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexIsMatch {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let replacement_operand = self.lower_expr(*replacement)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexReplace {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                        replacement: replacement_operand,
                    },
                )?
            }
            ExprKind::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let callback_operand = self.lower_expr(*callback)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexReplaceCallback {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                        callback: callback_operand,
                    },
                )?
            }
            ExprKind::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexReplaceFirstMatchUppercase {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::RegexSplit { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexSplit {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::RegexFind { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexFind {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::RegexExec { regex, haystack } => {
                let regex_operand = self.lower_expr(*regex)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexExec {
                        regex: regex_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::RegexMatchAll { regex, haystack } => {
                let regex_operand = self.lower_expr(*regex)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::RegexMatchAll {
                        regex: regex_operand,
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::StringCharAt { operand, index } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let index_operand = self.lower_expr(*index)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringCharAt {
                        operand: lowered_operand,
                        index: index_operand,
                    },
                )?
            }
            ExprKind::StringCharCodeAt { operand, index } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let index_operand = self.lower_expr(*index)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringCharCodeAt {
                        operand: lowered_operand,
                        index: index_operand,
                    },
                )?
            }
            ExprKind::StringContains {
                haystack,
                needle,
                from_index,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let from_index_operand =
                    from_index.map(|value| self.lower_expr(value)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringContains {
                        haystack: haystack_operand,
                        needle: needle_operand,
                        from_index: from_index_operand,
                    },
                )?
            }
            ExprKind::StringSlice {
                operand,
                start,
                end,
            } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringSlice {
                        operand: lowered_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                )?
            }
            ExprKind::ListContains { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListContains {
                        list: list_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::SetContains { set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetContains {
                        set: set_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::SetDisjoint { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetDisjoint {
                        left: left_operand,
                        right: right_operand,
                    },
                )?
            }
            ExprKind::SetRelation { op, left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetRelation {
                        op: *op,
                        left: left_operand,
                        right: right_operand,
                    },
                )?
            }
            ExprKind::SetAdd { set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetAdd {
                        set: set_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::SetRemove { op, set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetRemove {
                        op: *op,
                        set: set_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::SetClear { set } => {
                let set_operand = self.lower_expr(*set)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::SetClear { set: set_operand })?
            }
            ExprKind::SetCopy { set } => {
                let set_operand = self.lower_expr(*set)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::SetCopy { set: set_operand })?
            }
            ExprKind::ListToSet { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListToSet { list: list_operand })?
            }
            ExprKind::ListPairsToDict { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListPairsToDict { list: list_operand })?
            }
            ExprKind::SetBinary { op, left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetBinary {
                        op: *op,
                        left: left_operand,
                        right: right_operand,
                    },
                )?
            }
            ExprKind::SetProjection { op, set } => {
                let set_operand = self.lower_expr(*set)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::SetProjection {
                        op: *op,
                        set: set_operand,
                    },
                )?
            }
            ExprKind::ListConcat { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListConcat {
                        left: left_operand,
                        right: right_operand,
                    },
                )?
            }
            ExprKind::ConcatSpread { value } => {
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ConcatSpread {
                        value: value_operand,
                    },
                )?
            }
            ExprKind::ListSearch {
                op,
                list,
                item,
                from_index,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let from_index_operand =
                    from_index.map(|value| self.lower_expr(value)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListSearch {
                        op: *op,
                        list: list_operand,
                        item: item_operand,
                        from_index: from_index_operand,
                    },
                )?
            }
            ExprKind::ListCallback { op, list, callback } => {
                let list_operand = self.lower_expr(*list)?;
                let callback_operand = self.lower_expr(*callback)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListCallback {
                        op: *op,
                        list: list_operand,
                        callback: callback_operand,
                    },
                )?
            }
            ExprKind::ListFromLength { length } => {
                let length_operand = self.lower_expr(*length)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListFromLength {
                        length: length_operand,
                    },
                )?
            }
            ExprKind::ListRepeat { value, count } => {
                let value_operand = self.lower_expr(*value)?;
                let count_operand = self.lower_expr(*count)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListRepeat {
                        value: value_operand,
                        count: count_operand,
                    },
                )?
            }
            ExprKind::ListFromLengthMap { length, callback } => {
                let length_operand = self.lower_expr(*length)?;
                let callback_operand = self.lower_expr(*callback)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListFromLengthMap {
                        length: length_operand,
                        callback: callback_operand,
                    },
                )?
            }
            ExprKind::ListReduce {
                list,
                initial,
                callback,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let initial_operand = initial
                    .map(|initial_expr| self.lower_expr(initial_expr))
                    .transpose()?;
                let callback_operand = self.lower_expr(*callback)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListReduce {
                        list: list_operand,
                        initial: initial_operand,
                        callback: callback_operand,
                    },
                )?
            }
            ExprKind::ListSlice { list, start, end } => {
                let list_operand = self.lower_expr(*list)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListSlice {
                        list: list_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                )?
            }
            ExprKind::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let start_operand = self.lower_expr(*start)?;
                let delete_count_operand = delete_count
                    .map(|count| self.lower_expr(count))
                    .transpose()?;
                let item_operands = items
                    .iter()
                    .map(|item| {
                        Ok(MirListSpliceItem {
                            value: self.lower_expr(item.value)?,
                            spread: item.spread,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListSplice {
                        list: list_operand,
                        start: start_operand,
                        delete_count: delete_count_operand,
                        items: item_operands,
                        mutate: *mutate,
                    },
                )?
            }
            ExprKind::ListFill {
                list,
                value,
                start,
                end,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let value_operand = self.lower_expr(*value)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListFill {
                        list: list_operand,
                        value: value_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                )?
            }
            ExprKind::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let target_operand = self.lower_expr(*target)?;
                let start_operand = self.lower_expr(*start)?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListCopyWithin {
                        list: list_operand,
                        target: target_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                )?
            }
            ExprKind::ListWith { list, index, value } => {
                let list_operand = self.lower_expr(*list)?;
                let index_operand = self.lower_expr(*index)?;
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListWith {
                        list: list_operand,
                        index: index_operand,
                        value: value_operand,
                    },
                )?
            }
            ExprKind::ListFlat { list, depth } => {
                let list_operand = self.lower_expr(*list)?;
                let depth_operand = depth
                    .map(|depth_expr| self.lower_expr(depth_expr))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListFlat {
                        list: list_operand,
                        depth: depth_operand,
                    },
                )?
            }
            ExprKind::ListProjection { op, list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListProjection {
                        op: *op,
                        list: list_operand,
                    },
                )?
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
                        cleanup: self.generator_cleanups.last().copied(),
                    },
                )?
            }
            ExprKind::GeneratorNext {
                generator,
                value,
                kind,
            } => {
                let operand = self.lower_expr(*generator)?;
                let value = value.map(|value| self.lower_expr(value)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GeneratorNext {
                        generator: operand,
                        value,
                        kind: match kind {
                            smelt_hir::GeneratorResumeKind::Next => {
                                crate::GeneratorResumeKind::Next
                            }
                            smelt_hir::GeneratorResumeKind::Return => {
                                crate::GeneratorResumeKind::Return
                            }
                            smelt_hir::GeneratorResumeKind::Throw => {
                                crate::GeneratorResumeKind::Throw
                            }
                        },
                    },
                )?
            }
            ExprKind::GeneratorDone { result } => {
                let operand = self.lower_expr(*result)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GeneratorDone { result: operand },
                )?
            }
            ExprKind::GeneratorValue { result } => {
                let operand = self.lower_expr(*result)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GeneratorValue { result: operand },
                )?
            }
            ExprKind::GeneratorDelegate { generator } => {
                let operand = self.lower_expr(*generator)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::GeneratorDelegate { generator: operand },
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
            ExprKind::ListCopy { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListCopy { list: list_operand })?
            }
            ExprKind::TupleToList { tuple } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TupleToList {
                        tuple: tuple_operand,
                    },
                )?
            }
            ExprKind::ListToTuple { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListToTuple { list: list_operand })?
            }
            ExprKind::TupleToSet { tuple } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TupleToSet {
                        tuple: tuple_operand,
                    },
                )?
            }
            ExprKind::ListCount { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListCount {
                        list: list_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::ListSum { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListSum { list: list_operand })?
            }
            ExprKind::ListBoolFold { op, list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListBoolFold {
                        op: *op,
                        list: list_operand,
                    },
                )?
            }
            ExprKind::ListSorted { list, key, reverse } => {
                let list_operand = self.lower_expr(*list)?;
                let key_operand = key.map(|key_expr| self.lower_expr(key_expr)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListSorted {
                        list: list_operand,
                        key: key_operand,
                        reverse: *reverse,
                    },
                )?
            }
            ExprKind::ListReversed { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListReversed { list: list_operand })?
            }
            ExprKind::ListEnumerate { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListEnumerate { list: list_operand })?
            }
            ExprKind::ListZip { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListZip {
                        left: left_operand,
                        right: right_operand,
                    },
                )?
            }
            ExprKind::ListRange { start, end, step } => {
                let start_operand = self.lower_expr(*start)?;
                let end_operand = self.lower_expr(*end)?;
                let step_operand = self.lower_expr(*step)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListRange {
                        start: start_operand,
                        end: end_operand,
                        step: step_operand,
                    },
                )?
            }
            ExprKind::ListRandomChoice { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListRandomChoice { list: list_operand })?
            }
            ExprKind::ListIndex { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ListIndex {
                        list: list_operand,
                        item: item_operand,
                    },
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
            ExprKind::ListNext { list } => {
                let list_operand = self.lower_expr(*list)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::ListNext { list: list_operand })?
            }
            ExprKind::IteratorDone { result } => {
                let result_operand = self.lower_expr(*result)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::IteratorDone {
                        result: result_operand,
                    },
                )?
            }
            ExprKind::IteratorValue { result } => {
                let result_operand = self.lower_expr(*result)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::IteratorValue {
                        result: result_operand,
                    },
                )?
            }
            ExprKind::TupleContains { tuple, item } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let item_operand = self.lower_expr(*item)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TupleContains {
                        tuple: tuple_operand,
                        item: item_operand,
                    },
                )?
            }
            ExprKind::TupleIndex { tuple, index } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TupleIndex {
                        tuple: tuple_operand,
                        index: *index,
                    },
                )?
            }
            ExprKind::TupleSlice { tuple, start, end } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::TupleSlice {
                        tuple: tuple_operand,
                        start: *start,
                        end: *end,
                    },
                )?
            }
            ExprKind::DictContainsKey { dict, key } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictContainsKey {
                        dict: dict_operand,
                        key: key_operand,
                    },
                )?
            }
            ExprKind::DictSet { dict, key, value } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictSet {
                        dict: dict_operand,
                        key: key_operand,
                        value: value_operand,
                    },
                )?
            }
            ExprKind::DictRemoveKey { dict, key } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictRemoveKey {
                        dict: dict_operand,
                        key: key_operand,
                    },
                )?
            }
            ExprKind::DictGet { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = default
                    .map(|default_expr| self.lower_expr(default_expr))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictGet {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                )?
            }
            ExprKind::DictSetDefault { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = self.lower_expr(*default)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictSetDefault {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                )?
            }
            ExprKind::DictClear { dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::DictClear { dict: dict_operand })?
            }
            ExprKind::DictPop { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = default
                    .as_ref()
                    .map(|default_expr| self.lower_expr(*default_expr))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictPop {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                )?
            }
            ExprKind::DictUpdate { dict, other } => {
                let dict_operand = self.lower_expr(*dict)?;
                let other_operand = self.lower_expr(*other)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictUpdate {
                        dict: dict_operand,
                        other: other_operand,
                    },
                )?
            }
            ExprKind::DictAssign { target, sources } => {
                let target_operand = self.lower_expr(*target)?;
                let source_operands = sources
                    .iter()
                    .map(|source| self.lower_expr(*source))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictAssign {
                        target: target_operand,
                        sources: source_operands,
                    },
                )?
            }
            ExprKind::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => {
                let callable_operand = self.lower_expr(*callable)?;
                let prop_operands = props
                    .iter()
                    .map(|(name, value)| Ok((*name, self.lower_expr(*value)?)))
                    .collect::<Result<Vec<_>, LowerError>>()?;
                let spread_operands = spreads
                    .iter()
                    .map(|value| self.lower_expr(*value))
                    .collect::<Result<Vec<_>, LowerError>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::CallableObjectAssign {
                        callable: callable_operand,
                        props: prop_operands,
                        spreads: spread_operands,
                    },
                )?
            }
            ExprKind::DictCopy { dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::DictCopy { dict: dict_operand })?
            }
            ExprKind::DictProjection { op, dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DictProjection {
                        op: *op,
                        dict: dict_operand,
                    },
                )?
            }
            ExprKind::StringSplit {
                haystack,
                separator,
                limit,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let separator_operand = self.lower_expr(*separator)?;
                let limit_operand = limit
                    .map(|limit_expr| self.lower_expr(limit_expr))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringSplit {
                        haystack: haystack_operand,
                        separator: separator_operand,
                        limit: limit_operand,
                    },
                )?
            }
            ExprKind::StringChars { haystack } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringChars {
                        haystack: haystack_operand,
                    },
                )?
            }
            ExprKind::StringJoin { items, separator } => {
                let items_operand = self.lower_expr(*items)?;
                let separator_operand = self.lower_expr(*separator)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::StringJoin {
                        items: items_operand,
                        separator: separator_operand,
                    },
                )?
            }
            ExprKind::JsonStringify { value } => {
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::JsonStringify {
                        value: value_operand,
                    },
                )?
            }
            ExprKind::JsonParse { text } => {
                // `JSON.parse` is fallible: malformed text throws a catchable
                // `SyntaxError`. A `Statement::Assign` has no unwind edge, so an
                // rvalue form can never reach an enclosing `try` -- the catch
                // block would have no predecessor and be dropped, turning a
                // JavaScript-catchable error into an abort. Route it through the
                // call terminator, exactly as the `Await` arm below does, so the
                // handler (or, with none active, the throwing-function
                // propagation pass) sees the throwing edge.
                let text_operand = self.lower_expr(*text)?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Builtin(BuiltinFn::JsonParse),
                    args: vec![text_operand],
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::HttpGetText { url } => {
                let url_operand = self.lower_expr(*url)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::HttpGetText { url: url_operand })?
            }
            ExprKind::DateNow => {
                self.assign_temp(expr.ty, expr.span, Rvalue::DateNow)?
            }
            ExprKind::DateSetNow { timestamp } => {
                let timestamp_operand = self.lower_expr(*timestamp)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateSetNow {
                        timestamp: timestamp_operand,
                    },
                )?
            }
            ExprKind::VitestRestoreAllMocks => {
                self.assign_temp(expr.ty, expr.span, Rvalue::VitestRestoreAllMocks)?
            }
            ExprKind::DateResetNow => {
                self.assign_temp(expr.ty, expr.span, Rvalue::DateResetNow)?
            }
            ExprKind::DateTimezoneOffset => {
                self.assign_temp(expr.ty, expr.span, Rvalue::DateTimezoneOffset)?
            }
            ExprKind::DateSetTimezoneOffset { offset } => {
                let offset_operand = self.lower_expr(*offset)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateSetTimezoneOffset {
                        offset: offset_operand,
                    },
                )?
            }
            ExprKind::DateResetTimezoneOffset => {
                self.assign_temp(expr.ty, expr.span, Rvalue::DateResetTimezoneOffset)?
            }
            ExprKind::VitestMockFn { implementation } => {
                let implementation_operand = implementation
                    .map(|implementation| self.lower_expr(implementation))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestMockFn {
                        implementation: implementation_operand,
                    },
                )?
            }
            ExprKind::VitestMockCalledTimes { mock, count } => {
                let mock_operand = self.lower_expr(*mock)?;
                let count_operand = self.lower_expr(*count)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestMockCalledTimes {
                        mock: mock_operand,
                        count: count_operand,
                    },
                )?
            }
            ExprKind::VitestMockCalledWith { mock, args, last } => {
                let mock_operand = self.lower_expr(*mock)?;
                let arg_operands = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestMockCalledWith {
                        mock: mock_operand,
                        args: arg_operands,
                        last: *last,
                    },
                )?
            }
            ExprKind::VitestSpyOn { target, name } => {
                let target_operand = self.lower_expr(*target)?;
                let name_operand = self.lower_expr(*name)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestSpyOn {
                        target: target_operand,
                        name: name_operand,
                    },
                )?
            }
            ExprKind::VitestAsymmetricEqual { actual, expected } => {
                let actual_operand = self.lower_expr(*actual)?;
                let expected_operand = self.lower_expr(*expected)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestAsymmetricEqual {
                        actual: actual_operand,
                        expected: expected_operand,
                    },
                )?
            }
            ExprKind::VitestMockLastResolvedWith { mock, expected } => {
                let mock_operand = self.lower_expr(*mock)?;
                let expected_operand = self.lower_expr(*expected)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::VitestMockLastResolvedWith {
                        mock: mock_operand,
                        expected: expected_operand,
                    },
                )?
            }
            ExprKind::DateTimezoneContext { timezone } => {
                let timezone_operand = self.lower_expr(*timezone)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateTimezoneContext {
                        timezone: timezone_operand,
                    },
                )?
            }
            ExprKind::DateToIsoString { timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateToIsoString {
                        timestamp_ms: timestamp_operand,
                    },
                )?
            }
            ExprKind::DateToString { timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateToString {
                        timestamp_ms: timestamp_operand,
                    },
                )?
            }
            ExprKind::DateFromParts { parts } => {
                let part_operands = parts
                    .iter()
                    .map(|part| self.lower_expr(*part))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateFromParts {
                        parts: part_operands,
                    },
                )?
            }
            ExprKind::DateFromValue { value } => {
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateFromValue {
                        value: value_operand,
                    },
                )?
            }
            ExprKind::DateGetPart { part, timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateGetPart {
                        part: *part,
                        timestamp_ms: timestamp_operand,
                    },
                )?
            }
            ExprKind::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                let value_operands = values
                    .iter()
                    .map(|value| self.lower_expr(*value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::DateSetPart {
                        part: *part,
                        timestamp_ms: timestamp_operand,
                        values: value_operands,
                    },
                )?
            }
            ExprKind::UrlField { field, url } => {
                let url_operand = self.lower_expr(*url)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::UrlField {
                        field: *field,
                        url: url_operand,
                    },
                )?
            }
            ExprKind::FileReadText { path } => {
                let path_operand = self.lower_expr(*path)?;
                self.assign_temp(expr.ty, expr.span, Rvalue::FileReadText { path: path_operand })?
            }
            ExprKind::FileWriteText { path, text } => {
                let path_operand = self.lower_expr(*path)?;
                let text_operand = self.lower_expr(*text)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::FileWriteText {
                        path: path_operand,
                        text: text_operand,
                    },
                )?
            }
            ExprKind::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => {
                let parts_operand = self.lower_expr(*parts)?;
                let blob_type_operand = self.lower_expr(*blob_type)?;
                let name_operand = name.map(|name_expr| self.lower_expr(name_expr)).transpose()?;
                let last_modified_operand = last_modified
                    .map(|last_modified_expr| self.lower_expr(last_modified_expr))
                    .transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::BlobFromParts {
                        parts: parts_operand,
                        blob_type: blob_type_operand,
                        name: name_operand,
                        last_modified: last_modified_operand,
                    },
                )?
            }
            ExprKind::HostConstruct { class_name, args } => {
                let arg_operands = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::HostConstruct {
                        class_name: class_name.clone(),
                        args: arg_operands,
                    },
                )?
            }
            ExprKind::BuiltinNamespace { name } => self.assign_temp(
                expr.ty,
                expr.span,
                Rvalue::BuiltinNamespace {
                    name: name.clone(),
                },
            )?,
            ExprKind::ArgumentsObject { fixed, rest } => {
                let fixed_operands = fixed
                    .iter()
                    .map(|value| self.lower_expr(*value))
                    .collect::<Result<Vec<_>, _>>()?;
                let rest_operand = rest.map(|value| self.lower_expr(value)).transpose()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ArgumentsObject {
                        fixed: fixed_operands,
                        rest: rest_operand,
                    },
                )?
            }
            ExprKind::HostGlobalRead { class } => {
                self.assign_temp(expr.ty, expr.span, Rvalue::HostGlobalRead { class: *class })?
            }
            ExprKind::HostGlobalWrite { class, value } => {
                let value_operand = self.lower_expr(*value)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::HostGlobalWrite {
                        class: *class,
                        value: value_operand,
                    },
                )?
            }
            ExprKind::HostGlobalPresent { class } => self.assign_temp(
                expr.ty,
                expr.span,
                Rvalue::HostGlobalPresent { class: *class },
            )?,
            ExprKind::Method {
                receiver,
                method,
                args,
            } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                if matches!(self.krate.types.get(receiver_ty), Some(Type::Union(_))) {
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
                let mut lowered_args = vec![receiver_operand];
                lowered_args.extend(
                    args.iter()
                        .map(|arg| self.lower_expr(*arg))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
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
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Await(future) => {
                let lowered_future = self.lower_expr(*future)?;
                let dest = self.push_temp(expr.ty, expr.span);
                if self.current_exception_handler().is_some() {
                    let target = self.function.push_block(expr.span);
                    self.set_terminator(Terminator::Await {
                        future: lowered_future,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.current_block = target;
                } else {
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::Await(lowered_future),
                    });
                }
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::AsyncOp { op, args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::AsyncOp {
                        op: *op,
                        args: lowered_args,
                    },
                )?
            }
            ExprKind::Closure(closure) => self.lower_closure_expr(closure, expr.ty, expr.span)?,
            ExprKind::ThisRead => self.assign_temp(expr.ty, expr.span, Rvalue::ThisRead)?,
            ExprKind::BindThis { callee, receiver } => {
                let callee_operand = self.lower_expr(*callee)?;
                let receiver_operand = self.lower_expr(*receiver)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::BindThis {
                        callee: callee_operand,
                        receiver: receiver_operand,
                    },
                )?
            }
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
                    self.set_terminator(Terminator::Call {
                        callee: Callee::Indirect(callee_operand),
                        args: lowered_args,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.current_block = target;
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
            ExprKind::ClosureCallSpread { callee, args } => {
                let callee_operand = self.lower_expr(*callee)?;
                let args_operand = self.lower_expr(*args)?;
                self.assign_temp(
                    expr.ty,
                    expr.span,
                    Rvalue::ClosureCallSpread {
                        callee: callee_operand,
                        args: args_operand,
                    },
                )?
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
        };

        self.exprs.insert(expr_id, operand.clone());
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
            Ok(Callee::Builtin(BuiltinFn::ConsoleLog))
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

        let place = self.lower_place(expr_id)?;
        if matches!(place, Place::Local(_)) {
            return Ok((Operand::Copy(place), None));
        }

        let local = self.push_temp(expr.ty, expr.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: local,
            value: Rvalue::Use(Operand::Copy(place.clone())),
        });
        Ok((Operand::Copy(Place::Local(local)), Some((place, local))))
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

    /// Extracts a local variable ID from an operand or returns an error.
    pub(super) fn local_operand(&self, operand: Operand, span: Span) -> Result<LocalId, LowerError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => match place {
                Place::Local(local) => Ok(local),
                Place::Field { .. } | Place::Index { .. } => Err(self.error(
                    "field and index reads currently require a local receiver",
                    Some(span),
                )),
            },
            Operand::Const(_) => Err(self.error(
                "field and index reads currently require a local receiver",
                Some(span),
            )),
        }
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
