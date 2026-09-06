//! Lowering from HIR to MIR.
//!
//! This module converts a HIR crate into a MIR representation. The entry point
//! [`lower_hir`] builds the MIR class/interface tables, lowers every function,
//! method, constructor, and module top-level body into a [`MirFunction`], and
//! then runs the whole-program closure/throwing finalization passes.
//!
//! Per-body lowering is driven by `LoweringCtx`, whose implementation is split
//! across focused submodules so the builder can be refactored incrementally:
//!
//! * [`context`] — the `LoweringCtx` state, its constructors, and the shared
//!   block/HIR accessor helpers.
//! * [`stmt`] — statement, block, and control-flow terminator lowering.
//! * [`expr`] — the exhaustive [`ExprKind`](smelt_hir::ExprKind) dispatcher and
//!   its call/temporary helpers.
//! * [`place`] — lvalue (assignment target) lowering.
//! * [`closures`] — closure construction, escape analysis, and throwing ABI
//!   widening.
//! * [`generics`] — interface field flattening and generic type substitution.
//! * [`passes`] — whole-program analyses that run after core lowering.

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use smelt_hir::{BodyId, Literal as HirLiteral, Span, Type, TypeId};

use crate::{
    BuiltinFn, Callee, ClosureId, Constant, FuncId, LocalId, Mir, MirClosure, MirFunction, Operand,
    Place, Terminator,
};

/// The `LoweringCtx` state machine, its constructors, and shared accessors.
mod context;
/// Expression lowering: the core `lower_expr` dispatcher and call helpers.
mod expr;
/// Interface field flattening and generic type-parameter substitution.
mod generics;
/// Lvalue (place) lowering for assignment targets.
mod place;
/// Statement, block, and control-flow terminator lowering.
mod stmt;

/// Closure ABI ownership: construction, escape analysis, and throwing widening.
mod closures;
/// Whole-program MIR analyses that run after core HIR lowering.
mod passes;

use context::{FunctionSpec, LoweringCtx, LoweringShared};
use generics::lower_effective_interface_fields;

/// Converts a `usize` into `u32`, panicking if it does not fit.
///
/// # Panics
///
/// Panics when `value` does not fit in `u32`.
fn u32_from_usize(value: usize, context: &str) -> Result<u32, LowerError> {
    u32::try_from(value).map_err(|error| LowerError {
        message: format!("{context}: {error}"),
        span: None,
    })
}

/// Converts a `u32` into `usize`, panicking if it does not fit.
///
/// # Panics
///
/// Panics when `value` does not fit in `usize`.
fn usize_from_u32(value: u32, context: &str) -> Result<usize, LowerError> {
    usize::try_from(value).map_err(|error| LowerError {
        message: format!("{context}: {error}"),
        span: None,
    })
}

/// Converts a local ID into an optional slice index.
fn local_index(local: LocalId) -> Option<usize> {
    usize::try_from(local.0).ok()
}

/// Converts a closure ID into an optional slice index.
fn closure_id_index(closure: ClosureId) -> Option<usize> {
    usize::try_from(closure.0).ok()
}

/// Finds the HIR function item that owns `body_id`, if any.
fn function_item_for_body(
    krate: &smelt_hir::Crate,
    body_id: BodyId,
) -> Option<&smelt_hir::Function> {
    krate.items.iter().find_map(|item| {
        let smelt_hir::Item::Function(function) = item else {
            return None;
        };
        (function.body == Some(body_id)).then_some(function)
    })
}

/// An error encountered during HIR to MIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LowerError {
    /// The error message.
    pub message: String,
    /// The source span associated with the error, if available.
    pub span: Option<Span>,
}

/// Function lowering output with any closures discovered inside the function body.
type LoweredFunction = (MirFunction, Vec<MirClosure>);

/// Mapping from HIR function-item ids to their assigned MIR [`FuncId`]s.
type FunctionIds = HashMap<smelt_hir::ItemId, FuncId>;

