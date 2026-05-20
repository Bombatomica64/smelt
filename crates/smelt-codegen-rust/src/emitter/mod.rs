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
use literals::operand_local;
use smelt_hir::{FileId, Span, Symbol, Type, TypeId};
use smelt_mir::{
    BasicBlock, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, Mir,
    MirClosure, MirFunction, MirListSpliceItem, Operand, Place, Rvalue, Statement, Terminator,
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
    pub(crate) none_ty: TypeId,
    /// Rust function names keyed by MIR function ID.
    function_names: HashMap<FuncId, String>,
    /// Emitted parameter types keyed by Rust function name.
    function_param_types: HashMap<String, Vec<TypeId>>,
    /// Emitted return types keyed by Rust function name.
    function_return_types: HashMap<String, TypeId>,
    /// Whether an emitted Rust function can throw and therefore returns `Result`.
    function_can_throw: HashMap<String, bool>,
    /// First emitted Rust name keyed by source callback symbol.
    callback_names: HashMap<Symbol, String>,
    /// Function parameters that must use an owned callback handle.
    owned_callback_params: HashSet<(FuncId, LocalId)>,
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
        let mut function_return_types = HashMap::new();
        let mut function_param_type_priorities = HashMap::<String, u8>::new();
        let mut function_can_throw = HashMap::new();
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
            let priority = emitted_signature_priority(function);
            if function_param_type_priorities
                .get(&rust_name)
                .copied()
                .is_none_or(|existing| priority >= existing)
            {
                function_param_types.insert(rust_name.clone(), params);
                function_return_types.insert(rust_name.clone(), function.return_ty);
                function_param_type_priorities.insert(rust_name.clone(), priority);
            }
            function_can_throw.insert(rust_name.clone(), function.can_throw);
            function_names.insert(function.id, rust_name);
        }
        let owned_callback_params = compute_owned_callback_params(mir)?;

        Ok(Self {
            none_ty,
            function_names,
            function_param_types,
            function_return_types,
            function_can_throw,
            callback_names,
            owned_callback_params,
        })
    }
}

/// Return precedence for cross-module emitted ABI signatures.
///
/// Manifest builds can contain imported or test-local function entries that
/// resolve to the same Rust symbol as the real source function. Calls must be
/// adapted to the concrete Rust function definition, so body-backed non-test
/// functions win over test/import-like entries when the registry sees the same
/// emitted name more than once.
fn emitted_signature_priority(function: &MirFunction) -> u8 {
    match (function.is_test, function.origin) {
        (false, HirOrigin::Body(_)) => 2,
        (false, _) => 1,
        (true, _) => 0,
    }
}

/// Computes which callback parameters need owned `Rc<RefCell<dyn FnMut...>>`.
///
/// A callback parameter needs ownership if it escapes its defining function, or
/// if it is forwarded to another function parameter that itself needs
/// ownership. The latter is a fixpoint because helper functions often pass
/// callbacks through multiple layers before one layer returns or stores them.
fn compute_owned_callback_params(mir: &Mir) -> Result<HashSet<(FuncId, LocalId)>, EmitError> {
    let mut owned = HashSet::new();
    for function in &mir.functions {
        for param in &function.params {
            let local = function
                .locals
                .get(id_index(param.0, "local index does not fit usize")?)
                .ok_or_else(|| EmitError::new("function parameter references unknown local"))?;
            if !matches!(mir.types.get(local.ty), Some(Type::Function(_))) {
                continue;
            }
            if callback_param_escapes_locally(mir, function, *param)? {
                owned.insert((function.id, *param));
            }
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for function in &mir.functions {
            for block in &function.blocks {
                let Some(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args,
                    ..
                }) = &block.terminator
                else {
                    continue;
                };
                let Some(callee) = mir
                    .functions
                    .get(id_index(callee_id.0, "function index does not fit usize")?)
                else {
                    continue;
                };
                let closure_defs = closure_definitions(function)?;
                for (arg, callee_param) in args.iter().zip(callee.params.iter()) {
                    if !owned.contains(&(callee.id, *callee_param)) {
                        continue;
                    }
                    let Some(arg_local) = operand_local(arg) else {
                        continue;
                    };
                    if function.params.contains(&arg_local)
                        && owned.insert((function.id, arg_local))
                    {
                        changed = true;
                    }
                    if let Some(closure_id) = closure_defs.get(&arg_local)
                        && let Some(closure) = mir
                            .closures
                            .get(id_index(closure_id.0, "closure index does not fit usize")?)
                    {
                        for capture in &closure.captures {
                            if function.params.contains(&capture.source_local)
                                && owned.insert((function.id, capture.source_local))
                            {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(owned)
}

/// Returns whether a callback parameter escapes inside its own function body.
fn callback_param_escapes_locally(
    mir: &Mir,
    function: &MirFunction,
    local: LocalId,
) -> Result<bool, EmitError> {
    let directly_returned = function.blocks.iter().any(|block| {
        matches!(
            &block.terminator,
            Some(Terminator::Return(Operand::Copy(place) | Operand::Move(place)))
                if *place == Place::Local(local)
        )
    });
    let closure_ids = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match statement {
            Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            }
            | Statement::AssignPlace {
                value: Rvalue::Closure { id, .. },
                ..
            } => Some(*id),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let captured_by_any_closure = mir
        .closures
        .iter()
        .filter(|closure| closure_ids.contains(&closure.id))
        .any(|closure| {
            closure
                .captures
                .iter()
                .any(|capture| capture.source_local == local)
        });
    Ok(directly_returned
        || type_contains_function(mir, function.return_ty)
        || (matches!(mir.types.get(function.return_ty), Some(Type::Function(_)))
            && captured_by_any_closure)
        || mir.closures.iter().any(|closure| {
            closure_ids.contains(&closure.id)
                && closure.escapes
                && closure
                    .captures
                    .iter()
                    .any(|capture| capture.source_local == local)
        }))
}

/// Return closure definitions in a function keyed by destination local.
fn closure_definitions(
    function: &MirFunction,
) -> Result<HashMap<LocalId, smelt_mir::ClosureId>, EmitError> {
    let mut definitions = HashMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } => {
                    definitions.insert(*dest, *id);
                }
                Statement::Assign {
                    dest,
                    value: Rvalue::Use(Operand::Copy(place) | Operand::Move(place)),
                } => {
                    if let Place::Local(source) = place
                        && let Some(id) = definitions.get(source).copied()
                    {
                        definitions.insert(*dest, id);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(definitions)
}

/// Return whether a type recursively contains a function value.
fn type_contains_function(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Function(_)) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_contains_function(mir, *item)
        }
        Some(Type::Dict(key, value)) => {
            type_contains_function(mir, *key) || type_contains_function(mir, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => {
            items.iter().any(|item| type_contains_function(mir, *item))
        }
        _ => false,
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
    /// Captured callback names that are emitted as borrowed `FnMut` values.
    borrowed_callback_names: HashSet<String>,
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
