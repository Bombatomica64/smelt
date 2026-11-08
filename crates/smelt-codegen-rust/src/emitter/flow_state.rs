//! Per-emitter control-flow emission state.
//!
//! Structured block emission keeps a little state across its recursion: two
//! recursion-depth counters that stop a cyclic region from recursing forever,
//! whether the most recently emitted region tail diverges, and the stack of
//! pending loop-body JOIN stops. It used to live in thread-locals shared by
//! every emitter on the thread — a closure body (rendered by its own emitter
//! in the middle of its parent's recursion) saw and wrote its parent's
//! counters and divergence register, and the join stops had to be keyed by the
//! address of the emitter's `MirFunction` to tell emitters apart.
//!
//! [`ControlFlowState`] is owned by one [`FunctionEmitter`], so none of that
//! sharing exists: a join stop belongs to the emitter that registered it, and
//! the depth counters are held through RAII [`DepthGuard`]s that cannot be left
//! incremented by an early `?`.
//!
//! It also keeps the stack of generated Rust loops whose bodies are being
//! emitted ([`ControlFlowState::open_loop`]). A region walker only knows its
//! own loop's continue and break targets; an edge that leaves several loops at
//! once (a source `continue outer` / `break outer`) has to name the loop it
//! targets, so such an edge becomes a labelled `continue 'smelt_loop_N` /
//! `break 'smelt_loop_N`, and the label is written onto the loop only when
//! some edge used it.

use std::cell::{Cell, RefCell};

/// How a rendered region ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Flow {
    /// Control can continue after the rendered region.
    #[default]
    FallsThrough,
    /// The rendered region's structural tail is a `return`, `throw` or
    /// `unreachable!()`: nothing after it is reachable.
    Diverges,
}

impl Flow {
    /// The flow of a region that diverges exactly when `diverges` holds.
    pub(crate) const fn from_diverges(diverges: bool) -> Self {
        if diverges { Self::Diverges } else { Self::FallsThrough }
    }

    /// Whether the region diverges.
    pub(crate) const fn diverges(self) -> bool {
        matches!(self, Self::Diverges)
    }
}

/// A pending loop-body join: a branch walker reaching `join` ends its arm
/// there, and the caller emits the join once after the `if` (see
/// `FunctionEmitter::emit_loop_branch_at_join`).
#[derive(Clone, Copy, Debug)]
struct LoopJoinStop {
    /// The join block.
    join: smelt_mir::BlockId,
    /// The continue target of the loop the join belongs to.
    loop_context: smelt_mir::BlockId,
}

/// A generated Rust loop whose body is being emitted.
#[derive(Clone, Copy, Debug)]
struct OpenLoop {
    /// The MIR block heading the loop; it names the loop's label.
    header: smelt_mir::BlockId,
    /// The block an iteration ends at: the header, or the latch whose
    /// statements close each iteration before the header runs again.
    continue_target: smelt_mir::BlockId,
    /// Whether `continue_target` is a latch (its statements are emitted at the
    /// end of the body, so a labelled `continue` must run them first).
    has_latch: bool,
    /// The block the loop exits to, when it has a single structural exit.
    break_block: Option<smelt_mir::BlockId>,
    /// Whether some edge jumped to this loop by label.
    labelled: bool,
}

/// An edge that leaves more than one generated loop: see
/// [`ControlFlowState::enclosing_loop_jump`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopJump {
    /// Start the next iteration of the loop headed by `header`, after running
    /// the statements of `latch` when the loop has one.
    Continue {
        /// The loop's header.
        header: smelt_mir::BlockId,
        /// The latch block whose statements run before the jump.
        latch: Option<smelt_mir::BlockId>,
    },
    /// Leave the loop headed by `header`.
    Break {
        /// The loop's header.
        header: smelt_mir::BlockId,
    },
}

/// The Rust label of the generated loop headed by `header`.
pub(crate) fn loop_label(header: smelt_mir::BlockId) -> String {
    format!("'smelt_loop_{}", header.0)
}

/// One open loop pushed by [`ControlFlowState::open_loop`]. Closing it with
/// [`OpenLoopScope::close`] pops it and labels the loop when an edge used the
/// label; dropping it unclosed (an early `?`) only pops it.
pub(crate) struct OpenLoopScope<'a> {
    /// The state whose loop stack this scope pushed onto.
    state: &'a ControlFlowState,
    /// The byte offset in the output buffer where the loop's opening line
    /// (`    loop {` / `    while cond {`) starts.
    opened_at: usize,
    /// Whether [`OpenLoopScope::close`] already popped the loop.
    closed: bool,
}

