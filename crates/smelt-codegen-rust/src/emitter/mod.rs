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
    BasicBlock, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, Mir,
    MirFunction, MirListSpliceItem, Operand, Place, Rvalue, Statement, Terminator,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

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

/// Precomputed crate-level codegen facts shared by all function emitters.
pub(crate) struct EmitContext {
    /// The type ID of the None type.
    none_ty: TypeId,
    /// Rust function names keyed by MIR function ID.
    function_names: HashMap<FuncId, String>,
    /// Emitted parameter types keyed by Rust function name.
    function_param_types: HashMap<String, Vec<TypeId>>,
    /// First emitted Rust name keyed by source callback symbol.
    callback_names: HashMap<Symbol, String>,
}

impl EmitContext {
    /// Builds crate-level name and signature indexes for Rust emission.
    pub(crate) fn new(mir: &Mir) -> Result<Self, EmitError> {
        let none_ty = mir
            .types
            .all()
            .iter()
            .enumerate()
            .find_map(|(id, ty)| {
                (*ty == Type::None)
                    .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
            })
            .transpose()?
            .ok_or_else(|| EmitError::new("MIR is missing the None type"))?;
        let mut duplicate_counts = HashMap::<Symbol, usize>::new();
        for function in &mir.functions {
            if !matches!(
                function.origin,
                HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
            ) {
                let count = duplicate_counts.entry(function.name).or_insert(0usize);
                *count = count.saturating_add(1);
            }
        }

        let mut function_names = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut callback_names = HashMap::new();
        for function in &mir.functions {
            let source_name = mir
                .symbols
                .get(function.name)
                .ok_or_else(|| EmitError::new("function has unknown symbol"))?;
            let base = sanitize_ident(source_name);
            let rust_name =
                if !function.is_test && source_name == "main" && function.return_ty == none_ty {
                    base
                } else if duplicate_counts.get(&function.name).copied().unwrap_or(0) > 1
                    || source_name.starts_with("__smelt_module_")
                {
                    format!("{}_{}", base, function.id.0)
                } else {
                    base
                };
            let params = function
                .params
                .iter()
                .map(|param| {
                    function
                        .locals
                        .get(id_index(param.0, "local index does not fit usize")?)
                        .map(|local| local.ty)
                        .ok_or_else(|| {
                            EmitError::new("function parameter references unknown local")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            callback_names
                .entry(function.name)
                .or_insert_with(|| rust_name.clone());
            function_param_types.insert(rust_name.clone(), params);
            function_names.insert(function.id, rust_name);
        }

        Ok(Self {
            none_ty,
            function_names,
            function_param_types,
            callback_names,
        })
    }
}

/// Emits Rust source for one MIR function.
pub(crate) struct FunctionEmitter<'mir> {
    /// Reference to the MIR.
    mir: &'mir Mir,
    /// Shared crate-level emission indexes.
    context: &'mir EmitContext,
    /// The function being emitted.
    function: &'mir MirFunction,
    /// Mapping from local IDs to variable names.
    names: HashMap<LocalId, String>,
    /// Set of locals that are mutated.
    mutable_locals: HashSet<LocalId>,
    /// Locals that have already been introduced in the generated Rust scope.
    declared_locals: RefCell<HashSet<LocalId>>,
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
