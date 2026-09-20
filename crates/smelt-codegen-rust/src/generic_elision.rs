//! Which of a generic class's or interface's type parameters the emitted Rust
//! actually declares.
//!
//! # The rule
//!
//! A hand-writing Rust team does not declare a struct parameter its data never
//! holds. A TypeScript type parameter that never reaches a position the
//! generated Rust *spells* is type-level plumbing: the frontend keeps it (so
//! type-checking is unchanged), and only the Rust arity drops it.
//!
//! Concretely, for every generated nominal type (a `MirClass` or a
//! `MirInterface`, both of which are emitted as a Rust `struct` with a
//! `PhantomData` filler over their declared parameters), this computes the
//! **least fixpoint** of
//!
//! > parameter position `i` of `N` is CARRIED when the parameter's symbol
//! > occurs somewhere `N`'s own emitted Rust spells it — a field type, a static
//! > field, a descriptor, a base-class type argument, a method or constructor
//! > signature, a local of one of those bodies, or a closure nested in one —
//! > where an occurrence nested inside another generated nominal type
//! > `M<.., a_j, ..>` counts only when position `j` of `M` is itself carried,
//! > and the `PhantomData` filler counts for nothing.
//!
//! Starting from "nothing is carried" and growing to the fixpoint is what makes
//! a self-referential position drop: `Context<E, P, I>` whose only mention of
//! `E` is a field of type `Rc<dyn Fn(Context<E, .., ..>) -> ..>` reaches `E`
//! only through position 0 of `Context` itself, so nothing forces it and it
//! stays out. Positions that reach real storage — `HonoRequest<P, ..>` held in
//! a field, when `HonoRequest`'s own position 0 is carried — survive.
//!
//! # Why this needs no erasure
//!
//! An elided parameter is, by construction, spelled NOWHERE in the emitted
//! Rust for its own type: every occurrence the analysis found was inside a
//! position that is itself dropped. So dropping it changes `<..>` lists and
//! nothing else — no value ever has to be converted, and in particular no
//! `SmeltUnknown` is introduced. A mistake here is loud rather than silent:
//! rustc reports `cannot find type` for a parameter that was dropped while
//! still being spelled.
//!
//! # A deliberate conservatism
//!
//! A parameter that a method mentions only in its RETURN type is treated as
//! carried, even though the class's data never holds it. The alternative —
//! lifting it to a method-level generic — makes the generated class trait
//! non-object-safe and leaves a return-position-only parameter uninferable at
//! call sites; neither is worth it for a parameter the emitted Rust can simply
//! keep. The analysis therefore answers "does the emitted Rust have to spell
//! it", which is a superset of "does the data hold it".

#![expect(
    clippy::redundant_pub_crate,
    reason = "the analysis is shared across sibling emitter modules"
)]

use std::collections::{HashMap, HashSet};

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{BasicBlock, ClosureId, Mir, Rvalue, Statement};

/// Per-nominal-type record of which declared parameter positions survive into
/// the emitted Rust.
#[derive(Debug, Default, Clone)]
pub(crate) struct TypeParamElision {
    /// Carried flags per declared parameter position, keyed by the nominal
    /// type's name symbol. A name absent from the map declares no parameters,
    /// or is not a generated nominal type at all (a stdlib class, an imported
    /// class), and keeps every argument it is written with.
    carried: HashMap<Symbol, Vec<bool>>,
}

impl TypeParamElision {
    /// Returns whether position `index` of the nominal type `name` is spelled by
    /// the emitted Rust.
    ///
    /// A name this analysis knows nothing about keeps every position: only a
    /// generated class or interface has a declaration whose arity we control.
    pub(crate) fn is_carried(&self, name: Symbol, index: usize) -> bool {
        self.carried
            .get(&name)
            .and_then(|flags| flags.get(index).copied())
            .unwrap_or(true)
    }

    /// Returns whether `name` drops at least one declared parameter.
    pub(crate) fn elides_any(&self, name: Symbol) -> bool {
        self.carried
            .get(&name)
            .is_some_and(|flags| flags.iter().any(|carried| !carried))
    }

    /// Returns whether the emitted struct for `name` still declares a
    /// `PhantomData` filler.
    ///
    /// The filler exists to use the declared parameters; when every one of them
    /// is elided the struct declares none, so the field must disappear from the
    /// declaration AND from every struct literal that constructs it.
    pub(crate) fn emits_phantom(&self, name: Symbol, declared_params: usize) -> bool {
        (0..declared_params).any(|index| self.is_carried(name, index))
    }

