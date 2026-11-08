//! Lowering that is undone: a probe asks what a subtree would type as, and only
//! the answer survives.
//!
//! Expressions land in the current body and closure bodies in the crate, so a
//! probe that keeps them leaves orphans behind, and MIR lowers every crate body
//! whether an expression refers to it or not. A probe that declared an item
//! (a class expression, a lifted function) keeps its bodies: the item is
//! registered by id in the builder's name tables and owns them.

use smelt_hir::Body;

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
}

impl ModuleBuilder<'_> {
    /// Mark the lowering state before a probe of `body`.
    pub(in crate::lowering) fn speculation_mark(&self, body: &Body) -> SpeculationMark {
        SpeculationMark {
            exprs: body.exprs.len(),
            bodies: self.ctx.krate.bodies.len(),
            items: self.ctx.krate.items.len(),
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
        }
        grew
    }
}
