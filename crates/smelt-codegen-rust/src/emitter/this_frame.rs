//! The receiver-channel prologue every generated frame starts with.
//!
//! JavaScript passes `this` to each call like a hidden argument: a member call,
//! `.call`/`.apply`/`.bind` supply a receiver and a plain call supplies
//! `undefined`. The runtime's channel holds a receiver only between the install
//! at a call and the callee's entry, so every function, method, closure and
//! generator body takes it (when the body reads `this`) or discards it (when it
//! does not) as its first act. Calls the body then makes without installing a
//! receiver -- a plain `f()`, a callback a helper invokes -- observe
//! `undefined`, whatever their static type. An ARROW (`ClosureThis::Lexical`)
//! has no `this` of its own: it only resets, and its `this` is the enclosing
//! frame's `smelt_frame_this`, cloned into it like a capture. A program that
//! never needs the channel (`Need::ThisChannel`) emits no prologue at all.

use super::*;

impl FunctionEmitter<'_> {
    /// Whether this frame needs the receiver it took at entry: its own body
    /// reads `this`, or an ARROW it creates (at any depth of arrows) does.
    pub(super) fn body_reads_this(&self) -> bool {
        self.blocks_need_enclosing_this(&self.function.blocks, 0)
    }

    /// Whether an arrow closure reads its enclosing frame's `this` (directly
    /// or through a nested arrow); `false` for any other closure, whose `this`
    /// is its own.
    pub(super) fn closure_needs_frame_this(&self, id: smelt_mir::ClosureId) -> bool {
        self.mir_closure(id).is_ok_and(|closure| {
            closure.this_binding == smelt_hir::ClosureThis::Lexical
                && self.blocks_need_enclosing_this(&closure.blocks, 1)
        })
    }

    /// Whether `blocks` read the `this` of the frame they belong to: a
    /// `Rvalue::ThisRead`, or an arrow created there that needs it. `depth`
    /// bounds the walk through nested arrows.
    fn blocks_need_enclosing_this(&self, blocks: &[BasicBlock], depth: usize) -> bool {
        const MAX_DEPTH: usize = 64;
        blocks.iter().any(|block| {
            block.statements.iter().any(|statement| match statement.value() {
                Rvalue::ThisRead | Rvalue::BindOwnThis { .. } => true,
                Rvalue::Closure { id, .. } if depth < MAX_DEPTH => self
                    .mir_closure(*id)
                    .is_ok_and(|closure| {
                        closure.this_binding == smelt_hir::ClosureThis::Lexical
                            && self.blocks_need_enclosing_this(&closure.blocks, depth.saturating_add(1))
                    }),
                _ => false,
            }) || matches!(
                &block.terminator,
                Some(Terminator::Call { callee: Callee::Op(op, _), .. }) if matches!(**op, Rvalue::ThisRead)
            )
        })
    }

    /// The frame prologue statement (without indentation or newline), or
    /// `None` in a program without the receiver channel.
    pub(super) fn this_frame_prologue(&self) -> Option<&'static str> {
        self.this_frame_prologue_for(false)
    }

    /// [`Self::this_frame_prologue`] for a closure body; an ARROW (`lexical`)
    /// never takes the channel's receiver -- its `this` is the enclosing
    /// frame's, captured as `smelt_frame_this` -- it only resets the channel
    /// for the calls it makes.
    pub(super) fn this_frame_prologue_for(&self, lexical: bool) -> Option<&'static str> {
        if !self.context.demand().has(Need::ThisChannel) {
            return None;
        }
        Some(if !lexical && self.body_reads_this() {
            "let smelt_frame_this = smelt_take_this();"
        } else {
            "smelt_reset_this();"
        })
    }

    /// The text of a `this` read: the receiver this frame took at entry.
    pub(super) fn this_read_text(&self) -> &'static str {
        if self.context.demand().has(Need::ThisChannel) {
            "smelt_frame_this.clone()"
        } else {
            "smelt_this()"
        }
    }
}
