//! Internal Rust source emission helpers split by concept.
#![allow(
    clippy::wildcard_imports,
    reason = "emitter shards share a common private helper surface through the parent module"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the private emitter module still exposes helpers to its parent codegen module"
)]

use crate::{compact_index, id_index, sanitize_ident, EmitError};
use std::collections::{HashMap, HashSet};
use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{
    BasicBlock, BuiltinFn, Callee, Constant, HirOrigin, LocalId, LocalKind, Mir, MirFunction,
    Operand, Place, Rvalue, Statement, Terminator,
};

mod call;
mod call_runtime;
mod control_flow;
mod control_flow_match;
mod core;
mod list;
mod list_mutation;
mod list_query;
mod list_ordering;
mod literals;
mod map;
mod numeric;
mod place;
mod set;
mod strings;
mod strings_io;
mod tuple;
mod types;
mod unknown;

use literals::{
    assigned_locals, block_terminates, constant_text, hir_literal_text, method_mutates_this,
};

/// Emits Rust source for one MIR function.
pub(crate) struct FunctionEmitter<'mir> {
    /// Reference to the MIR.
    mir: &'mir Mir,
    /// The function being emitted.
    function: &'mir MirFunction,
    /// Mapping from local IDs to variable names.
    names: HashMap<LocalId, String>,
    /// Set of locals that are mutated.
    mutable_locals: HashSet<LocalId>,
    /// The type ID of the None type.
    none_ty: TypeId,
}

/// How to compute the default end bound for a slice.
#[derive(Clone, Copy)]
enum SliceLenKind {
    /// Use `.len()`.
    Len,
    /// Use `.chars().count()`.
    Chars,
}
