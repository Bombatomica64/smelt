//! HIR construction context for TypeScript frontend lowering.

use std::collections::HashMap;

use smelt_hir::{Crate as HirCrate, ItemId};

/// Context for building HIR from TypeScript source.
///
/// Manages the crate structure and accumulates items during lowering.
#[derive(Debug)]
pub struct HirCtx {
    /// The HIR crate being constructed.
    pub krate: HirCrate,
    /// Exported aliases created by re-export declarations.
    pub export_aliases: HashMap<String, ItemId>,
    /// Exported object constants used as namespace-like API surfaces.
    pub object_namespaces: HashMap<String, HashMap<String, ItemId>>,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
            export_aliases: HashMap::new(),
            object_namespaces: HashMap::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