    /// Keeps only the entries of `items` whose position is carried by `name`.
    ///
    /// Used for both halves of the change: a declaration's parameter list and a
    /// reference's type-argument list must drop the same positions, and a list
    /// longer than the declaration (which nothing produces today, but which a
    /// mismatch would make silent) keeps its surplus tail.
    pub(crate) fn retain_carried<T>(&self, name: Symbol, items: Vec<T>) -> Vec<T> {
        if !self.elides_any(name) {
            return items;
        }
        items
            .into_iter()
            .enumerate()
            .filter(|(index, _)| self.is_carried(name, *index))
            .map(|(_, item)| item)
            .collect()
    }
}

/// The type positions one nominal type's emitted Rust is built from.
struct ScanRoots {
    /// Name of the nominal type these roots belong to.
    name: Symbol,
    /// Declared parameter symbol at each position.
    params: Vec<Symbol>,
    /// Types the emitted Rust renders directly.
    types: Vec<TypeId>,
    /// `(parent name, parent type arguments)` for each inherited declaration:
    /// a class's single base, or an interface's `extends` list.
    parents: Vec<(Symbol, Vec<TypeId>)>,
}

/// Computes the carried-parameter fixpoint for every generated class and
/// interface in `mir`.
///
/// Collecting the scan roots walks each nominal type's own items once; the
/// fixpoint then re-walks the collected type ids until no position flips, which
/// terminates because flags only ever go from false to true.
pub(crate) fn compute(mir: &Mir) -> TypeParamElision {
    let mut roots = Vec::new();
    let mut carried: HashMap<Symbol, Vec<bool>> = HashMap::new();
    // Every nominal name the crate DECLARES. A `Type::Class` naming anything
    // else — an ambient type the crate only imports, a modeled host class — is
    // rendered as `SmeltUnknown` or as a fixed prelude type, both of which
    // spell none of its type arguments, so an occurrence inside one reaches no
    // emitted Rust at all (see `FunctionEmitter::rust_type`).
    let declared: HashSet<Symbol> = mir
        .classes
        .iter()
        .map(|class| class.name)
        .chain(mir.interfaces.iter().map(|interface| interface.name))
        .collect();

    for class in &mir.classes {
        if class.type_params.is_empty() {
            continue;
        }
        let params: Vec<Symbol> = class.type_params.iter().map(|param| param.name).collect();
        let mut types = Vec::new();
        for field in &class.fields {
            types.push(field.ty);
        }
        for field in &class.static_fields {
            types.push(field.ty);
        }
        for descriptor in &class.descriptors {
            types.push(descriptor.read_ty);
            if let Some(write_ty) = descriptor.write_ty {
                types.push(write_ty);
            }
        }
        let mut closures = Vec::new();
        for func_id in class
            .constructor
            .iter()
            .chain(class.methods.iter())
            .chain(class.static_methods.iter())
        {
            let Some(function) = mir.functions.iter().find(|func| func.id == *func_id) else {
                continue;
            };
            types.push(function.return_ty);
            types.extend(function.locals.iter().map(|local| local.ty));
            collect_block_types(&function.blocks, &mut types, &mut closures);
        }
        for method in &class.abstract_methods {
            types.extend(method.params.iter().map(|param| param.ty));
            types.push(method.return_ty);
        }
        collect_closure_types(mir, &mut closures, &mut types);
        declare(&mut carried, class.name, params.len());
        roots.push(ScanRoots {
            name: class.name,
            params,
            types,
            parents: class
                .base
                .map(|base_name| (base_name, class.base_args.clone()))
                .into_iter()
                .collect(),
        });
    }

    for interface in &mir.interfaces {
        if interface.type_params.is_empty() {
            continue;
        }
        let params: Vec<Symbol> = interface
            .type_params
            .iter()
            .map(|param| param.name)
            .collect();
        let mut types: Vec<TypeId> = interface.fields.iter().map(|field| field.ty).collect();
        for method in &interface.methods {
            types.extend(method.params.iter().map(|param| param.ty));
            types.push(method.return_ty);
        }
        declare(&mut carried, interface.name, params.len());
        roots.push(ScanRoots {
            name: interface.name,
            params,
            types,
            parents: interface
                .extends
                .iter()
                .map(|heritage| (heritage.parent, heritage.args.clone()))
                .collect(),
        });
    }

    // `SMELT_ELISION_DEBUG` prints each position as it is forced, with the type
    // that forced it: the cheapest way to answer "why does this class still
    // declare that parameter" on a corpus-sized crate.
    let debug_enabled = std::env::var_os("SMELT_ELISION_DEBUG").is_some();
    loop {
        let mut changed = false;
        for root in &roots {
            let positions: HashMap<Symbol, usize> = root
                .params
                .iter()
                .enumerate()
                .map(|(index, name)| (*name, index))
                .collect();
            let mut found = vec![false; root.params.len()];
            let mut seen = HashSet::new();
            for ty in &root.types {
                scan_type(
                    mir,
                    *ty,
                    &positions,
                    &carried,
                    &declared,
                    &mut found,
                    &mut seen,
                );
            }
            for (parent_name, parent_args) in &root.parents {
                for (index, arg) in parent_args.iter().enumerate() {
                    if position_is_carried(&carried, &declared, *parent_name, index) {
                        scan_type(
                            mir,
                            *arg,
                            &positions,
                            &carried,
                            &declared,
                            &mut found,
                            &mut seen,
                        );
                    }
                }
            }
            let probe_carried = if debug_enabled {
                carried.clone()
            } else {
                HashMap::new()
            };
            let Some(flags) = carried.get_mut(&root.name) else {
                continue;
            };
            for (index, (flag, hit)) in flags.iter_mut().zip(found.iter()).enumerate() {
                if *hit && !*flag {
                    *flag = true;
                    changed = true;
                    #[expect(
                        clippy::print_stderr,
                        clippy::use_debug,
                        reason = "SMELT_ELISION_DEBUG is an opt-in developer trace"
                    )]
                    if debug_enabled {
                        let culprit = root.types.iter().find(|ty| {
                            let mut probe = vec![false; root.params.len()];
                            let mut probe_seen = HashSet::new();
                            scan_type(
                                mir,
                                **ty,
                                &positions,
                                &probe_carried,
                                &declared,
                                &mut probe,
                                &mut probe_seen,
                            );
                            probe.get(index).copied().unwrap_or(false)
                        });
                        eprintln!(
                            "elision: {}<{}> position {index} is carried by {:?}",
                            mir.symbols.get(root.name).unwrap_or("?"),
                            root.params
                                .iter()
                                .map(|p| mir.symbols.get(*p).unwrap_or("?"))
                                .collect::<Vec<_>>()
                                .join(", "),
                            culprit.map(|ty| describe(mir, *ty, 4)),
                        );
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    TypeParamElision { carried }
}

/// Registers a nominal type's declared arity in the working map.
///
/// A class and an interface can share a name symbol when a crate declares both
/// (a declaration-merged TypeScript `class` and `interface`, which are emitted
/// as ONE Rust struct). The two parameter lists then have to agree for that
/// struct to be well-formed, so the SHORTER list wins: a position only one of
/// them declares stays outside the analysis and is therefore kept, which is the
/// safe answer in both directions.
fn declare(carried: &mut HashMap<Symbol, Vec<bool>>, name: Symbol, arity: usize) {
    let flags = carried.entry(name).or_insert_with(|| vec![false; arity]);
    if flags.len() > arity {
        flags.truncate(arity);
    }
}

/// Returns whether position `index` of `name` is carried in the working map.
///
/// A name the map does not know — a stdlib class, an imported class, a
/// non-generic one — carries every position, so an occurrence inside it counts.
fn position_is_carried(
    carried: &HashMap<Symbol, Vec<bool>>,
    declared: &HashSet<Symbol>,
    name: Symbol,
    index: usize,
) -> bool {
    if let Some(flags) = carried.get(&name) {
        return flags.get(index).copied().unwrap_or(true);
    }
    // A declared but non-generic name keeps whatever arguments it is written
    // with; an undeclared one is not rendered with arguments at all.
    declared.contains(&name)
}

/// Records every parameter of `positions` that `ty` forces the emitted Rust to
/// spell.
///
/// `seen` memoizes within one fixpoint iteration; it must not be shared across
/// iterations, because a position opening up can make a type that was fully
/// explored before reach further.
fn scan_type(
    mir: &Mir,
    ty: TypeId,
    positions: &HashMap<Symbol, usize>,
    carried: &HashMap<Symbol, Vec<bool>>,
    declared: &HashSet<Symbol>,
    found: &mut [bool],
    seen: &mut HashSet<TypeId>,
) {
    if !seen.insert(ty) {
        return;
    }
    let Some(resolved) = mir.types.get(ty) else {
        return;
    };
    match resolved {
        Type::TypeParam { name } => {
            if let Some(index) = positions.get(name)
                && let Some(flag) = found.get_mut(*index)
            {
                *flag = true;
            }
        }
        Type::Class { name, args } => {
            for (index, arg) in args.iter().enumerate() {
                if position_is_carried(carried, declared, *name, index) {
                    scan_type(mir, *arg, positions, carried, declared, found, seen);
                }
            }
        }
        Type::List(item)
        | Type::Set(item)
        | Type::Optional(item)
        | Type::Future(item) => {
            scan_type(mir, *item, positions, carried, declared, found, seen);
        }
        Type::Dict(key, value) | Type::JsMap(key, value) => {
            scan_type(mir, *key, positions, carried, declared, found, seen);
            scan_type(mir, *value, positions, carried, declared, found, seen);
        }
        Type::Tuple(items) | Type::Union(items) => {
            for item in items {
                scan_type(mir, *item, positions, carried, declared, found, seen);
            }
        }
        Type::Function(signature) => {
            for param in &signature.params {
                scan_type(mir, *param, positions, carried, declared, found, seen);
            }
            scan_type(mir, signature.return_ty, positions, carried, declared, found, seen);
        }
        Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        } => {
            scan_type(mir, *yield_ty, positions, carried, declared, found, seen);
            scan_type(mir, *return_ty, positions, carried, declared, found, seen);
            scan_type(mir, *next_ty, positions, carried, declared, found, seen);
        }
        Type::GeneratorResult {
            yield_ty,
            return_ty,
        } => {
            scan_type(mir, *yield_ty, positions, carried, declared, found, seen);
            scan_type(mir, *return_ty, positions, carried, declared, found, seen);
        }
        Type::Bool
        | Type::Int
        | Type::Float
        | Type::String
        | Type::Unknown
        | Type::Never
        | Type::None => {}
    }
}

/// Collects the types a body's blocks name directly, and the closures it builds.
///
/// Only two rvalues carry a type the emitted Rust spells on its own: a phi's
/// result type and an `UnknownCast` target. Everything else a block names is
/// already the type of a local.
fn collect_block_types(
    blocks: &[BasicBlock],
    types: &mut Vec<TypeId>,
    closures: &mut Vec<ClosureId>,
) {
    for block in blocks {
        for phi in &block.phis {
            types.push(phi.ty);
        }
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                    collect_rvalue_types(value, types, closures);
                }
                _ => {}
            }
        }
    }
}