impl OpenLoopScope<'_> {
    /// Pop the loop and, when some edge jumped to it by label, write the label
    /// in front of its opening keyword in `out` (the buffer it was opened in).
    pub(crate) fn close(mut self, out: &mut String) {
        self.closed = true;
        let Some(open) = self.state.open_loops.borrow_mut().pop() else {
            return;
        };
        if !open.labelled {
            return;
        }
        let indent = out[self.opened_at..]
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count();
        out.insert_str(self.opened_at + indent, &format!("{}: ", loop_label(open.header)));
    }
}

impl Drop for OpenLoopScope<'_> {
    /// Pop the loop if [`OpenLoopScope::close`] was not reached.
    fn drop(&mut self) {
        if !self.closed {
            self.state.open_loops.borrow_mut().pop();
        }
    }
}

/// Control-flow emission state owned by one emitter; see the module docs.
#[derive(Debug, Default)]
pub(crate) struct ControlFlowState {
    /// Recursion depth of `FunctionEmitter::emit_block`.
    block_depth: Cell<usize>,
    /// Recursion depth of `FunctionEmitter::emit_block_until_goto`.
    until_depth: Cell<usize>,
    /// How the most recently emitted terminator ends.
    ///
    /// Every structured emission ends by emitting its continuation block last
    /// (a hoisted match join, the block after an `if`, or — when all arms
    /// diverge — the final diverging arm itself), so the last terminator
    /// emitted always corresponds to the structural tail of the rendered
    /// region. Region emitters that need the answer read it through
    /// [`ControlFlowState::last_flow`] right after the region; the MIR CFG can
    /// retain a phantom fall-through edge the structured emitter never
    /// renders, so `block_eventually_terminates` alone under-reports it.
    last_flow: Cell<Flow>,
    /// Pending loop-body join stops, innermost last.
    loop_join_stops: RefCell<Vec<LoopJoinStop>>,
    /// Set when a walker reached a pending join from a DIFFERENT loop context
    /// (from inside a nested generated loop, or a forward join region), where
    /// ending the arm would be wrong; the join-based attempt is then discarded.
    loop_join_violation: Cell<bool>,
    /// Generated Rust loops whose bodies are being emitted, innermost last.
    open_loops: RefCell<Vec<OpenLoop>>,
}

/// Holds one level of a recursion-depth counter and releases it on drop.
pub(crate) struct DepthGuard<'a> {
    /// The counter this guard incremented.
    depth: &'a Cell<usize>,
}

impl Drop for DepthGuard<'_> {
    /// Release the level this guard holds.
    fn drop(&mut self) {
        self.depth.set(self.depth.get().saturating_sub(1));
    }
}

/// Enter one level of `depth`, or answer `None` when it already exceeds
/// `limit` (the caller then surrenders instead of recursing).
fn enter(depth: &Cell<usize>, limit: usize) -> Option<DepthGuard<'_>> {
    let current = depth.get();
    if current > limit {
        return None;
    }
    depth.set(current.saturating_add(1));
    Some(DepthGuard { depth })
}

/// A pending join stop registered by
/// [`ControlFlowState::push_loop_join_stop`]; popping it and restoring the
/// enclosing violation flag happen on drop.
pub(crate) struct LoopJoinScope<'a> {
    /// The state whose stop stack this scope pushed onto.
    state: &'a ControlFlowState,
    /// The enclosing attempt's violation flag, restored on drop.
    previous_violation: bool,
    /// Whether [`LoopJoinScope::finish`] already restored the state.
    finished: bool,
}

impl LoopJoinScope<'_> {
    /// Pop the stop and answer whether any walker reached it from a different
    /// loop context while it was pending.
    pub(crate) fn finish(mut self) -> bool {
        self.finished = true;
        self.restore()
    }

    /// Pop the stop, restore the enclosing violation flag, and answer this
    /// scope's own violation.
    fn restore(&self) -> bool {
        self.state.loop_join_stops.borrow_mut().pop();
        self.state.loop_join_violation.replace(self.previous_violation)
    }
}

impl Drop for LoopJoinScope<'_> {
    /// Restore the state if [`LoopJoinScope::finish`] was not reached.
    fn drop(&mut self) {
        if !self.finished {
            self.restore();
        }
    }
}

