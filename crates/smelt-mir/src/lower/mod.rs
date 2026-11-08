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

use std::collections::HashMap;
use std::convert::TryFrom;

use smelt_hir::{BodyId, Literal as HirLiteral, Span, Type, TypeId};

use crate::{
    ClosureId, Constant, FuncId, LocalId, Mir, MirClosure, MirFunction, Operand, Place
};

/// The `LoweringCtx` state machine, its constructors, and shared accessors.
mod context;
/// Locals that can change while one expression is evaluated.
mod volatile_locals;
/// Expression lowering: the core `lower_expr` dispatcher and call helpers.
mod expr;
/// Interface field flattening and generic type-parameter substitution.
mod generics;
/// Optional-chained method calls lowered with an unwind edge inside `try`.
mod optional_call;
/// Lvalue (place) lowering for assignment targets.
mod place;
/// Statement, block, and control-flow terminator lowering.
mod stmt;
/// The lexical control-scope stack (loops, `try` regions) and block cursor.
mod scope;
/// Lowering of the operations the op table transcribes.
mod transcribe;

/// Closure ABI ownership: construction, escape analysis, and throwing widening.
mod closures;
/// Reserved function slots: the one source of `FuncId`s during lowering.
mod function_slots;
/// Whole-program MIR analyses that run after core HIR lowering.
mod passes;

use context::{FunctionSpec, LoweringCtx, LoweringShared};
use generics::{InterfaceFlattening, lower_effective_interface_fields};

/// Converts a `usize` into `u32`, returning a `LowerError` if it does not fit.
fn u32_from_usize(value: usize, context: &str) -> Result<u32, LowerError> {
    u32::try_from(value).map_err(|error| LowerError {
        message: format!("{context}: {error}"),
        span: None,
    })
}

/// Converts a `u32` into `usize`, returning a `LowerError` if it does not fit.
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

/// Copies of PROJECTED assignment receivers that a place write must commit back,
/// outermost first.
///
/// A MIR place is rooted at a local, so `a.b[i] = v` materializes `a.b` into a
/// temporary and writes through that. Whether the write reaches `a` then depends
/// on the receiver's REPRESENTATION: a Smelt collection handle (`SmeltList`,
/// `SmeltRecord`, a reference class) shares its storage across clones, so the
/// write lands in the original, while a value representation (a plain
/// `HashMap`/`Vec` field, a value-class struct) is deep-copied and the write is
/// silently lost -- with no rustc error, because the generated code type-checks
/// perfectly. MIR cannot tell the two apart: which Rust type a `Dict`/`List`
/// becomes is a codegen representation choice.
///
/// So the assignment path does what the collection-mutation path already does
/// (`lower_mutation_receiver`): it records the projection it copied and stores
/// the temporary back through it once the write has been emitted. Correct for
/// both representations -- committing a handle stores an equal handle, and
/// committing a value commits the copy -- and it needs no answer to the
/// representation question. Nested receivers (`a.b.c[i] = v`) push one entry per
/// level and are replayed INNERMOST FIRST, so each level commits into a base
/// that is itself still live.
///
/// See `blocker-logs/hono-h31-projected-receiver-writeback.md`.
type PlaceWritebacks = Vec<(Place, LocalId)>;

/// A lowered assignment target: the place to write, and the receiver copies that
/// write must commit back afterwards.
///
/// The pair is what [`LoweringCtx::lower_place`] hands its callers, and the two
/// halves are useless apart: writing the place without replaying the writebacks
/// is exactly the silently-lost write H31 fixed.
type LoweredPlace = (Place, PlaceWritebacks);

/// A place ROOT: the local a projection is rooted at, and the receiver copies
/// reaching that root left behind.
///
/// Same contract as [`LoweredPlace`] one level down -- the root is a local, so
/// callers can hand it straight to `Place::Field`/`Place::Index` -- and it is
/// what `place_base_local` and `narrowed_receiver_base` return.
type LoweredPlaceBase = (LocalId, PlaceWritebacks);


