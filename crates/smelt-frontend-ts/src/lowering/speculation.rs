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
//! values that do not exist, so they are rolled back with the bodies; a
//! probe that declared an item keeps them with it.
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
    /// Identity slots created (`HirCtx::identity_slot_log`).
    identity_slots: usize,
}

impl ModuleBuilder<'_> {
    /// Mark the lowering state before a probe of `body`.
    pub(in crate::lowering) fn speculation_mark(&self, body: &Body) -> SpeculationMark {
        SpeculationMark {
            exprs: body.exprs.len(),
            bodies: self.ctx.krate.bodies.len(),
            items: self.ctx.krate.items.len(),
            erasures: self.ctx.krate.erasures.len(),
            identity_slots: self.ctx.identity_slot_log.len(),
        }
    }

    /// Discard what lowering added to `body` and the crate since `mark`; true
    /// when `body` had grown.
    pub(in crate::lowering) fn roll_back_to(
        &mut self,
        mark: SpeculationMark,
        body: &mut Body,
    ) -> bool {
        let grew = body.exprs.len() > mark.exprs;
        body.exprs.truncate(mark.exprs);
        self.discard_kept_receivers_after(mark.exprs);
        self.discard_identity_slots_after(mark);
        if self.ctx.krate.items.len() == mark.items {
            self.ctx.krate.bodies.truncate(mark.bodies);
            self.ctx.krate.erasures.truncate(mark.erasures);
        }
        grew
    }

    /// Remove the identity slots a discarded probe created, when they are the
    /// only items it added: a value read the probe lowered and dropped must
    /// not leave a slot (and its initializer) that nothing reads.
    fn discard_identity_slots_after(&mut self, mark: SpeculationMark) {
        let Some(&(_, first_items)) = self.ctx.identity_slot_log.get(mark.identity_slots) else {
            return;
        };
        // Items the probe declared before its first slot, or between slots,
        // are registered by name and keep everything (see the module docs).
        let only_slots = first_items == mark.items
            && self
                .ctx
                .identity_slot_log
                .get(mark.identity_slots..)
                .unwrap_or_default()
                .windows(2)
                .all(|pair| matches!(pair, [(_, before), (_, after)] if *after == before + 2))
            && self
                .ctx
                .identity_slot_log
                .last()
                .is_some_and(|&(_, last)| self.ctx.krate.items.len() == last + 2);
        if !only_slots {
            return;
        }
        for (key, _) in self.ctx.identity_slot_log.drain(mark.identity_slots..) {
            self.ctx.identity_slots.remove(&key);
        }
        self.ctx.krate.items.truncate(mark.items);
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
