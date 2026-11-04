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

    /// Forget all state, for a fresh render of the same body.
    pub(crate) fn reset(&self) {
        self.block_depth.set(0);
        self.until_depth.set(0);
        self.last_flow.set(Flow::FallsThrough);
        self.loop_join_stops.borrow_mut().clear();
        self.loop_join_violation.set(false);
    }
}
