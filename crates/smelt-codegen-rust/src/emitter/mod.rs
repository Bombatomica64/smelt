//! Internal Rust source emission helpers split by concept.
#![allow(
    clippy::wildcard_imports,
    reason = "emitter shards share a common private helper surface through the parent module"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the private emitter module still exposes helpers to its parent codegen module"
)]

use crate::{EmitError, compact_index, id_index, sanitize_ident};
use smelt_hir::{FileId, Span, Symbol, Type, TypeId};
use smelt_mir::{
    BasicBlock, BuiltinFn, Callee, Constant, HirOrigin, LocalDecl, LocalId, LocalKind, Mir,
    MirFunction, MirListSpliceItem, Operand, Place, Rvalue, Statement, Terminator,
};
use std::collections::{HashMap, HashSet};

mod call;
mod call_runtime;
mod control_flow;
mod control_flow_match;
mod core;
mod list;
mod list_mutation;
mod list_ordering;
mod list_query;
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

use literals::{assigned_locals, constant_text, hir_literal_text, method_mutates_this};

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
    /// Synthetic unknown local used when malformed MIR references a missing local.
    unknown_local: LocalDecl,
}

/// How to compute the default end bound for a slice.
#[derive(Clone, Copy)]
enum SliceLenKind {
    /// Use `.len()`.
    Len,
    /// Use `.chars().count()`.
    Chars,
}
