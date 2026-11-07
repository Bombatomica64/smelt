//! Structured control flow: blocks, statements and terminators, with assignment,
//! call, switch and loop emission in the child modules.

use crate::rust::RustExpr;
use super::*;
use super::flow_state::Flow;

mod assign;
mod calls;
mod loops;
mod switch;

/// Describes how the successor blocks of a terminator must be rendered.
///
/// Structured emission has exactly two contexts. At the top level a successor is
/// simply the next region and is emitted with [`FunctionEmitter::emit_block`].
/// Inside a generated Rust `loop`, the same successor may instead be the loop
/// header — a back edge, which must render as `continue` — or the loop's exit
/// block, which must render as `break`.
///
/// The throwing-call and throwing-`await` emitters used to hardcode the
/// top-level choice for their `Ok`/`Err` continuations. A `try`/`catch` in a loop
/// body reaches the loop header from inside those match arms, so hardcoding
/// `emit_block` there re-emitted the whole loop header and body inside the arm,
/// recursively, until the block-depth cap (`ControlFlowState::enter_block`) replaced the tail with a
/// default return. Threading this enum through instead is what turns those back
/// edges into `continue`.
pub(super) enum Continuation<'a> {
    /// Emit the successor as an ordinary structured region.
    Block,
    /// Emit the successor as part of the body of an enclosing generated `loop`.
    InLoop {
        /// The loop header; an edge to it is a back edge and renders as `continue`.
        continue_target: smelt_mir::BlockId,
        /// The loop's exit block; an edge to it renders as `break`.
        break_target: Option<smelt_mir::BlockId>,
        /// Blocks already rendered on the current path, to stop unstructured cycles.
        visited: &'a BlockIdSet,
    },
    /// Emit the successor inside a Rust closure body.
    ///
    /// Closure bodies are rendered by their own recursive emitter
    /// ([`FunctionEmitter::emit_closure_block_inner`]) because a closure block can
    /// end as a Rust tail expression and its `Return` handling differs. Routing
    /// the throwing-terminator emitters through this variant lets a closure body
    /// reuse them verbatim instead of growing a second copy of their coercion
    /// logic that could drift.
    Closure {
        /// Blocks currently on the closure emitter's emission stack.
        active: &'a [smelt_mir::BlockId],
        /// Block at which emission stops (the enclosing loop header, if any).
        stop: Option<smelt_mir::BlockId>,
        /// Whether emission is inside a generated `loop`, which forces a MIR
        /// `Return` to render as an explicit `return` rather than a tail value.
        in_loop: bool,
    },
    /// Emit the successor as part of a forward region that ends at a join block.
    ///
    /// This is the non-loop sibling of [`Continuation::InLoop`]: an `if`/`else`
    /// arm or a `finally` cleanup region is also emitted "until a goto to a stop
    /// target", but that stop target is a forward join the caller emits once
    /// *after* the region, not a loop header. An edge to it therefore renders as
    /// nothing (the region simply ends), never as `continue` — which outside a
    /// loop is rustc E0268.
    Region {
        /// The join block at which the region ends; an edge to it renders nothing.
        stop: smelt_mir::BlockId,
        /// Blocks already rendered on the current path, to stop unstructured cycles.
        visited: &'a BlockIdSet,
    },
}

