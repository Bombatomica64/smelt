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
    MirClass, MirClosure, MirDescriptor, MirField, MirFunction, MirListSpliceItem, Operand, Place,
    Rvalue, Statement, Terminator,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

mod binary_ops;
mod call;
mod call_runtime;
mod capture_analysis;
mod cfg_queries;
mod coercion;
mod control_flow;
mod control_flow_match;
mod core;
mod list;
mod list_mutation;
mod list_ordering;
mod list_query;
pub(crate) mod literals;
mod local_analysis;
mod map;
mod numeric;
mod optional_access;
mod place;
mod rendered_value;
mod set;
mod strings;
mod strings_io;
mod tuple;
mod types;
mod union;

use literals::{assigned_locals, constant_text, method_mutates_this};
use rendered_value::{Precedence, RenderedValue};
pub(crate) use union::emit_union_definitions;

/// Precomputed crate-level codegen facts shared by all function emitters.
pub(crate) struct EmitContext {
    /// The type ID of the None type.
    pub(crate) none_ty: TypeId,
    /// Whether emitted native tests must isolate mutable `Date.now()` mock state.
    needs_date_now_runtime: bool,
    /// Whether emitted native tests must isolate virtual timer runtime state.
    needs_timer_helpers: bool,
    /// Rust function names keyed by MIR function ID.
    function_names: HashMap<FuncId, String>,
    /// Emitted parameter types keyed by Rust function name.
    function_param_types: HashMap<String, Vec<TypeId>>,
    /// Emitted return types keyed by Rust function name.
    function_return_types: HashMap<String, TypeId>,
    /// Function parameters that must use an owned callback handle.
    owned_callback_params: HashSet<(FuncId, LocalId)>,
    /// Per-function-item value accessors collected while erasing
    /// function-item-as-value wrappers to `SmeltUnknown`.
    ///
    /// Maps a crate-unique function item key to the self-contained accessor body
    /// expression that builds its erased `SmeltUnknown::Function`. After the
    /// function loop the crate emitter flushes one `__smelt_fn_value_<key>()`
    /// accessor per entry, each caching a single shared erased value so repeated
    /// references to the same named function keep JavaScript reference identity.
    /// A `BTreeMap` keeps the emitted order deterministic for golden tests.
    pub(crate) function_item_accessors:
        ::std::cell::RefCell<::std::collections::BTreeMap<usize, String>>,
    /// Per-function-item accessors for the CONCRETE `SmeltErasedFunction` type
    /// (as opposed to the `SmeltUnknown` value held in `function_item_accessors`).
    ///
    /// A nullary function-item constant lowered to a typed `SmeltErasedFunction`
    /// context (e.g. Remeda's `doNothing`/`constant`) would otherwise build a
    /// FRESH `SmeltErasedFunction` — and thus a fresh callback `Rc` — on every
    /// call, so two calls never satisfy `Rc::ptr_eq`. JavaScript instead returns
    /// the same function singleton. Routing the build through a per-item
    /// `__smelt_fn_erased_<key>()` accessor (caching one `SmeltErasedFunction`)
    /// makes every call return clones that share one inner callback `Rc`. Maps a
    /// crate-unique function item key to the self-contained `SmeltErasedFunction
    /// { .. }` factory expression. A `BTreeMap` keeps emitted order deterministic.
    pub(crate) function_item_erased_fn_accessors:
        ::std::cell::RefCell<::std::collections::BTreeMap<usize, String>>,
    /// MIR ids of free functions that emit real Rust generics.
    ///
    /// A generic free function only keeps real generics when its signature is
    /// generic-safe AND its body keeps every type parameter opaque (the body
    /// trial in [`FunctionEmitter::emit`]). This decision must be identical at
    /// the definition and at every call site — otherwise a call would pass an
    /// argument through concretely to a parameter that was actually emitted as
    /// erased `SmeltUnknown`. The set is computed once by
    /// [`EmitContext::populate_generic_functions`] and read through
    /// [`EmitContext::is_generic_function`].
    generic_functions: RefCell<HashSet<FuncId>>,
    /// Class symbols emitted as reference classes (handle newtype over
    /// `Rc<RefCell<Inner>>`), computed once by [`crate::classify`]. Every class
    /// not in this set stays a by-value value class with its current emission.
    reference_classes: HashSet<Symbol>,
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
            let priority = emitted_signature_priority(function);
            if function_param_type_priorities
                .get(&rust_name)
                .copied()
                .is_none_or(|existing| priority > existing)
            {
                function_param_types.insert(rust_name.clone(), params);
                function_return_types.insert(rust_name.clone(), function.return_ty);
                function_param_type_priorities.insert(rust_name.clone(), priority);
            }
            function_names.insert(function.id, rust_name);
        }
        let owned_callback_params = compute_owned_callback_params(mir)?;

        Ok(Self {
            none_ty,
            needs_date_now_runtime: crate::stdlib::needs_date_now_runtime(mir),
            needs_timer_helpers: crate::needs_timer_helpers(mir),
            function_names,
            function_param_types,
            function_return_types,
            owned_callback_params,
            function_item_accessors: ::std::cell::RefCell::new(
                ::std::collections::BTreeMap::new(),
            ),
            function_item_erased_fn_accessors: ::std::cell::RefCell::new(
                ::std::collections::BTreeMap::new(),
            ),
            generic_functions: RefCell::new(HashSet::new()),
            reference_classes: crate::classify::reference_classes(mir),
        })
    }

    /// Return whether the class named `symbol` is emitted as a reference class.
    ///
    /// Reference classes use the handle-newtype representation with interior
    /// mutability; value classes keep the current by-value struct emission.
    pub(crate) fn is_reference_class(&self, symbol: Symbol) -> bool {
        self.reference_classes.contains(&symbol)
    }

    /// Compute, once, which free functions emit real Rust generics.
    ///
    /// Must be called after [`EmitContext::new`] and before any function is
    /// emitted. For each free function whose signature is generic-safe, this
    /// trial-renders the body with the type parameters in scope; the function
    /// keeps real generics only when the rendered body does not need the erased
    /// carrier (see `FunctionEmitter::renders_real_generics`). The result is
    /// shared by the definition and all call sites through
    /// [`EmitContext::is_generic_function`].
    pub(crate) fn populate_generic_functions(&self, mir: &Mir) -> Result<(), EmitError> {
        let mut generic = HashSet::new();
        for function in &mir.functions {
            if !matches!(function.origin, HirOrigin::Body(_)) {
                continue;
            }
            if !crate::classes::function_emits_rust_generics(mir, function) {
                continue;
            }
            let emitter = FunctionEmitter::new(mir, self, function)?;
            if emitter.renders_real_generics()? {
                generic.insert(function.id);
            }
        }
        *self.generic_functions.borrow_mut() = generic;
        Ok(())
    }

    /// Return whether the free function with `id` emits real Rust generics.
    pub(crate) fn is_generic_function(&self, id: FuncId) -> bool {
        self.generic_functions.borrow().contains(&id)
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

/// Computes which callback parameters need owned reentrant `Rc<dyn Fn...>` handles.
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
                    if !owned.contains(&(callee.id, *callee_param))
                        && !callee_param_is_owned_callback_sink(mir, callee, *callee_param)
                    {
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

/// Return whether a callee parameter is a position that always stores an owned
/// callback handle, so any callback argument passed into it must itself be owned.
///
/// An `Optional<fn(..)>` (and any container-wrapped function) parameter cannot be
/// emitted as a borrowed `&dyn Fn`: it lowers to `Option<Rc<dyn Fn>>` (an owned,
/// `'static` handle). Passing a borrowed callback parameter straight into such a
/// sink forces codegen to wrap it in an escaping `Rc::new(move || borrowed(..))`
/// adapter, and a non-`'static` borrow cannot satisfy the `Rc`'s `'static` bound
/// (borrowck "lifetime may not live long enough"). Treating these sinks as
/// ownership requirements propagates ownership back to the caller's parameter so
/// it is emitted as an owned `Rc<dyn Fn>` in the first place. A bare `Type::Function`
/// sink is handled separately through the `owned` set (it may still be borrowed).
fn callee_param_is_owned_callback_sink(
    mir: &Mir,
    callee: &MirFunction,
    param: LocalId,
) -> bool {
    let Ok(index) = id_index(param.0, "local index does not fit usize") else {
        return false;
    };
    let Some(local) = callee.locals.get(index) else {
        return false;
    };
    match mir.types.get(local.ty) {
        Some(Type::Optional(inner)) => {
            matches!(mir.types.get(*inner), Some(Type::Function(_)))
        }
        _ => false,
    }
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
    let closure_defs = closure_definitions(function)?;
    let captured_by_erased_closure_value = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            let Statement::Assign { dest, value } = statement else {
                return false;
            };
            let Some(dest_ty) = id_index(dest.0, "local index does not fit usize")
                .ok()
                .and_then(|index| function.locals.get(index))
                .map(|decl| decl.ty)
            else {
                return false;
            };
            if !type_erases_values(mir, dest_ty) {
                return false;
            }
            let maybe_source_local = match value {
                Rvalue::Use(
                    Operand::Copy(Place::Local(source) | Place::Field { base: source, .. })
                    | Operand::Move(Place::Local(source) | Place::Field { base: source, .. }),
                ) => Some(*source),
                _ => None,
            };
            let Some(source_local) = maybe_source_local else {
                return false;
            };
            let Some(closure_id) = closure_defs.get(&source_local) else {
                return false;
            };
            mir.closures
                .get(
                    id_index(closure_id.0, "closure index does not fit usize")
                        .unwrap_or(usize::MAX),
                )
                .is_some_and(|closure| {
                    closure
                        .captures
                        .iter()
                        .any(|capture| capture.source_local == local)
                })
        })
    });
    let captured_by_erased_return = type_erases_values(mir, function.return_ty)
        && function.blocks.iter().any(|block| {
            let Some(Terminator::Return(
                Operand::Copy(Place::Local(returned)) | Operand::Move(Place::Local(returned)),
            )) = &block.terminator
            else {
                return false;
            };
            closure_source_for_local(function, *returned, Some(&closure_defs))
                .and_then(|closure_id| {
                    mir.closures.get(
                        id_index(closure_id.0, "closure index does not fit usize")
                            .unwrap_or(usize::MAX),
                    )
                })
                .is_some_and(|closure| {
                    closure
                        .captures
                        .iter()
                        .any(|capture| capture.source_local == local)
                })
        });
    let erased_or_dynamic_escape = function.blocks.iter().any(|block| {
        block
            .statements
            .iter()
            .any(|statement| statement_erases_callback_param(mir, function, statement, local))
    });
    // A callback parameter that is rebound in the body (`callback = …`, e.g. the
    // `mapAsync`/`filterAsync` concurrency wrappers reassign their callback to a
    // `limitAsync`-wrapped handle) receives an owned `Rc<dyn Fn…>` on the right.
    // A borrowed `&dyn Fn` binding cannot hold that owned value, so the parameter
    // must enter the function as an owned handle.
    let rebound_locally = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| match statement {
            Statement::Assign { dest, .. } => *dest == local,
            Statement::AssignPlace {
                place: Place::Local(candidate),
                ..
            } => *candidate == local,
            _ => false,
        })
    });
    // A callback passed as the function argument of a timer async op
    // (`setTimeout`/`setInterval`) is wrapped by the backend into a `'static`
    // timer closure (see the emitter's timer callback lowering). A borrowed
    // `&dyn Fn` cannot escape into that `'static` closure, so the parameter must
    // enter the function as an owned handle. Mirrors es-toolkit `delay`.
    let escapes_into_timer = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            let value = match statement {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => value,
                _ => return false,
            };
            matches!(
                value,
                Rvalue::AsyncOp {
                    op: smelt_hir::AsyncOp::SetTimeout | smelt_hir::AsyncOp::SetInterval,
                    args,
                } if matches!(
                    args.first(),
                    Some(Operand::Copy(Place::Local(callback)) | Operand::Move(Place::Local(callback)))
                        if *callback == local
                )
            )
        })
    });
    // An async function runs its entire body inside the returned future, so any
    // callback parameter it references is retained past the caller's statement
    // (the future outlives the call expression). A borrowed `&dyn Fn` parameter
    // cannot survive that, and callers passing `&*(temporary)` would drop the
    // temporary at the end of the call statement (E0716/E0515). Async
    // function-typed params must therefore enter as owned `Rc<dyn Fn…>` handles.
    let retained_by_async_body = function.is_async;
    Ok(directly_returned
        || retained_by_async_body
        || rebound_locally
        || escapes_into_timer
        || erased_or_dynamic_escape
        || captured_by_erased_closure_value
        || captured_by_erased_return
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

/// Return whether a statement may put a callback parameter behind erased state.
///
/// `SmeltUnknown::Function` is stored as a `'static` callable handle in the
/// generated runtime. If a source callback parameter is wrapped into unknown
/// state, or passed to an erased closure-call result, the parameter cannot stay
/// as `&dyn Fn`; it must enter the function as an owned handle.
fn statement_erases_callback_param(
    mir: &Mir,
    function: &MirFunction,
    statement: &Statement,
    local: LocalId,
) -> bool {
    let Statement::Assign {
        dest,
        value: statement_value,
    } = statement
    else {
        return false;
    };
    let dest_ty = id_index(dest.0, "local index does not fit usize")
        .ok()
        .and_then(|dest_index| function.locals.get(dest_index))
        .map(|decl| decl.ty);
    let function_erases_return = type_erases_values(mir, function.return_ty);
    let closure_defs = closure_definitions(function).ok();
    match statement_value {
        Rvalue::Dict(entries) => {
            (function_erases_return || dest_ty.is_some_and(|ty| type_erases_values(mir, ty)))
                && entries.iter().any(|(_, entry_value)| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        entry_value,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            (function_erases_return || dest_ty.is_some_and(|ty| type_erases_values(mir, ty)))
                && items.iter().any(|item| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        item,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::ClosureCall { args, .. } => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && args.iter().any(|arg| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        arg,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::CallableObjectAssign { callable, props } => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && (operand_local(callable) == Some(local)
                    || operand_local(callable)
                        .and_then(|callable_local| {
                            closure_source_for_local(
                                function,
                                callable_local,
                                closure_defs.as_ref(),
                            )
                        })
                        .and_then(|closure_id| {
                            mir.closures.get(
                                id_index(closure_id.0, "closure index does not fit usize").ok()?,
                            )
                        })
                        .is_some_and(|closure| {
                            closure
                                .captures
                                .iter()
                                .any(|capture| capture.source_local == local)
                        })
                    || props
                        .iter()
                        .any(|(_, prop_value)| operand_local(prop_value) == Some(local)))
        }
        Rvalue::Use(operand) => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && operand_local(operand) == Some(local)
        }
        _ => false,
    }
}

/// Return whether an erased operand is a callback parameter or a closure capturing it.
fn operand_refs_callback_param_or_capturing_closure(
    mir: &Mir,
    function: &MirFunction,
    operand: &Operand,
    local: LocalId,
    closure_defs: Option<&HashMap<LocalId, smelt_mir::ClosureId>>,
) -> bool {
    if operand_local(operand) == Some(local) {
        return true;
    }
    operand_local(operand)
        .and_then(|operand_local| closure_source_for_local(function, operand_local, closure_defs))
        .and_then(|closure_id| {
            mir.closures
                .get(id_index(closure_id.0, "closure index does not fit usize").ok()?)
        })
        .is_some_and(|closure| {
            closure
                .captures
                .iter()
                .any(|capture| capture.source_local == local)
        })
}

/// Resolve a local through simple copy aliases to the closure assigned to it.
fn closure_source_for_local(
    function: &MirFunction,
    local: LocalId,
    closure_defs: Option<&HashMap<LocalId, smelt_mir::ClosureId>>,
) -> Option<smelt_mir::ClosureId> {
    let closure_definitions = closure_defs?;
    if let Some(closure_id) = closure_definitions.get(&local).copied() {
        return Some(closure_id);
    }
    let mut current = local;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let next = function.blocks.iter().find_map(|block| {
            block.statements.iter().find_map(|statement| {
                let Statement::Assign { dest, value } = statement else {
                    return None;
                };
                if *dest != current {
                    return None;
                }
                match value {
                    Rvalue::Use(
                        Operand::Copy(Place::Local(source) | Place::Field { base: source, .. })
                        | Operand::Move(Place::Local(source) | Place::Field { base: source, .. }),
                    ) => Some(*source),
                    _ => None,
                }
            })
        })?;
        if let Some(closure_id) = closure_definitions.get(&next).copied() {
            return Some(closure_id);
        }
        current = next;
    }
    None
}

/// Return whether a Rust value of `ty` erases nested values into unknown state.
fn type_erases_values(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_erases_values(mir, *item)
        }
        Some(Type::Dict(key, value)) => {
            type_erases_values(mir, *key) || type_erases_values(mir, *value)
        }
        Some(Type::Tuple(items)) => items.iter().any(|item| type_erases_values(mir, *item)),
        _ => false,
    }
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
    /// Locals that must be declared before structured block emission.
    predeclared_locals: HashSet<LocalId>,
    /// Cached termination queries for this function CFG.
    termination_cache: RefCell<HashMap<smelt_mir::BlockId, bool>>,
    /// Cached loop-exit shape queries keyed by block, continue target, and break target.
    loop_exit_cache:
        RefCell<HashMap<(smelt_mir::BlockId, smelt_mir::BlockId, smelt_mir::BlockId), bool>>,
    /// Captured callback names that are emitted as borrowed `Fn` values.
    borrowed_callback_names: HashSet<String>,
    /// Record types currently being wrapped or extracted through erased objects.
    ///
    /// Callback-bearing records can contain their own option type again, so
    /// this bounds structural expansion of cyclic TypeScript object shapes.
    record_conversion_stack: RefCell<Vec<TypeId>>,
    /// The type ID of the None type.
    none_ty: TypeId,
    /// Synthetic unknown local used when malformed MIR references a missing local.
    unknown_local: LocalDecl,
    /// When set, a generic free function's own type parameters are treated as
    /// out of scope (erased to `SmeltUnknown`) even though the function declares
    /// them.
    ///
    /// A generic free function only emits real Rust generics when its body keeps
    /// each type parameter opaque. The emitter trial-renders the body with the
    /// type parameters in scope; if the rendered body still references the erased
    /// `SmeltUnknown` carrier (the body inspects, compares, or erases a
    /// `T`-typed value), emission falls back to the fully erased signature by
    /// setting this flag and re-rendering. See [`FunctionEmitter::emit`].
    suppress_type_params: RefCell<bool>,
    /// Type parameters that are in scope in the enclosing Rust output but are
    /// not declared by this emitter's own function.
    ///
    /// A closure is emitted inline inside its enclosing function, so any generic
    /// parameters that function declares (`fn difference<T>`) are visible to the
    /// closure body in the generated Rust. The closure is rendered through a
    /// *separate* synthetic [`MirFunction`] with no type parameters of its own,
    /// so without this the sub-emitter would erase every `T`-typed closure
    /// parameter to `SmeltUnknown` and force the enclosing signature to erase too
    /// (see [`FunctionEmitter::current_function_type_params`]). The set is
    /// populated from the enclosing emitter's in-scope type parameters at
    /// construction time, so it is already gated by the enclosing function's
    /// erasure decision.
    enclosing_type_params: HashSet<Symbol>,
}

/// How to compute the default end bound for a slice.
#[derive(Clone, Copy)]
enum SliceLenKind {
    /// Use `.len()`.
    Len,
    /// Use `.chars().count()`.
    Chars,
}
