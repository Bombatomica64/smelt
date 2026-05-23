//! HIR construction context for TypeScript frontend lowering.

use std::collections::{HashMap, HashSet};

use crate::lowering::{ConstCollection, RestParam};
use smelt_hir::{Crate as HirCrate, ExprKind, Field, FunctionType, ItemId, Literal, TypeId};

/// Reusable value stored in a static object constant.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ObjectConstValue {
    /// Primitive literal value.
    Literal(Literal),
    /// Capturable HIR expression value.
    Expr(ExprKind),
}

/// A static object-constant entry that can be recreated in later lowered bodies.
#[derive(Debug, Clone)]
pub struct ObjectConstEntry {
    /// Source object key.
    pub key: String,
    /// Value stored under the key.
    pub value: ObjectConstValue,
    /// HIR type of the value.
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
    /// Source index of a rest parameter, when this overload declares one.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
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
    /// Exported item names grouped by source module path.
    pub module_exports: HashMap<String, HashMap<String, ItemId>>,
    /// Exported object constants used as namespace-like API surfaces.
    pub object_namespaces: HashMap<String, HashMap<String, ItemId>>,
    /// Exported object constants with literal data values.
    pub object_consts: HashMap<String, ObjectConst>,
    /// Exported object constants whose values can be projected by `Object.values`.
    pub object_value_collections: HashMap<String, ConstCollection>,
    /// Exported array/set constants that can be inlined by later modules.
    pub const_collections: HashMap<String, ConstCollection>,
    /// Function overload signatures visible to later TypeScript modules.
    pub overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Rest-parameter metadata visible to later TypeScript modules.
    pub function_rests: HashMap<String, RestParam>,
    /// Structural fields attached to type aliases visible to later modules.
    pub type_alias_fields: HashMap<smelt_hir::Symbol, Vec<Field>>,
    /// Interface heritage clauses visible to later modules for lazy field lookup.
    pub interface_extends: HashMap<smelt_hir::Symbol, Vec<crate::lowering::InterfaceHeritageRef>>,
    /// Value types declared by interface string index signatures.
    pub interface_index_values: HashMap<smelt_hir::Symbol, TypeId>,
    /// Interface call signatures visible to later modules.
    pub interface_call_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Structural fields attached to callable intersection types.
    pub callable_fields: HashMap<TypeId, Vec<Field>>,
    /// Type aliases whose source surface is a callable object intersection.
    pub callable_object_aliases: HashSet<smelt_hir::Symbol>,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
            export_aliases: HashMap::new(),
            module_exports: HashMap::new(),
            object_namespaces: HashMap::new(),
            object_consts: HashMap::new(),
            object_value_collections: HashMap::new(),
            const_collections: HashMap::new(),
            overloads: HashMap::new(),
            function_rests: HashMap::new(),
            type_alias_fields: HashMap::new(),
            interface_extends: HashMap::new(),
            interface_index_values: HashMap::new(),
            interface_call_signatures: HashMap::new(),
            callable_fields: HashMap::new(),
            callable_object_aliases: HashSet::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
}
