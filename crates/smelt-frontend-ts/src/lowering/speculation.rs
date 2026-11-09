//! Lowering that is undone: a probe asks what a subtree would type as, and only
//! the answer survives.
//!
//! Expressions land in the current body and closure bodies in the crate, so a
//! probe that keeps them leaves orphans behind, and MIR lowers every crate body
//! whether an expression refers to it or not. A probe that declared an item
//! (a class expression, a lifted function) keeps its bodies: the item is
//! registered by id in the builder's name tables and owns them.
//!
//! Lowering also LOGS: every erasure decision is appended to
//! `krate.erasures`, and the gated unknown and typedness reports explain
//! avoidable erasure from that log. A discarded probe's records describe
//! values that no longer exist, so they are rolled back with the bodies
//! (audit finding A10); a probe that declared an item keeps them with it.
//!
//! Prefer [`ModuleBuilder::speculate`], which rolls back on every exit that
//! does not keep the probe, an early `?` included. The narrowing stack needs
//! no entry here: it is only pushed through `with_narrowing`, which restores
//! it on every exit (`state::local_scope`).

use smelt_hir::Body;

use crate::SmeltError;

use super::ModuleBuilder;

/// What a [`ModuleBuilder::roll_back_to`] returns to.
#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct SpeculationMark {
    /// Expressions of the current body.
    exprs: usize,
    /// Bodies of the crate.
    bodies: usize,
    /// Items of the crate.
    items: usize,
    /// Erasure records of the crate.
    erasures: usize,
}

impl ModuleBuilder<'_> {
    /// Mark the lowering state before a probe of `body`.
    pub(in crate::lowering) fn speculation_mark(&self, body: &Body) -> SpeculationMark {
        SpeculationMark {
            exprs: body.exprs.len(),
            bodies: self.ctx.krate.bodies.len(),
            items: self.ctx.krate.items.len(),
            erasures: self.ctx.krate.erasures.len(),
        }
    }

    /// Discard what lowering added to `body` and the crate since `mark`; true
    /// when `body` had grown.
    pub(in crate::lowering) fn roll_back_to(&mut self, mark: SpeculationMark, body: &mut Body) -> bool {
        let grew = body.exprs.len() > mark.exprs;
        body.exprs.truncate(mark.exprs);
        self.discard_kept_receivers_after(mark.exprs);
        if self.ctx.krate.items.len() == mark.items {
            self.ctx.krate.bodies.truncate(mark.bodies);
            self.ctx.krate.erasures.truncate(mark.erasures);
        }
        grew
    }

    /// Run `probe` and keep what it lowered only when it returns `Ok(Some(_))`.
    ///
    /// `Ok(None)` (the probe declined) and `Err` (it failed, through any
    /// early `?`) both roll everything it added back to the state before it
    /// ran, so no exit path can leave a half-lowered probe behind.
    pub(in crate::lowering) fn speculate<T>(
        &mut self,
        body: &mut Body,
        probe: impl FnOnce(&mut Self, &mut Body) -> Result<Option<T>, SmeltError>,
    ) -> Result<Option<T>, SmeltError> {
        let mark = self.speculation_mark(body);
        let result = probe(self, body);
        if !matches!(result, Ok(Some(_))) {
            self.roll_back_to(mark, body);
        }
        result
    }
}
