//! HIR construction context for TypeScript frontend lowering.

use std::collections::HashMap;

use smelt_hir::{Crate as HirCrate, Field, ItemId, Literal, TypeId};

/// A static object-constant entry that can be recreated in later lowered bodies.
#[derive(Debug, Clone)]
pub struct ObjectConstEntry {
    /// Source object key.
    pub key: String,
    /// Literal value stored under the key.
    pub value: Literal,
    /// HIR type of the literal value.
    pub value_ty: TypeId,
}

/// Static object-constant metadata preserved across TypeScript modules.
#[derive(Debug, Clone)]
pub struct ObjectConst {
    /// Entries in source order.
    pub entries: Vec<ObjectConstEntry>,
    /// HIR type of the object literal.
    pub ty: TypeId,
}

/// A TypeScript overload signature attached to a concrete implementation item.
#[derive(Debug, Clone)]
pub struct OverloadSignature {
    /// Parameter types in source order.
    pub params: Vec<TypeId>,
    /// Return type promised by this overload.
    pub return_ty: TypeId,
    /// Whether the signature describes an async function.
    pub is_async: bool,
}

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
    /// Exported object constants with literal data values.
    pub object_consts: HashMap<String, ObjectConst>,
    /// Function overload signatures visible to later TypeScript modules.
    pub overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Structural fields attached to type aliases visible to later modules.
    pub type_alias_fields: HashMap<smelt_hir::Symbol, Vec<Field>>,
    /// Structural fields attached to callable intersection types.
    pub callable_fields: HashMap<TypeId, Vec<Field>>,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
            export_aliases: HashMap::new(),
            object_namespaces: HashMap::new(),
            object_consts: HashMap::new(),
            overloads: HashMap::new(),
            type_alias_fields: HashMap::new(),
            callable_fields: HashMap::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
