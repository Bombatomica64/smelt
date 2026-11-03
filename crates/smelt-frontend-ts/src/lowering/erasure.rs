//! Recording erasure decisions at the lowering site.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), `src/sema/aot_rules.rs`:
//! every place the TypeScript frontend chooses `Type::Unknown` for a value
//! names its [`ErasureReason`] here, and the decision is appended to
//! [`smelt_hir::Crate::erasures`] with the source span being lowered. The
//! reason table, classes and severities live in [`smelt_hir::erasure`].
//!
//! # The anchor span
//!
//! Most erasure sites sit deep in helpers that receive a HIR type, not an AST
//! node, so they have no span of their own. The builder instead keeps an
//! *anchor*: the innermost source node currently being lowered. The three
//! recursive entry points — [`ModuleBuilder::ts_type_to_hir`] (type
//! annotations), `expression_with_hint` (expressions) and `statement_in_block`
//! (statements), plus the module-level statement loops — save the anchor, set
//! it to their node's span, and restore it on the way out. A record therefore
//! points at the annotation, expression or statement whose lowering erased the
//! value. A site with a more precise span at hand uses
//! [`ModuleBuilder::erase_at`].
//!
//! # Not every `Type::Unknown` is a decision
//!
//! Comparing a type against `unknown`, building the erased arm of a type that
//! is *already* erased upstream, or naming the tagged type in a runtime-helper
//! signature do not erase anything new. Those use
//! [`ModuleBuilder::unknown_type`], which records nothing. Its doc spells out
//! when it is allowed; a site that decides erasure must use `erase`.

use smelt_hir::{ErasureReason, ErasureRecord, Span, Type, TypeId};

use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Erase a value to `Type::Unknown` for `reason`, recording the decision at
    /// the current anchor span (the innermost node being lowered).
    pub(in crate::lowering) fn erase(&mut self, reason: ErasureReason) -> TypeId {
        let span = self.erasure_anchor;
        self.erase_at(reason, span, "")
    }

    /// Erase a value for `reason` with a short `detail` (a name, a construct),
    /// recorded at the current anchor span.
    pub(in crate::lowering) fn erase_with(
        &mut self,
        reason: ErasureReason,
        detail: &str,
    ) -> TypeId {
        let span = self.erasure_anchor;
        self.erase_at(reason, span, detail)
    }

    /// Erase a value for `reason`, recording the decision at `span`.
    pub(in crate::lowering) fn erase_at(
        &mut self,
        reason: ErasureReason,
        span: Span,
        detail: &str,
    ) -> TypeId {
        self.ctx
            .krate
            .erasures
            .push(ErasureRecord::new(reason, span, detail));
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// The interned `Type::Unknown` id, WITHOUT recording an erasure.
    ///
    /// Allowed only where no value is newly erased:
    /// - comparing or matching a type against `unknown`;
    /// - rebuilding a type whose erasure was already decided (and recorded)
    ///   upstream, e.g. the element of an already-erased collection;
    /// - naming the tagged type in a synthesized runtime-helper signature whose
    ///   values are converted at an explicit boundary.
    ///
    /// Anything that turns a value with a static shape into `unknown` must use
    /// [`Self::erase`] so the decision is reported.
    pub(in crate::lowering) fn unknown_type(&mut self) -> TypeId {
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// Run `lower` with the anchor moved to `span`, restoring the enclosing
    /// anchor on every exit path of `lower` (an early `?` included), so an
    /// inner node can never leave its span behind for an outer decision.
    pub(in crate::lowering) fn with_erasure_anchor<R>(
        &mut self,
        span: Span,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.erasure_anchor, span);
        let result = lower(self);
        self.erasure_anchor = previous;
        result
    }

    /// Anchor erasures at a top-level statement of the module.
    ///
    /// Top-level statements are not nested in any other node, so there is no
    /// enclosing anchor to restore: each statement simply replaces the
    /// previous one's.
    pub(in crate::lowering) fn anchor_top_level_statement(&mut self, span: Span) {
        self.erasure_anchor = span;
    }
}