/// Records the type and closure a single rvalue names, if any.
fn collect_rvalue_types(
    rvalue: &Rvalue,
    types: &mut Vec<TypeId>,
    closures: &mut Vec<ClosureId>,
) {
    match rvalue {
        Rvalue::UnknownCast { target, .. } => types.push(*target),
        Rvalue::Closure { id, .. } => closures.push(*id),
        _ => {}
    }
}

/// Drains `closures` — following closures nested inside closures — into `types`.
fn collect_closure_types(mir: &Mir, closures: &mut Vec<ClosureId>, types: &mut Vec<TypeId>) {
    let mut visited = HashSet::new();
    while let Some(id) = closures.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(closure) = mir.closures.iter().find(|candidate| candidate.id == id) else {
            continue;
        };
        types.push(closure.return_ty);
        types.extend(closure.locals.iter().map(|local| local.ty));
        types.extend(closure.captures.iter().map(|capture| capture.ty));
        collect_block_types(&closure.blocks, types, closures);
    }
}


/// Debug-only: renders a type id as a shallow tree, for `SMELT_ELISION_DEBUG`.
///
/// Truncates at `depth` so a self-referential type prints in bounded space.
fn describe(mir: &Mir, ty: TypeId, depth: usize) -> String {
    let Some(resolved) = mir.types.get(ty) else {
        return "?".to_owned();
    };
    let Some(next_depth) = depth.checked_sub(1) else {
        return "..".to_owned();
    };
    let child = |id: TypeId| describe(mir, id, next_depth);
    match resolved {
        Type::TypeParam { name } => {
            format!("TypeParam({})", mir.symbols.get(*name).unwrap_or("?"))
        }
        Type::Class { name, args } => format!(
            "{}<{}>",
            mir.symbols.get(*name).unwrap_or("?"),
            args.iter().map(|arg| child(*arg)).collect::<Vec<_>>().join(", ")
        ),
        Type::Optional(inner) => format!("Optional({})", child(*inner)),
        Type::List(inner) => format!("List({})", child(*inner)),
        Type::Future(inner) => format!("Future({})", child(*inner)),
        Type::Set(inner) => format!("Set({})", child(*inner)),
        Type::Dict(key, value) => format!("Dict({}, {})", child(*key), child(*value)),
        Type::JsMap(key, value) => format!("JsMap({}, {})", child(*key), child(*value)),
        Type::Tuple(items) => format!(
            "Tuple({})",
            items.iter().map(|arg| child(*arg)).collect::<Vec<_>>().join(", ")
        ),
        Type::Union(items) => format!(
            "Union({})",
            items.iter().map(|arg| child(*arg)).collect::<Vec<_>>().join(" | ")
        ),
        Type::Function(signature) => format!(
            "fn({}) -> {}",
            signature
                .params
                .iter()
                .map(|arg| child(*arg))
                .collect::<Vec<_>>()
                .join(", "),
            child(signature.return_ty)
        ),
        other => format!("{other:?}"),
    }
}