/// What the stop block of an [`FunctionEmitter::emit_block_until_goto`] region
/// means, and therefore how edges leaving the region are rendered.
///
/// The same region emitter serves two structurally different shapes, and the
/// difference decides whether `continue`/`break` are legal Rust at all:
///
/// * [`RegionExit::LoopBody`] — the region is the body of a generated `loop`.
///   The stop block is the loop header, so an edge to it is the back edge and
///   renders as `continue`; an edge to `break_block` renders as `break`.
/// * [`RegionExit::Join`] — the region is a forward arm (an `if`/`else` branch,
///   a `finally` cleanup) that ends at a join block the caller emits once after
///   the region. There is no enclosing loop, so an edge to the stop block just
///   ends the region and no edge may render as `continue` or `break`.
///
/// Carrying the break target *inside* the loop variant is what makes the two
/// shapes impossible to confuse: a loop region cannot be described without its
/// exit, and a join region cannot name one.
#[derive(Clone, Copy)]
pub(super) enum RegionExit {
    /// The region is a generated `loop` body.
    LoopBody {
        /// The loop's exit block; an edge to it renders as `break`. `None` when
        /// the caller reached this region through a path that has no single
        /// structural exit block (the loop is still there, so `continue` stays
        /// legal; there is simply no edge that renders as `break`).
        break_block: Option<smelt_mir::BlockId>,
    },
    /// The region is a forward arm ending at the caller's join block.
    Join,
}

impl RegionExit {
    /// Returns the loop exit block, or `None` for a forward join region.
    ///
    /// Region emission shares one body for both shapes; this adapts the variant
    /// back to the `Option<BlockId>` break target that shared body threads
    /// through to the loop-only helpers.
    pub(super) fn break_target(self) -> Option<smelt_mir::BlockId> {
        match self {
            Self::LoopBody { break_block } => break_block,
            Self::Join => None,
        }
    }

    /// Returns the [`Continuation`] the throwing-call and throwing-`await`
    /// emitters must use for a region ending at `stop`.
    ///
    /// A throwing terminator splits the region into `Ok`/`Err` arms whose
    /// successors are emitted through [`FunctionEmitter::emit_continuation`].
    /// Those successors live in exactly the region described by `self`, so the
    /// continuation has to agree with it: inside a loop body an edge to `stop`
    /// is the back edge (`continue`), inside a forward arm it is the end of the
    /// arm (nothing at all).
    pub(super) fn continuation(self, stop: smelt_mir::BlockId, visited: &BlockIdSet) -> Continuation<'_> {
        match self {
            Self::LoopBody { break_block } => Continuation::InLoop {
                continue_target: stop,
                break_target: break_block,
                visited,
            },
            Self::Join => Continuation::Region { stop, visited },
        }
    }
}

