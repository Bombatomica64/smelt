//! Recording the erasures the Rust backend decides itself.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), `src/sema/aot_rules.rs`:
//! the reason an erased value exists is named where the erasure is decided.
//! Most erasure is decided by the frontends (recorded in
//! `smelt_hir::Crate::erasures`); four causes are decided only here, by
//! [`FunctionEmitter::rust_type`]:
//!
//! - `Type::Never` rendered as `SmeltUnknown` ([`ErasureReason::NeverValue`]);
//! - a type parameter whose substitution resolves to erased
//!   ([`ErasureReason::ErasedGeneric`]);
//! - a `Type::Class` that names no lowered class or interface
//!   ([`ErasureReason::UnresolvedClass`]);
//! - a `Type::Union` that is not a concrete generated union
//!   ([`ErasureReason::ErasedUnion`]).
//!
//! # Span granularity
//!
//! `rust_type` renders a `TypeId`, not a local, so it does not know which
//! source value it is rendering. The record is attributed to the function
//! being emitted: the span of its entry block, which MIR lowering sets to the
//! function body. The detail names the erased construct (the type parameter,
//! the class, the union's arm count). Trial renders (a generic function's
//! body-cleanliness probe) can record the same decision twice; the consumer
//! deduplicates by span, reason and detail with
//! [`smelt_hir::normalize_erasures`].

use smelt_hir::{ErasureReason, ErasureRecord, Span};

use super::{EmitContext, FunctionEmitter};

impl EmitContext {
    /// Take every backend erasure recorded so far.
    pub(crate) fn take_backend_erasures(&self) -> Vec<ErasureRecord> {
        std::mem::take(&mut *self.backend_erasures.borrow_mut())
    }
}

impl FunctionEmitter<'_> {
    /// Record that `rust_type` erased a value of the function being emitted
    /// for `reason`; see the module docs for the span used.
    pub(super) fn note_backend_erasure(&self, reason: ErasureReason, detail: String) {
        let span = self.backend_erasure_span();
        self.context
            .backend_erasures
            .borrow_mut()
            .push(ErasureRecord::new(reason, span, detail));
    }

    /// The span a backend erasure in this function is attributed to: the
    /// entry block's span (the function body), or a zero span in the first
    /// file when the function has no blocks.
    fn backend_erasure_span(&self) -> Span {
        self.function
            .blocks
            .iter()
            .find(|block| block.id == self.function.entry)
            .or_else(|| self.function.blocks.first())
            .map_or(Span::new(smelt_hir::FileId(0), 0, 0), |block| block.span)
    }
}
