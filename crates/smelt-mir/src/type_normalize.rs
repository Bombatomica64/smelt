//! MIR operational type normalization.
//!
//! This pass rewrites MIR-visible type IDs to canonical shapes before
//! optimization and codegen. It keeps source-language optional/nullish spelling
//! from leaking into generated Rust as nested `Option<Option<T>>`.
//!
//! Which positions hold a type is not decided here: the pass maps every
//! position [`crate::type_positions`] lists, the same list the validator checks,
//! so a type the backend reads cannot be left unnormalized.

use smelt_hir::type_normalize::{NormalizeOptions, normalize_type};

use crate::Mir;

/// Normalize all MIR operational type references in place.
pub fn normalize_operational_types(mir: &mut Mir) {
    mir.map_types(|types, ty| normalize_type(types, ty, NormalizeOptions::default()));
}