impl FunctionEmitter<'_> {
    /// Emits a basic block's statements and terminator.
    ///
    /// Emission recurses once per successor, so a straight-line function of
    /// `n` blocks is `n` frames deep. The guard below only exists to stop a
    /// cyclic region from recursing forever, so it scales with the function:
    /// it used to be capped at 64, which silently replaced the tail of any
    /// longer straight-line body with a default `return` (a module that
    /// performed more than 64 calls in a row lost its last statements). The
    /// stack grows on demand so a long body cannot overflow it instead.
    pub(super) fn emit_block(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        let limit = self.function.blocks.len().saturating_mul(2).max(16);
        let Some(_depth) = self.flow.enter_block(limit) else {
            out.push_str("    // Smelt could not structurally emit this recursive control-flow region yet.\n");
            out.push_str(&self.default_return_statement()?);
            self.flow.record(Flow::Diverges);
            return Ok(());
        };
        stacker::maybe_grow(256 * 1024, 4 * 1024 * 1024, || {
            self.emit_block_body(block, out)
        })
    }

    /// Emits `block` as a whole structured region and answers how its rendered
    /// tail ends.
    ///
    /// The answer is the [`Flow`] the region's last emitted terminator recorded
    /// (see `flow_state::ControlFlowState`); callers that need it take it from
    /// here instead of reading the emitter's register after the fact.
    pub(super) fn emit_region_flow(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<Flow, EmitError> {
        self.emit_block(block, out)?;
        Ok(self.flow.last_flow())
    }

    /// Emits a basic block after recursion-depth accounting has been applied.
    fn emit_block_body(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        if let Some((cond, repeat_block, exit_block, repeat_when_true)) =
            self.do_while_body(block)?
        {
            let repeated = self.block(repeat_block)?;
            let exit = self.block(exit_block)?;
            let loop_declared = self.declared_scope();
            out.push_str("    loop {\n");
            for statement in &block.statements {
                self.emit_statement_for_block(block, statement, out)?;
            }
            if repeat_when_true {
                out.push_str(&format!("    if !({cond}) {{ break; }}\n"));
            } else {
                out.push_str(&format!("    if {cond} {{ break; }}\n"));
            }
            self.emit_block_until_goto(
                repeated,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(exit_block),
                },
                out,
            )?;
            out.push_str("    }\n");
            loop_declared.close();
            return self.emit_block(exit, out);
        }

        // A compound short-circuit `while` header is recognized before the
        // single-block recognizers for the same reason as in the nested path:
        // `while_header` also matches it but would emit the wrong structure.
        if let Some((decision, body_entry, exit_entry, body_is_then)) =
            self.compound_while(block, &[])?
        {
            self.emit_compound_while(block, decision, body_entry, exit_entry, body_is_then, out)?;
            return self.emit_block(self.block(exit_entry)?, out);
        }

        if let Some((cond, then_block, else_block, cond_statement_idx)) =
            self.while_header(block)?
        {
            let then = self.block(then_block)?;
            let else_ = self.block(else_block)?;
            let loop_declared = self.declared_scope();
            self.open_while_loop(block, &cond, cond_statement_idx, out)?;
            self.emit_block_until_goto(
                then,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(else_block),
                },
                out,
            )?;
            out.push_str("    }\n");
            loop_declared.close();
            return self.emit_block(else_, out);
        }

        if let Some((cond, then_block, latch_block, else_block, cond_statement_idx)) =
            self.while_header_with_latch(block)?
        {
            let then = self.block(then_block)?;
            let latch = self.block(latch_block)?;
            let else_ = self.block(else_block)?;
            let loop_declared = self.declared_scope();
            self.open_while_loop(block, &cond, cond_statement_idx, out)?;
            self.emit_block_until_goto(
                then,
                latch_block,
                RegionExit::LoopBody {
                    break_block: Some(else_block),
                },
                out,
            )?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            loop_declared.close();
            return self.emit_block(else_, out);
        }

        for statement in &block.statements {
            self.emit_statement_for_block(block, statement, out)?;
        }

        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("basic block has no terminator"));
        };
        self.emit_terminator(block.id, terminator, out)
    }

    /// Emits a statement unless it is a dead narrowing cast before an unknown return.
    pub(super) fn emit_statement_for_block(
        &self,
        block: &BasicBlock,
        statement: &Statement,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if self.statement_is_dead_unknown_return_cast(block, statement)? {
            return Ok(());
        }
        self.emit_statement(statement, out)
    }

    /// Returns whether a statement only narrows an unknown value immediately returned as unknown.
    fn statement_is_dead_unknown_return_cast(
        &self,
        block: &BasicBlock,
        statement: &Statement,
    ) -> Result<bool, EmitError> {
        if self.mir.types.get(self.function.return_ty) != Some(&Type::Unknown) {
            return Ok(false);
        }
        let Statement::Assign {
            dest,
            value: Rvalue::UnknownCast { value, .. },
        } = statement
        else {
            return Ok(false);
        };
        if self.mir.types.get(self.operand_ty(value)?) != Some(&Type::Unknown) {
            return Ok(false);
        }
        Ok(matches!(
            &block.terminator,
            Some(Terminator::Return(
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
            )) if local == dest
        ))
    }

    /// Emits a single statement.
    pub(super) fn emit_statement(
        &self,
        statement: &Statement,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match statement {
            Statement::Assign { dest, value } => {
                // The payload of a `throw` is built at the throw site instead of
                // being staged in a temporary; see `emitter::throw`.
                if self.is_folded_throw_payload(*dest) {
                    return Ok(());
                }
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                if matches!(value, Rvalue::Closure { .. }) && !self.closure_binding_is_read(*dest)? {
                    return Ok(());
                }
                // A UNIT-typed slot read is a touch: it only forces the slot's
                // lazy initializer (the module body touching a lifted binding at
                // its declaration so the initializer runs in source order; see
                // the frontend's `force_module_slot_init`). Touch the cell
                // without cloning or binding its value.
                // (A slot that itself holds a unit value keeps the ordinary
                // read.)
                if let Rvalue::GlobalGet { global } = value
                    && matches!(self.mir.types.get(local.ty), Some(Type::None))
                    && !matches!(
                        self.mir.types.get(self.global_ty(*global)?),
                        Some(Type::None)
                    )
                {
                    out.push_str(&format!("    {};\n", self.global_touch_expr(*global)));
                    self.mark_local_declared(*dest);
                    return Ok(());
                }
                // A `Function`-typed `ClosureCall` result whose only consumer
                // erases it back to `SmeltUnknown` is re-rendered at the erase
                // site; the typed-callback binding would be a dead store that
                // re-evaluates the call. Suppress it (mirrors the call-terminator
                // path in `emit_call_terminator_statement`).
                if matches!(value, Rvalue::ClosureCall { .. })
                    && self.function_call_result_dead_when_erased(*dest)?
                {
                    self.mark_local_declared(*dest);
                    return Ok(());
                }
                // A `typeof` whose destination was narrowed to `&'static str`
                // skips the owned copy entirely -- the borrowed spelling IS the
                // value. See `emitter::typeof_str_locals`.
                let rendered_value = match value {
                    // A base initializer copy is handed its instance: the
                    // constructor's allocation of `this` becomes the receiver.
                    Rvalue::Struct { .. } if self.is_base_initializer_receiver(*dest) => {
                        crate::base_init::BASE_INIT_RECEIVER.to_owned()
                    }
                    Rvalue::TypeofValue { value: typeof_operand }
                        if self.local_is_static_typeof_str(*dest) =>
                    {
                        self.typeof_str_expr(typeof_operand)?.into_string()
                    }
                    _ => self.rvalue_expr_for_dest(value, local.ty)?.into_string(),
                };
                if self.deferred_capture_cells.borrow().contains(dest) {
                    // The cell was declared without a value; its one
                    // assignment creates it (see `capture_cell_created_at_assignment`).
                    out.push_str(&format!(
                        "    smelt_capture_{name} = ::std::rc::Rc::new(::std::cell::RefCell::new({rendered_value}));\n"
                    ));
                    return Ok(());
                }
                if self.is_local_declared(*dest)
                    && (!matches!(local.kind, LocalKind::Temp)
                        || self.mutable_locals.contains(dest)
                        || self.predeclared_locals.contains(dest))
                {
                    let assignment = self.assignment_place_expr(&Place::Local(*dest))?;
                    out.push_str(&format!("    {assignment} = {rendered_value};\n"));
                    return Ok(());
                }
                let mutability = if name != "_" && self.local_binding_needs_mut(*dest) {
                    "mut "
                } else {
                    ""
                };
                let annotation = if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                    && (self.local_binding_needs_mut(*dest)
                        || self.predeclared_locals.contains(dest)
                        || self.mutable_locals.contains(dest))
                {
                    format!(": {}", self.rendered_type(local.ty)?)
                } else if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                    String::new()
                } else if matches!(self.function.origin, HirOrigin::ClassConstructor { .. })
                    && name == "this"
                {
                    ": Self".to_owned()
                } else {
                    format!(": {}", self.local_declaration_type(*dest, local.ty)?)
                };
                if self.local_uses_shared_capture_storage(*dest) {
                    // Annotate the cell with the local's declared type, exactly as
                    // the two sibling capture-cell emitters do (the predeclaration
                    // path in `function_item.rs` and the `AssignPlace` path below). An
                    // unannotated `RefCell::new(Default::default())` has nothing to
                    // infer `T` from at the declaration, and a captured local that
                    // is only *read* in this body (`let func: DebounceFunction<any>`
                    // assigned in a sibling `beforeEach`, read by an arrow) leaves
                    // it unsolved — E0282 in the generated crate.
                    out.push_str(&format!(
                        "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>> = ::std::rc::Rc::new(::std::cell::RefCell::new({rendered_value}));\n",
                        self.capture_cell_type(*dest, local.ty)?
                    ));
                } else {
                    out.push_str(&format!(
                        "    let {mutability}{name}{annotation} = {rendered_value};\n",
                    ));
                }
                self.mark_local_declared(*dest);
                Ok(())
            }
            Statement::AssignPlace { place, value } => {
                self.emit_assign_place_statement(place, value, out)
            }
            Statement::DictEntryUpdate {
                base,
                index,
                default,
                current,
                value,
            } => self.emit_dict_entry_update_statement(*base, index, default, *current, value, out),
        }
    }

    /// Emits a block terminator.
    pub(super) fn emit_terminator(
        &self,
        current: smelt_mir::BlockId,
        terminator: &Terminator,
        out: &mut String,
    ) -> Result<(), EmitError> {
        // Default to "falls through"; the diverging arms below and any nested
        // emission set this to reflect the structural tail (the last terminator
        // emitted wins, which is always the region's continuation).
        self.flow.record(Flow::FallsThrough);
        match terminator {
            Terminator::Goto(target) => {
                if target.0 <= current.0 {
                    if self.block_eventually_terminates(*target, &mut BlockIdSet::default())? {
                        return self.emit_block(self.block(*target)?, out);
                    }
                    return self.emit_fallthrough_return(out);
                }
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind,
            } => {
                if let Some(handler) = unwind {
                    return self.emit_throwing_call_terminator(
                        callee,
                        args,
                        *dest,
                        *target,
                        *handler,
                        &Continuation::Block,
                        out,
                    );
                }
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Await {
                future,
                dest,
                target,
                unwind,
            } => {
                if let Some(handler) = unwind {
                    return self.emit_throwing_await_terminator(
                        future,
                        *dest,
                        *target,
                        *handler,
                        &Continuation::Block,
                        out,
                    );
                }
                self.emit_await_terminator_statement(future, *dest, out)?;
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Switch {
                cond,
                then_block,
                else_block,
            } => self.emit_switch(current, cond, *then_block, *else_block, out),
            Terminator::Match {
                scrutinee,
                arms,
                default,
            } => self.emit_match(scrutinee, arms, *default, out),
            Terminator::Return(operand) => self.emit_return_terminator(operand, out),
            Terminator::Throw(operand) => {
                // Shared with the closure-body emitter so the two throw paths
                // cannot drift; see `emitter::throw`.
                out.push_str(&self.throw_terminator_text(operand)?);
                self.flow.record(Flow::Diverges);
                Ok(())
            }
        }
    }

    /// Emits the region reached after a terminator, honoring loop context.
    ///
    /// This is the single place that decides whether an edge is an inline region,
    /// a `continue`, or a `break`, so the throwing-call and throwing-`await`
    /// emitters behave identically at the top level and inside a loop body.
    pub(super) fn emit_continuation(
        &self,
        target: smelt_mir::BlockId,
        continuation: &Continuation<'_>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match continuation {
            Continuation::Block => self.emit_block(self.block(target)?, out),
            Continuation::InLoop {
                continue_target,
                break_target,
                visited,
            } => {
                if target == *continue_target {
                    out.push_str("    continue;\n");
                    self.flow.record(Flow::Diverges);
                    return Ok(());
                }
                if Some(target) == *break_target {
                    out.push_str("    break;\n");
                    self.flow.record(Flow::Diverges);
                    return Ok(());
                }
                // Each arm of the emitted `match` is its own path, so it gets its
                // own copy of the visited set: a join block legitimately reached
                // from both the `Ok` and the `Err` arm is not a cycle, and sharing
                // one set would make the second arm collapse to a wrong `continue`.
                self.emit_loop_branch_inner(
                    self.block(target)?,
                    *continue_target,
                    *break_target,
                    out,
                    &mut (*visited).clone(),
                )
            }
            Continuation::Closure {
                active,
                stop,
                in_loop,
            } => {
                // Each arm of the emitted `match` is its own path, so it gets its
                // own copy of the emission stack for the same reason the loop case
                // copies its visited set: a block legitimately reached from both
                // the `Ok` and the `Err` arm is not a recursive cycle, and sharing
                // one stack would make the second arm emit the closure emitter's
                // "not structured yet" panic instead of its real continuation.
                let mut arm_active = active.to_vec();
                self.emit_closure_block_inner(
                    self.block(target)?,
                    out,
                    &mut arm_active,
                    *stop,
                    *in_loop,
                )
            }
            Continuation::Region { stop, visited } => {
                // Reaching the region's join block ends this arm: the join is
                // emitted once by the caller after the whole region, so the arm
                // contributes nothing and, having no enclosing loop, must not
                // render `continue` (E0268).
                if target == *stop {
                    self.flow.record(Flow::FallsThrough);
                    return Ok(());
                }
                // Each arm of the emitted `match` is its own path and so gets its
                // own copy of the visited set, exactly as the loop case does.
                self.emit_block_until_goto_inner(
                    self.block(target)?,
                    *stop,
                    RegionExit::Join,
                    out,
                    &mut (*visited).clone(),
                )
            }
        }
    }

    /// Emits a type-correct conservative return for unsupported control-flow regions.
    fn default_return_statement(&self) -> Result<String, EmitError> {
        let value = self.default_value(self.function.return_ty)?;
        if self.function.can_throw {
            self.binding_uses.record_early_return();
            Ok(format!(
                "    return Ok::<_, {ERROR_BOX}>({value});\n"
            ))
        } else {
            Ok(format!("    return {value};\n"))
        }
    }
}