/// Original place and temporary local used for collection mutation writeback.
type MutationWriteback = Option<(Place, LocalId)>;

/// Lowered collection mutation receiver and its optional writeback.
type LoweredMutationReceiver = (Operand, MutationWriteback);


/// Synthetic MIR helper types shared by every body lowered in one run.
///
/// These are the interned types [`lower_hir`] pre-computes once so each
/// [`LoweringCtx`] can reuse them: the unit return type for module bodies and
/// the float/bool types used to synthesize `for`-loop counters and conditions.
#[derive(Clone, Copy)]
struct HelperTypes {
    /// The `None` (unit) type used as the return type of module top levels.
    none: TypeId,
    /// The float type used for generated index-based loop counters.
    loop_index_ty: TypeId,
    /// The bool type used for generated loop conditions.
    loop_bool_ty: TypeId,
}

/// Lowers a HIR crate to MIR, or returns lowering errors if the conversion fails.
///
/// # Errors
///
/// Returns a non-empty `Vec<LowerError>` when the crate cannot be lowered:
/// - the HIR crate fails [`smelt_hir::validate`] (each HIR validation error is
///   forwarded as a spanless [`LowerError`]);
/// - stable [`FuncId`]s cannot be assigned to the crate's function items;
/// - lowering an item function or module body fails (e.g. an index does not fit
///   in the target integer width, or a referenced local/block/type cannot be
///   resolved during lowering).
pub fn lower_hir(krate: &smelt_hir::Crate) -> Result<Mir, Vec<LowerError>> {
    let hir_errors = smelt_hir::validate(krate);
    if !hir_errors.is_empty() {
        return Err(hir_errors
            .into_iter()
            .map(|error| LowerError {
                message: error.message,
                span: None,
            })
            .collect());
    }

    let mut mir = Mir::new(
        krate.types.clone(),
        krate.symbols.clone(),
        krate.names.clone(),
    );
    let helpers = HelperTypes {
        none: mir.types.intern(Type::None),
        loop_index_ty: mir.types.intern(Type::Float),
        loop_bool_ty: mir.types.intern(Type::Bool),
    };
    let mut errors = Vec::new();

    let item_functions = assign_function_ids(krate)?;
    let global_ids = lower_globals(krate, &mut mir, &item_functions, &mut errors);
    let languages = index_source_languages(krate);
    let tables = LoweringTables {
        item_functions: &item_functions,
        global_ids: &global_ids,
        languages: &languages,
    };
    lower_type_tables(krate, &mut mir, &item_functions);
    lower_item_functions(krate, &mut mir, &tables, helpers, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    lower_module_bodies(krate, &mut mir, &tables, helpers, &mut errors);

    if errors.is_empty() {
        run_finalization_passes(&mut mir);
        Ok(mir)
    } else {
        Err(errors)
    }
}

/// Assign a stable [`FuncId`] to every HIR function item that has a body.
///
/// The map is built before any body is lowered so calls can resolve their
/// callees regardless of declaration order. Ids are dense, indexed by insertion
/// order, matching the order [`Mir::push_function`] later appends functions.
fn assign_function_ids(
    krate: &smelt_hir::Crate,
) -> Result<FunctionIds, Vec<LowerError>> {
    let mut item_functions = HashMap::new();
    for (idx, hir_item) in krate.items.iter().enumerate() {
        let item_id =
            smelt_hir::ItemId(u32_from_usize(idx, "HIR item index does not fit in u32").map_err(
                |error| vec![error],
            )?);
        if let smelt_hir::Item::Function(function) = hir_item
            && function.body.is_some()
        {
            let function_id = u32_from_usize(
                item_functions.len(),
                "MIR function index does not fit in u32",
            )
            .map_err(|error| vec![error])?;
            item_functions.insert(item_id, FuncId(function_id));
        }
    }
    Ok(item_functions)
}

/// Item-resolution tables shared by every body lowering.
///
/// Bundles the function-id map from [`assign_function_ids`] with the
/// mutable-global index map from [`lower_globals`] so body lowering entry
/// points take one resolution frame instead of parallel table arguments.
struct LoweringTables<'hir> {
    /// Mapping of HIR function items to their assigned MIR function ids.
    item_functions: &'hir FunctionIds,
    /// Mapping of mutable-global HIR items to their `Mir::globals` index.
    global_ids: &'hir HashMap<smelt_hir::ItemId, u32>,
    /// Source language of every file in the crate, keyed by the `FileId` that
    /// each expression's span carries.
    languages: &'hir HashMap<smelt_hir::FileId, smelt_hir::Language>,
}