impl ControlFlowState {
    /// Enter one level of block emission, or `None` past `limit`.
    pub(crate) fn enter_block(&self, limit: usize) -> Option<DepthGuard<'_>> {
        enter(&self.block_depth, limit)
    }

    /// Enter one level of until-goto region emission, or `None` past `limit`.
    pub(crate) fn enter_until(&self, limit: usize) -> Option<DepthGuard<'_>> {
        enter(&self.until_depth, limit)
    }

    /// Record how the terminator just emitted ends.
    pub(crate) fn record(&self, flow: Flow) {
        self.last_flow.set(flow);
    }

    /// How the most recently emitted terminator ends.
    pub(crate) fn last_flow(&self) -> Flow {
        self.last_flow.get()
    }

    /// Register a pending join stop for the duration of the returned scope.
    pub(crate) fn push_loop_join_stop(
        &self,
        join: smelt_mir::BlockId,
        loop_context: smelt_mir::BlockId,
    ) -> LoopJoinScope<'_> {
        self.loop_join_stops
            .borrow_mut()
            .push(LoopJoinStop { join, loop_context });
        LoopJoinScope {
            state: self,
            previous_violation: self.loop_join_violation.replace(false),
            finished: false,
        }
    }

    /// The loop context `block` was registered under as a pending join, if it
    /// is one (innermost registration wins).
    fn registered_join_context(&self, block: smelt_mir::BlockId) -> Option<smelt_mir::BlockId> {
        self.loop_join_stops
            .borrow()
            .iter()
            .rev()
            .find(|stop| stop.join == block)
            .map(|stop| stop.loop_context)
    }

    /// Whether `block` is a pending join (so the current arm must END there);
    /// reaching it from a context other than `loop_context` records a
    /// violation.
    pub(crate) fn loop_join_stop_hit(
        &self,
        block: smelt_mir::BlockId,
        loop_context: Option<smelt_mir::BlockId>,
    ) -> bool {
        let Some(context) = self.registered_join_context(block) else {
            return false;
        };
        if loop_context != Some(context) {
            self.loop_join_violation.set(true);
        }
        true
    }

    /// Every pending join block.
    pub(crate) fn pending_joins(&self) -> Vec<smelt_mir::BlockId> {
        self.loop_join_stops.borrow().iter().map(|stop| stop.join).collect()
    }

    /// Push a generated loop whose opening line starts at byte `opened_at` of
    /// the output buffer, for as long as the returned scope lives.
    ///
    /// `continue_target` is where an iteration ends (the header, or `latch`
    /// when the loop closes each iteration with a latch block's statements);
    /// `break_block` is the loop's exit, when it has one.
    pub(crate) fn open_loop(
        &self,
        header: smelt_mir::BlockId,
        latch: Option<smelt_mir::BlockId>,
        break_block: Option<smelt_mir::BlockId>,
        opened_at: usize,
    ) -> OpenLoopScope<'_> {
        self.open_loops.borrow_mut().push(OpenLoop {
            header,
            continue_target: latch.unwrap_or(header),
            has_latch: latch.is_some(),
            break_block,
            labelled: false,
        });
        OpenLoopScope {
            state: self,
            opened_at,
            closed: false,
        }
    }

    /// The labelled jump an edge to `target` needs when `target` ends an
    /// iteration of, or exits, an open loop OTHER than the innermost one.
    ///
    /// Edges to the innermost loop's own targets are the region walker's
    /// plain `continue`/`break` and are left to it; so is a block that is also
    /// one of the innermost loop's targets. Otherwise the innermost-out search
    /// finds the nearest enclosing loop the edge ends or leaves, marks it as
    /// labelled, and answers the jump. A walker that ignored this re-entered
    /// the enclosing loop's header as straight-line code and closed the cycle
    /// with a bare `continue` of the INNER loop: a source `continue outer`
    /// then skipped the outer update and never terminated.
    pub(crate) fn enclosing_loop_jump(&self, target: smelt_mir::BlockId) -> Option<LoopJump> {
        let mut loops = self.open_loops.borrow_mut();
        let (innermost, enclosing) = loops.split_last_mut()?;
        if target == innermost.continue_target
            || target == innermost.header
            || Some(target) == innermost.break_block
        {
            return None;
        }
        for open in enclosing.iter_mut().rev() {
            if target == open.continue_target {
                open.labelled = true;
                return Some(LoopJump::Continue {
                    header: open.header,
                    latch: open.has_latch.then_some(open.continue_target),
                });
            }
            if target == open.header {
                // Straight to the header: Rust's `continue` re-runs it
                // without the latch statements, as the edge does.
                open.labelled = true;
                return Some(LoopJump::Continue {
                    header: open.header,
                    latch: None,
                });
            }
            if Some(target) == open.break_block {
                open.labelled = true;
                return Some(LoopJump::Break { header: open.header });
            }
        }
        None
    }

    /// Whether a generated loop is open around the code being emitted.
    pub(crate) fn in_open_loop(&self) -> bool {
        !self.open_loops.borrow().is_empty()
    }

    /// Forget all state, for a fresh render of the same body.
    pub(crate) fn reset(&self) {
        self.block_depth.set(0);
        self.until_depth.set(0);
        self.last_flow.set(Flow::FallsThrough);
        self.loop_join_stops.borrow_mut().clear();
        self.loop_join_violation.set(false);
        self.open_loops.borrow_mut().clear();
    }
}