/// Name of the local a `finally` holds a thrown value in while its clause
/// runs (see `HelperTypes::thrown_ty`). Public so the `SmeltUnknown` report can
/// recognize the boundary by it.
pub const PENDING_THROWN_LOCAL: &str = "smelt_pending_thrown";

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
    /// The erased type a `finally`'s exceptional exit holds the thrown value
    /// at, and the name of the local holding it (`smelt_pending_thrown`).
    ///
    /// This is a genuine dynamic boundary: `throw` accepts any value and the
    /// rethrow forwards it untouched, so no static type describes it (the same
    /// reason a `catch (e)` binding is `unknown`); example 181 rethrows
    /// through one clause on two paths, and the classifier's
    /// `pending_thrown_value_is_a_boundary` covers mixed thrown values. The fixed name is what
    /// lets the `SmeltUnknown` report classify the local as that boundary.
    /// Interned only for a crate that has a `try` with a `finally`, so a
    /// program without one does not gain an erased type it never uses.
    thrown_ty: Option<(TypeId, smelt_hir::Symbol)>,
    /// The Python exception type a `finally` in a Python body holds its
    /// rethrown value at; interned when `thrown_ty` is and the crate has a
    /// Python module, so a TypeScript crate never names it.
    py_thrown_ty: Option<TypeId>,
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
    // Carry FileId -> path across the HIR/MIR boundary so a later diagnostic
    // can name its source file; see `Mir::file_paths`.
    mir.accessor_fields.clone_from(&krate.accessor_fields);
    mir.file_paths = krate
        .modules
        .iter()
        .map(|module| (module.source.file, module.source.path.clone()))
        .collect();
    let volatile = volatile_locals::VolatileLocals::of(krate);
    let shared_helpers = HelperTypes {
        none: mir.types.intern(Type::None),
        loop_index_ty: mir.types.intern(Type::Float),
        loop_bool_ty: mir.types.intern(Type::Bool),
        thrown_ty: krate
            .bodies
            .iter()
            .any(|body| {
                body.stmts.iter().any(|stmt| {
                    matches!(
                        stmt,
                        smelt_hir::Stmt::TryCatch {
                            finally_body: Some(_),
                            ..
                        }
                    )
                })
            })
            .then(|| {
                (
                    mir.types.intern(Type::Unknown),
                    mir.symbols.intern_synthesized(&smelt_hir::SynthesizedName::new(PENDING_THROWN_LOCAL)),
                )
            }),
        py_thrown_ty: None,
    };
    let helpers = HelperTypes {
        py_thrown_ty: (shared_helpers.thrown_ty.is_some()
            && krate
                .modules
                .iter()
                .any(|module| module.source.language == smelt_hir::Language::Python))
        .then(|| {
            let name = mir.symbols.intern_synthesized(&smelt_hir::SynthesizedName::new(smelt_stdlib::PY_EXCEPTION_CLASS_NAME));
            mir.types.intern(Type::Class { name, args: vec![] })
        }),
        ..shared_helpers
    };
    let mut errors = Vec::new();

    let mut slots = function_slots::FunctionSlots::default();
    let item_functions = assign_function_ids(krate, &mut slots)?;
    let global_ids = lower_globals(krate, &mut mir, &item_functions, &mut errors);
    let languages = index_source_languages(krate);
    let tables = LoweringTables {
        item_functions: &item_functions,
        volatile_locals: &volatile,
        global_ids: &global_ids,
        languages: &languages,
    };
    lower_type_tables(krate, &mut mir, &item_functions);
    if let Err(duplicates) = crate::index::NameIndex::new(&mir) {
        return Err(duplicates
            .iter()
            .map(|duplicate| duplicate_declaration_error(krate, &mir, duplicate))
            .collect());
    }
    lower_item_functions(krate, &mut mir, &mut slots, &tables, helpers, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    lower_module_bodies(krate, &mut mir, &mut slots, &tables, helpers, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }
    mir.functions = slots.finish()?;
    run_finalization_passes(&mut mir).map_err(|error| vec![error])?;
    Ok(mir)
}

/// A [`LowerError`] for two declarations of one name, naming where each is.
///
/// MIR's class and interface tables are built in HIR item order, so table
/// position `i` of a kind is the `i`-th HIR item of that kind.
fn duplicate_declaration_error(
    krate: &smelt_hir::Crate,
    mir: &Mir,
    duplicate: &crate::index::DuplicateName,
) -> LowerError {
    let spans: Vec<Span> = krate
        .items
        .iter()
        .filter_map(|item| match (item, duplicate.kind) {
            (smelt_hir::Item::Class(class), "class") => Some(class.span),
            (smelt_hir::Item::Interface(interface), "interface") => Some(interface.span),
            _ => None,
        })
        .collect();
    let locate = |position: usize| {
        spans.get(position).map_or_else(
            || "<unknown>".to_owned(),
            |span| {
                let path = mir.file_paths.get(&span.file).map_or("<unknown>", String::as_str);
                format!("{path}@{}", span.start)
            },
        )
    };
    let (first, second) = duplicate.positions;
    LowerError {
        message: format!("{} (at {} and {})", duplicate.message(mir), locate(first), locate(second)),
        span: spans.get(second).copied(),
    }
}