/// Index every module's source language by the `FileId` its spans carry.
///
/// A crate is not single-language: the transpiler picks a frontend per file, so
/// TypeScript and Python modules can sit in one [`smelt_hir::Crate`]. Semantics
/// that differ between the two -- negative element indexing, for one -- must be
/// decided per site, and an expression identifies its file through its span.
fn index_source_languages(
    krate: &smelt_hir::Crate,
) -> HashMap<smelt_hir::FileId, smelt_hir::Language> {
    krate
        .modules
        .iter()
        .map(|module| (module.source.file, module.source.language))
        .collect()
}

/// Populate [`Mir::globals`] from every HIR mutable-global item.
///
/// Scans crate items in order; each [`smelt_hir::Item::MutableGlobal`] becomes
/// one [`crate::MirGlobal`] with its literal initializer lowered to a
/// [`Constant`]. Returns a map from the global's HIR [`smelt_hir::ItemId`] to
/// its dense index in `Mir::globals`, which `GlobalGet`/`GlobalSet` lowering
/// uses to resolve references.
fn lower_globals(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    item_functions: &FunctionIds,
    errors: &mut Vec<LowerError>,
) -> HashMap<smelt_hir::ItemId, u32> {
    let mut global_ids = HashMap::new();
    for (idx, item) in krate.items.iter().enumerate() {
        let smelt_hir::Item::MutableGlobal(global) = item else {
            continue;
        };
        let Ok(item_index) = u32_from_usize(idx, "HIR item index does not fit in u32") else {
            continue;
        };
        let Ok(global_index) = u32_from_usize(mir.globals.len(), "MIR global index does not fit in u32")
        else {
            continue;
        };
        // A `Pending` initializer means the frontend registered the global but
        // never reached its own declaration to lower the initializer
        // expression. That is a compiler bug, not a source problem, so it is
        // reported rather than defaulted to some value the source never wrote.
        let init = match &global.init {
            smelt_hir::MutableGlobalInit::Literal(literal) => {
                crate::MirGlobalInit::Constant(lower_literal(literal))
            }
            smelt_hir::MutableGlobalInit::Initializer(init_item) => {
                let Some(func_id) = item_functions.get(init_item) else {
                    errors.push(LowerError {
                        message: "mutable global initializer function has no MIR function id"
                            .to_owned(),
                        span: Some(global.span),
                    });
                    continue;
                };
                crate::MirGlobalInit::Call(*func_id)
            }
            smelt_hir::MutableGlobalInit::Pending => {
                errors.push(LowerError {
                    message: "mutable global initializer expression was never lowered".to_owned(),
                    span: Some(global.span),
                });
                continue;
            }
        };
        mir.globals.push(crate::MirGlobal {
            name: global.name,
            ty: global.ty,
            init,
        });
        global_ids.insert(smelt_hir::ItemId(item_index), global_index);
    }
    global_ids
}

