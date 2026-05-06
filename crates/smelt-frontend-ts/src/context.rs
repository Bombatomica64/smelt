//! HIR construction context for TypeScript frontend lowering.

use smelt_hir::Crate as HirCrate;

/// Context for building HIR from TypeScript source.
///
/// Manages the crate structure and accumulates items during lowering.
#[derive(Debug)]
pub struct HirCtx {
    /// The HIR crate being constructed.
    pub krate: HirCrate,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