/// Assign a stable [`FuncId`] to every HIR function item that has a body.
///
/// The map is built before any body is lowered so calls can resolve their
/// callees regardless of declaration order. Each id is a reserved slot of
/// `slots`, which the body is later stored into, so an id is its position by
/// construction.
fn assign_function_ids(
    krate: &smelt_hir::Crate,
    slots: &mut function_slots::FunctionSlots,
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
            let function_id = slots.reserve().map_err(|error| vec![error])?;
            item_functions.insert(item_id, function_id);
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
    /// Locals that can change while one expression is evaluated.
    volatile_locals: &'hir volatile_locals::VolatileLocals,
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
    // Shared across the interfaces so an `extends` chain flattens each
    // ancestor once; see `lower_effective_interface_fields`.
    let mut flattening = InterfaceFlattening::default();
    for hir_item in &krate.items {
        match hir_item {
            smelt_hir::Item::Class(class) => lower_class_table_entry(mir, class, item_functions),
            smelt_hir::Item::Interface(interface) => {
                lower_interface_table_entry(krate, mir, interface, &mut flattening);
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
    flattening: &mut InterfaceFlattening,
) {
    let fields = lower_effective_interface_fields(krate, mir, interface, &HashMap::new(), flattening);
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
        decode_facts: generics::lower_effective_interface_decode_facts(
            krate,
            interface,
            &mut std::collections::HashSet::new(),
        ),
    });
}

/// Lower every HIR function item that has a body into a [`MirFunction`],
/// stored into the slot [`assign_function_ids`] reserved for it.
///
/// Async functions have their `Promise<T>` return type unwrapped to `T`.
/// Lowering stops at the first hard error (pushing it to `errors` and breaking)
/// because later functions may depend on state this one would have produced.
/// A body id that names no body is such an error: skipping it would leave its
/// reserved slot empty.
#[expect(
    clippy::too_many_arguments,
    reason = "the output table (`mir` plus its reserved function `slots`) and the shared lowering inputs are separate borrows"
)]
fn lower_item_functions(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    slots: &mut function_slots::FunctionSlots,
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
        let Some(body) = usize_from_u32(body_id.0, "HIR body index does not fit in usize")
            .ok()
            .and_then(|index| krate.bodies.get(index))
        else {
            errors.push(LowerError {
                message: format!("function item {idx} names missing body {body_id:?}"),
                span: Some(function.span),
            });
            break;
        };
        let return_ty = if function.is_async {
            future_inner_type(krate, function.return_ty).unwrap_or(function.return_ty)
        } else {
            function.return_ty
        };
        let Some(function_id) = tables.item_functions.get(&item_id).copied() else {
            errors.push(LowerError {
                message: format!("function item {idx} has no reserved function id"),
                span: Some(function.span),
            });
            break;
        };
        let shared = LoweringShared {
            krate,
            item_functions: tables.item_functions,
            volatile_locals: tables.volatile_locals,
            global_ids: tables.global_ids,
            languages: tables.languages,
            loop_index_ty: helpers.loop_index_ty,
            loop_bool_ty: helpers.loop_bool_ty,
            thrown_ty: helpers.thrown_ty,
            py_thrown_ty: helpers.py_thrown_ty,
            closure_base: mir.closures.len(),
        };
        // A source function named `main` is not the program's entry point:
        // that is the module body below, which carries the module's name. The
        // backend recognises the entry by that name, so an item spelled `main`
        // (`async function main() {..}; main()`) would otherwise be taken for
        // it and emitted as a second `fn main` (E0428). The `_` spelling is
        // the one `sanitize_ident` gives every other reference to it.
        let name = if mir.symbols.get(function.name) == Some("main") {
            mir.symbols.intern("main_")
        } else {
            function.name
        };
        let spec = FunctionSpec {
            function_id,
            body_id,
            name,
            return_ty,
            owner: function.owner,
            is_async: function.is_async,
        };
        match LoweringCtx::new(shared, spec, body).and_then(LoweringCtx::lower) {
            Ok((lowered_function, closures)) => {
                mir.closures.extend(closures);
                if let Err(error) = slots.fill(function_id, lowered_function) {
                    errors.push(error);
                    break;
                }
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
/// to the next module rather than aborting. A module whose body id names no
/// body is such a failure: dropping it would silently drop the module's
/// top-level code.
#[expect(
    clippy::too_many_arguments,
    reason = "the output table (`mir` plus its reserved function `slots`) and the shared lowering inputs are separate borrows"
)]
fn lower_module_bodies(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    slots: &mut function_slots::FunctionSlots,
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
        let Some(body) = usize_from_u32(body_id.0, "HIR body index does not fit in usize")
            .ok()
            .and_then(|index| krate.bodies.get(index))
        else {
            errors.push(LowerError {
                message: format!("module `{}` names missing body {body_id:?}", module.name),
                span: None,
            });
            continue;
        };
        let name = mir.symbols.intern(&module.name);
        let function_id = match slots.reserve() {
            Ok(id) => id,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let shared = LoweringShared {
            krate,
            item_functions: tables.item_functions,
            volatile_locals: tables.volatile_locals,
            global_ids: tables.global_ids,
            languages: tables.languages,
            loop_index_ty: helpers.loop_index_ty,
            loop_bool_ty: helpers.loop_bool_ty,
            thrown_ty: helpers.thrown_ty,
            py_thrown_ty: helpers.py_thrown_ty,
            closure_base: mir.closures.len(),
        };
        let spec = FunctionSpec {
            function_id,
            body_id,
            name,
            // An async module body still RETURNS nothing: it is driven by the
            // emitted `#[tokio::main]`, not awaited by a caller, so the return
            // type stays `none` rather than being wrapped in a future. That
            // also keeps the emitter's `main` recognition (which tests for a
            // `none` return) working for both shapes.
            return_ty: helpers.none,
            owner: smelt_hir::FunctionOwner::Module,
            is_async: module.is_async,
        };
        match LoweringCtx::new(shared, spec, body).and_then(LoweringCtx::lower) {
            Ok((mut function, closures)) => {
                // An async module body awaits, and a statement-form await is
                // emitted as `future.await?` -- so the entry point's signature
                // has to carry the error channel its own body uses. The
                // throwing pass counts only the TERMINATOR form of await, which
                // is right for ordinary functions (widening every awaiting
                // function would change signatures across the crate for no
                // gain) but leaves the one function whose signature is decided
                // here unmarked.
                function.can_throw = function.can_throw || function.is_async;
                mir.closures.extend(closures);
                if let Err(error) = slots.fill(function_id, function) {
                    errors.push(error);
                }
            }
            // The slot stays empty, so `finish` reports it too; the lowering
            // error is the one that explains why.
            Err(error) => errors.push(error),
        }
    }
}

/// Run the whole-program closure/throwing finalization passes on lowered MIR.
///
/// Each step states what it requires of the MIR and what it establishes for
/// the steps after it; the order below is the only one that satisfies all of
/// them.
///
/// # Errors
///
/// The throwing fixpoint did not converge within its round cap.
fn run_finalization_passes(mir: &mut Mir) -> Result<(), LowerError> {
    // Requires: every body lowered. Establishes: each generic record
    // instantiation the program uses is in the table, so its declaration and
    // record helpers are emitted.
    passes::generic_records::intern_generic_record_instantiations(mir);
    // Requires: closures constructed. Establishes: `escapes` on every closure
    // (invariants 1-2 in `closures`), which throwing widening reads.
    closures::mark_escaping_closures(mir);
    // Requires: escape facts. Establishes: `can_throw` on every function and
    // closure, and the throwing ABI on every local holding a throwing closure
    // (invariants 3-4 in `closures`) -- jointly, at their fixpoint.
    run_throwing_fixpoint(mir)?;
    // Requires: final local types (widening retypes function-typed locals).
    // Establishes: an erased record that is mutated is promoted.
    crate::promote_erased_mutated_records(mir);
    // Requires: final local types. Establishes: operational type
    // normalization the emitter relies on.
    crate::normalize_operational_types(mir);
    // Requires: normalized types. Establishes: every type an inherited member
    // copy is emitted at is in the table, so its declaration is emitted.
    passes::inherited_types::intern_inherited_member_types(mir);
    // The passes above rewrote tables a by-name lookup may already have
    // indexed; the index is rebuilt on first use from the MIR as it now is.
    mir.clear_index();
    Ok(())
}

/// Upper bound on [`run_throwing_fixpoint`] rounds.
///
/// Both passes only ever flip a flag from "does not throw" to "throws" (a
/// function's `can_throw`, a function type's `may_throw`), so the joint state
/// rises monotonically and settles in a handful of rounds; the bound only
/// guards against a future non-monotone change.
const MAX_THROWING_ROUNDS: usize = 32;

/// Run throw propagation and throwing-closure widening to their joint fixpoint.
///
/// The two feed each other: [`passes::throwing::propagate_throwing_functions`]
/// marks a body throwing when it calls through a local typed with the throwing
/// ABI, and [`closures::widen_throwing_closure_types`] gives that ABI to every
/// local holding a closure marked throwing. The pair runs until a round leaves
/// the observable state (every `can_throw` flag and every local and capture
/// type) unchanged. It used to be unrolled exactly twice; no source program is
/// known to need a third round (a five-closure call chain settles in two), but
/// the fixpoint makes that a checked property instead of an assumption.
///
/// # Errors
///
/// The state still changed in round [`MAX_THROWING_ROUNDS`]. That is an error
/// in every build profile: continuing would emit a half-propagated throwing ABI
/// (a caller calling a `Result`-returning closure as if it were infallible).
fn run_throwing_fixpoint(mir: &mut Mir) -> Result<(), LowerError> {
    run_bounded_fixpoint(mir, MAX_THROWING_ROUNDS, throwing_state, |program| {
        passes::throwing::propagate_throwing_functions(program);
        closures::widen_throwing_closure_types(program);
    })
}

/// Run `round` until `observe` reports the same state twice in a row, at most
/// `max_rounds` times.
///
/// # Errors
///
/// A spanless [`LowerError`] when the state still changed in the last round.
fn run_bounded_fixpoint<S: PartialEq>(
    mir: &mut Mir,
    max_rounds: usize,
    observe: impl Fn(&Mir) -> S,
    mut round: impl FnMut(&mut Mir),
) -> Result<(), LowerError> {
    let mut state = observe(mir);
    for _ in 0..max_rounds {
        round(mir);
        let next = observe(mir);
        if next == state {
            return Ok(());
        }
        state = next;
    }
    Err(LowerError {
        message: format!(
            "throwing propagation/widening did not converge in {max_rounds} rounds"
        ),
        span: None,
    })
}

/// The part of the MIR the throwing fixpoint reads and writes: every body's
/// `can_throw` and local types, and every closure capture's type.
fn throwing_state(mir: &Mir) -> Vec<u64> {
    let mut state = Vec::new();
    for function in &mir.functions {
        state.push(u64::from(function.can_throw));
        state.extend(function.locals.iter().map(|local| u64::from(local.ty.0)));
    }
    for closure in &mir.closures {
        state.push(u64::from(closure.can_throw));
        state.extend(closure.locals.iter().map(|local| u64::from(local.ty.0)));
        state.extend(closure.captures.iter().map(|capture| u64::from(capture.ty.0)));
    }
    state
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
        HirLiteral::BigInt(digits) => Constant::BigInt(digits.clone()),
        HirLiteral::String(value) => Constant::String(value.clone()),
        HirLiteral::StringUnits(units) => Constant::StringUnits(units.clone()),
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

#[cfg(test)]
mod fixpoint_tests {
    //! The bounded fixpoint is an error, not a silent stop, in every profile
    //! (audit finding C9).

    use smelt_hir::{OriginalNameTable, SymbolInterner, TypeInterner};

    use super::run_bounded_fixpoint;
    use crate::Mir;

    /// An empty program to run rounds over.
    fn mir() -> Mir {
        Mir::new(TypeInterner::default(), SymbolInterner::default(), OriginalNameTable::default())
    }

    /// A round that changes the observed state every time never converges, and
    /// the fixpoint says so instead of returning half-propagated MIR.
    #[test]
    fn a_state_that_never_settles_is_an_error() {
        let mut program = mir();
        let mut counter = 0_u32;
        let result = run_bounded_fixpoint(
            &mut program,
            4,
            |observed| observed.globals.len(),
            |round| {
                counter += 1;
                round.globals.push(crate::MirGlobal {
                    name: smelt_hir::Symbol(counter),
                    ty: smelt_hir::TypeId(0),
                    init: crate::MirGlobalInit::Constant(crate::Constant::None),
                });
            },
        );
        assert!(result.is_err());
        assert_eq!(counter, 4, "the cap bounds the rounds");
    }

    /// A state that settles converges without error.
    #[test]
    fn a_settling_state_converges() {
        let mut program = mir();
        let result = run_bounded_fixpoint(&mut program, 4, |observed| observed.globals.len(), |_| {});
        assert!(result.is_ok());
    }
}