/// Build the MIR class and interface tables from HIR items.
///
/// Class descriptors, methods, and constructors are resolved to their already
/// assigned [`FuncId`]s, and interface fields are flattened (with generic
/// substitution) so codegen sees a concrete record layout. Function, type
/// alias, and const items carry no table entry here.
fn lower_type_tables(krate: &smelt_hir::Crate, mir: &mut Mir, item_functions: &FunctionIds) {
    for hir_item in &krate.items {
        match hir_item {
            smelt_hir::Item::Class(class) => lower_class_table_entry(mir, class, item_functions),
            smelt_hir::Item::Interface(interface) => {
                lower_interface_table_entry(krate, mir, interface);
            }
            smelt_hir::Item::Function(_)
            | smelt_hir::Item::TypeAlias(_)
            | smelt_hir::Item::Const(_)
            // Mutable globals are collected separately by `lower_globals`.
            | smelt_hir::Item::MutableGlobal(_) => {}
        }
    }
}

/// Append one HIR class to the MIR class table.
///
/// Field, static-field, and descriptor layouts are copied directly; getters,
/// setters, the constructor, and methods are resolved from `item_functions` to
/// the [`FuncId`]s [`assign_function_ids`] allocated for them.
fn lower_class_table_entry(mir: &mut Mir, class: &smelt_hir::Class, item_functions: &FunctionIds) {
    mir.classes.push(crate::MirClass {
        name: class.name,
        kind: class.kind.clone(),
        type_params: class.type_params.clone(),
        is_abstract: matches!(class.kind, smelt_hir::ClassKind::Abstract),
        base: class.base,
        base_args: class.base_args.clone(),
        fields: class
            .fields
            .iter()
            .map(|field| crate::MirField {
                name: field.name,
                ty: field.ty,
                visibility: field.visibility,
            })
            .collect(),
        static_fields: class
            .static_fields
            .iter()
            .map(|field| crate::MirStaticField {
                name: field.name,
                ty: field.ty,
                visibility: field.visibility,
                value: field.value.clone(),
            })
            .collect(),
        descriptors: class
            .descriptors
            .iter()
            .map(|descriptor| crate::MirDescriptor {
                name: descriptor.name,
                read_ty: descriptor.read_ty,
                write_ty: descriptor.write_ty,
                getter: descriptor
                    .getter
                    .and_then(|item| item_functions.get(&item).copied()),
                setter: descriptor
                    .setter
                    .and_then(|item| item_functions.get(&item).copied()),
                data_descriptor: descriptor.data_descriptor,
                is_static: descriptor.is_static,
                value_fields: descriptor.value_fields.clone(),
            })
            .collect(),
        constructor: class
            .constructor
            .and_then(|item_id| item_functions.get(&item_id).copied()),
        methods: class
            .methods
            .iter()
            .filter_map(|method_item| item_functions.get(method_item).copied())
            .collect(),
        static_methods: class
            .static_methods
            .iter()
            .filter_map(|method_item| item_functions.get(method_item).copied())
            .collect(),
        abstract_methods: class.abstract_methods.clone(),
        implements: class
            .implements
            .iter()
            .map(|implemented| implemented.parent)
            .collect(),
        protocols: class
            .protocols
            .iter()
            .filter_map(|protocol| match protocol {
                smelt_hir::ClassProtocol::Add { method } => item_functions
                    .get(method)
                    .copied()
                    .map(|method| crate::MirClassProtocol::Add { method }),
            })
            .collect(),
    });
}

/// Append one HIR interface to the MIR interface table with fields flattened.
///
/// Inherited fields are folded in (with generic substitution) by
/// [`lower_effective_interface_fields`] so codegen sees a single concrete
/// record layout without retaining the frontend heritage chain.
fn lower_interface_table_entry(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    interface: &smelt_hir::Interface,
) {
    let fields =
        lower_effective_interface_fields(krate, mir, interface, &HashMap::new(), &mut HashSet::new());
    mir.interfaces.push(crate::MirInterface {
        name: interface.name,
        type_params: interface.type_params.clone(),
        extends: interface.extends.clone(),
        fields: fields
            .iter()
            .map(|field| crate::MirField {
                name: field.name,
                ty: field.ty,
                visibility: field.visibility,
            })
            .collect(),
        methods: interface.methods.clone(),
    });
}