/// Returns successor blocks for MIR terminators that continue execution.
pub(super) fn control_flow_successors(terminator: &Terminator) -> Vec<smelt_mir::BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Call { target, unwind, .. } | Terminator::Await { target, unwind, .. } => {
            unwind
                .iter()
                .map(|handler| handler.catch_block)
                .chain(std::iter::once(*target))
                .collect()
        }
        Terminator::Switch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Match { arms, default, .. } => {
            let mut successors = arms.iter().map(|arm| arm.target).collect::<Vec<_>>();
            if let Some(default_block) = default {
                successors.push(*default_block);
            }
            successors
        }
        Terminator::Return(_) | Terminator::Throw(_) => Vec::new(),
    }
}

impl FunctionEmitter<'_> {
    /// A `return`, with the adoption and wrapping the function kind asks of it.
    fn emit_return_terminator(
        &self,
        operand: &Operand,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let body_return_ty = self.body_return_ty();
        // `async function f(): Promise<T> { return p; }` ADOPTS `p` in
        // JavaScript: the returned promise is awaited and `f`'s promise
        // settles with *its* result, never with the promise object. Smelt
        // used to route this through the ordinary return coercion, which
        // saw a `Future<T>` flowing into a `T` slot and erased it with
        // `SmeltPromise::from_future` — so `await f()` handed back the
        // inner promise itself (`[object Promise]`) and a rejection inside
        // it never surfaced at the caller. That is what made
        // `withTimeout`, whose whole body is `return Promise.race([...])`,
        // both resolve with the wrong value and never reject on timeout.
        // Awaiting here is the general rule, not a promise-library
        // special case: a `Promise<Promise<T>>` is unspellable in
        // TypeScript, so an async body whose declared return type is not
        // itself a future can only mean adoption.
        if self.function.is_async
            && !self.function.is_generator
            && !matches!(self.function.origin, HirOrigin::ClassConstructor { .. })
            && let Some(&Type::Future(item_ty)) =
                self.mir.types.get(self.operand_ty(operand)?)
            && !matches!(self.mir.types.get(body_return_ty), Some(Type::Future(_)))
        {
            let future_text = self.operand_expr(operand)?;
            out.push_str(&format!(
                "    let smelt_adopted = {future_text}.await?;\n"
            ));
            if body_return_ty == self.none_ty {
                out.push_str("    let _ = smelt_adopted;\n");
                self.binding_uses.record_early_return();
                out.push_str("    return Ok(());\n");
            } else {
                let value =
                    self.rendered_at_type(&RustExpr::path("smelt_adopted"), item_ty, body_return_ty, &self.render_scope())?;
                self.binding_uses.record_early_return();
                out.push_str(&format!("    return Ok({value});\n"));
            }
            return Ok(());
        }
        if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
            if self.function.can_throw {
                self.binding_uses.record_early_return();
                out.push_str(&format!(
                    "    return Ok({});\n",
                    self.operand_expr(operand)?
                ));
            } else {
                out.push_str(&format!("    return {};\n", self.operand_expr(operand)?));
            }
        } else if body_return_ty == self.none_ty {
            // A `void`/`None` return still evaluates its operand for
            // side effects when one is present. The Rust output type is
            // `Result<(), _>` whenever the function is fallible *or*
            // async (async lowering always produces a `Result` output),
            // so wrap the terminator in `Ok(())` in both those cases.
            if !matches!(operand, Operand::Const(Constant::None))
                && self.operand_ty(operand)? != self.none_ty
            {
                out.push_str(&format!("    {};\n", self.operand_expr(operand)?));
            }
            if self.function.can_throw || self.function.is_async {
                self.binding_uses.record_early_return();
                if self.function.is_generator {
                    out.push_str(&format!("    return Ok::<(), {ERROR_BOX}>(());\n"));
                } else {
                    out.push_str("    return Ok(());\n");
                }
            } else {
                out.push_str("    return;\n");
            }
        } else if self.function.can_throw || self.function.is_async {
            // Fallible and async functions both return `Result<T, _>` in
            // Rust, so the returned value must be wrapped in `Ok(..)`.
            // Async is the addition here: an `async fn` desugars to a
            // future whose `Output` is `Result<T, _>` even when the
            // source body never throws, so a bare `return value;` would
            // mismatch the `Result` output (E0308).
            if matches!(operand, Operand::Const(Constant::None))
                && self.has_plain_default_value(body_return_ty)
            {
                let value = self.default_value(body_return_ty)?;
                if self.function.is_generator {
                    let return_ty_text =
                        self.rendered_type(body_return_ty)?;
                    self.binding_uses.record_early_return();
                    out.push_str(&format!(
                        "    return Ok::<{return_ty_text}, {ERROR_BOX}>({value});\n"
                    ));
                } else {
                    self.binding_uses.record_early_return();
                    out.push_str(&format!("    return Ok({value});\n"));
                }
            } else {
                let value = self.value_at_type(operand, body_return_ty)?;
                if self.function.is_generator {
                    let return_ty_text =
                        self.rendered_type(body_return_ty)?;
                    self.binding_uses.record_early_return();
                    out.push_str(&format!(
                        "    return Ok::<{return_ty_text}, {ERROR_BOX}>({value});\n"
                    ));
                } else {
                    self.binding_uses.record_early_return();
                    out.push_str(&format!("    return Ok({value});\n"));
                }
            }
        } else if self.mir.types.get(body_return_ty) == Some(&Type::Unknown)
            && let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                operand
            && let Some(source) = self.single_assignment_unknown_cast_source(*local)
            && self.mir.types.get(self.operand_ty(source)?) == Some(&Type::Unknown)
        {
            out.push_str(&format!(
                "    return {};\n",
                self.value_at_type(source, body_return_ty)?
            ));
        } else {
            out.push_str(&format!(
                "    return {};\n",
                self.value_at_type(operand, body_return_ty)?
            ));
        }
        self.flow.record(Flow::Diverges);
        Ok(())
    }
}