/// Lower every HIR function item that has a body into a [`MirFunction`].
///
/// Async functions have their `Promise<T>` return type unwrapped to `T`.
/// Lowering stops at the first hard error (pushing it to `errors` and breaking)
/// because later functions may depend on state this one would have produced.
fn lower_item_functions(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    tables: &LoweringTables<'_>,
    helpers: HelperTypes,
    errors: &mut Vec<LowerError>,
) {
    for (idx, item) in krate.items.iter().enumerate() {
        let item_id = match u32_from_usize(idx, "HIR item index does not fit in u32") {
            Ok(value) => smelt_hir::ItemId(value),
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let smelt_hir::Item::Function(function) = item else {
            continue;
        };
        let Some(body_id) = function.body else {
            continue;
        };
        let Some(body) = krate.bodies.get(
            match usize_from_u32(body_id.0, "HIR body index does not fit in usize") {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            },
        ) else {
            continue;
        };
        let return_ty = if function.is_async {
            future_inner_type(krate, function.return_ty).unwrap_or(function.return_ty)
        } else {
            function.return_ty
        };
        let Some(function_id) = tables.item_functions.get(&item_id).copied() else {
            continue;
        };
        let shared = LoweringShared {
            krate,
            item_functions: tables.item_functions,
            global_ids: tables.global_ids,
            languages: tables.languages,
            loop_index_ty: helpers.loop_index_ty,
            loop_bool_ty: helpers.loop_bool_ty,
            closure_base: mir.closures.len(),
        };
        let spec = FunctionSpec {
            function_id,
            body_id,
            name: function.name,
            return_ty,
            owner: function.owner,
            is_async: function.is_async,
        };
        match LoweringCtx::new(shared, spec, body).and_then(LoweringCtx::lower) {
            Ok((lowered_function, closures)) => {
                mir.closures.extend(closures);
                mir.push_function(lowered_function);
            }
            Err(error) => {
                errors.push(error);
                break;
            }
        }
    }
}

/// Lower each module's top-level body into a synthetic module `MirFunction`.
///
/// Empty test-only module bodies are skipped so they do not produce an empty
/// `main`. Unlike item lowering, a failure here records the error and continues
/// to the next module rather than aborting.
fn lower_module_bodies(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    tables: &LoweringTables<'_>,
    helpers: HelperTypes,
    errors: &mut Vec<LowerError>,
) {
    for module in &krate.modules {
        let Some(body_id) = module.body else {
            continue;
        };
        if module_has_test_items(krate, module) && body_is_empty(krate, body_id) {
            continue;
        }
        let Some(body) = krate.bodies.get(
            match usize_from_u32(body_id.0, "HIR body index does not fit in usize") {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            },
        ) else {
            continue;
        };
        let name = mir.symbols.intern(&module.name);
        let function_id = mir.next_function_id();
        let shared = LoweringShared {
            krate,
            item_functions: tables.item_functions,
            global_ids: tables.global_ids,
            languages: tables.languages,
            loop_index_ty: helpers.loop_index_ty,
            loop_bool_ty: helpers.loop_bool_ty,
            closure_base: mir.closures.len(),
        };
        let spec = FunctionSpec {
            function_id,
            body_id,
            name,
            return_ty: helpers.none,
            owner: smelt_hir::FunctionOwner::Module,
            is_async: false,
        };
        match LoweringCtx::new(shared, spec, body).and_then(LoweringCtx::lower) {
            Ok((function, closures)) => {
                mir.closures.extend(closures);
                mir.push_function(function);
            }
            Err(error) => errors.push(error),
        }
    }
}

/// Run the whole-program closure/throwing finalization passes on lowered MIR.
///
/// Closure ABI finalization: `closures` owns escape analysis and the
/// throwing-closure type widening; the general function-throwing propagation in
/// `passes::throwing` feeds the widening, which is why the two interleave. See
/// [`closures`] for the invariants this publishes. Erased-record promotion and
/// operational type normalization then run once over the finalized MIR.
fn run_finalization_passes(mir: &mut Mir) {
    intern_fallible_builtin_return_types(mir);
    closures::mark_escaping_closures(mir);
    passes::throwing::propagate_throwing_functions(mir);
    closures::widen_throwing_closure_types(mir);
    passes::throwing::propagate_throwing_functions(mir);
    closures::widen_throwing_closure_types(mir);
    crate::promote_erased_mutated_records(mir);
    crate::normalize_operational_types(mir);
}

/// Interns the return type of every fallible builtin the program calls.
///
/// A `Terminator::Call` records the destination's type, not the callee's, so a
/// builtin whose own return type appears nowhere else in the program would
/// leave that type absent from the table — and the backend, which asks for the
/// callee's return type to coerce the call result, would find nothing.
///
/// There are two such builtins: `JSON.parse`, whose result is a dynamic
/// JavaScript value (`Type::Unknown`) regardless of the destination it is
/// asserted into, and the URI decoders, which answer a `String`. Collecting the
/// types first and interning afterwards keeps this a scan rather than a
/// hard-coded pair, so a third fallible builtin needs only its own arm.
fn intern_fallible_builtin_return_types(mir: &mut Mir) {
    let mut needed = Vec::new();
    for terminator in mir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .chain(mir.closures.iter().flat_map(|closure| closure.blocks.iter()))
        .filter_map(|block| block.terminator.as_ref())
    {
        if let Terminator::Call {
            callee: Callee::Builtin(builtin),
            ..
        } = terminator
        {
            match builtin {
                BuiltinFn::JsonParse => needed.push(Type::Unknown),
                BuiltinFn::UriDecode(_) => needed.push(Type::String),
                BuiltinFn::ConsoleLog { .. }
                | BuiltinFn::ConsoleWrite
                | BuiltinFn::ConsoleErrorWrite => {}
            }
        }
    }
    for ty in needed {
        mir.types.intern(ty);
    }
}

/// Return whether `module` contains any function marked as a source test.
fn module_has_test_items(krate: &smelt_hir::Crate, module: &smelt_hir::Module) -> bool {
    module.items.iter().any(|item_id| {
        usize::try_from(item_id.0)
            .ok()
            .and_then(|index| krate.items.get(index))
            .is_some_and(
                |item| matches!(item, smelt_hir::Item::Function(function) if function.is_test),
            )
    })
}

/// Return whether a HIR body has no executable statements or tail expression.
fn body_is_empty(krate: &smelt_hir::Crate, body_id: BodyId) -> bool {
    usize::try_from(body_id.0)
        .ok()
        .and_then(|index| krate.bodies.get(index))
        .is_some_and(|body| {
            usize::try_from(body.root.0)
                .ok()
                .and_then(|index| body.blocks.get(index))
                .is_some_and(|block| block.stmts.is_empty() && block.tail.is_none())
        })
}

/// Converts a HIR literal to a MIR constant.
fn lower_literal(literal: &HirLiteral) -> Constant {
    match literal {
        HirLiteral::Bool(value) => Constant::Bool(*value),
        HirLiteral::Int(value) => Constant::Int(*value),
        HirLiteral::Float(value) => Constant::Float(*value),
        HirLiteral::String(value) => Constant::String(value.clone()),
        HirLiteral::Symbol(value) => Constant::Symbol(value.clone()),
        HirLiteral::Undefined => Constant::Undefined,
        HirLiteral::None => Constant::None,
    }
}

/// Returns the output type of a Future type.
fn future_inner_type(krate: &smelt_hir::Crate, ty: TypeId) -> Option<TypeId> {
    match krate.types.get(ty) {
        Some(Type::Future(inner)) => Some(*inner),
        _ => None,
    }
}
