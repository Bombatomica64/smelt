//! Which type parameters of a free function lift to real Rust generics.
//!
//! A hand-written port keeps `T` generic where it can. This analysis answers,
//! from MIR alone, where the emitted Rust can do the same: a parameter lifts when
//! the body only moves it, reaches a bound through the generated trait, or is
//! inferable through a callback at every call site. `EmitContext` runs it before
//! any body renders, so the definition and every call site agree on the shape.

use std::{cell::RefCell, collections::{HashMap, HashSet}};

use smelt_hir::{AsyncOp, Symbol, Type, TypeId};
use smelt_mir::{
    BasicBlock, FuncId, LocalDecl, LocalId, Mir, MirFunction, Operand, Place, Rvalue,
    Statement, Terminator,
};

use crate::{
    EmitError, bound_traits::BoundTraitPlan, classes::{GENERATED_TYPE_PARAM_BOUNDS, class_name_text, type_param_in_dict_key},
    generic_bindings, id_index, rust::RustIdent,
};

/// Per function, whether a call site passes an erased argument at a bare
/// type-parameter position. Every call site asks about its callee, and the answer
/// scans the callee's callers, so it is computed once per function.
pub(crate) type ErasedArgumentMemo = RefCell<HashMap<FuncId, bool>>;

/// Return whether a free function's signature should emit real Rust generics.
///
/// This increment lowers a free function to real Rust generics
/// (`fn identity<T>(x: T) -> T`) only when doing so is provably sound for Rust's
/// type inference at every call site. Lifting is decided per type parameter
/// (see [`liftable_type_params`]): a parameter that fails a per-parameter check
/// erases alone while its siblings can still lift. Only the two function-wide
/// gates — `called_with_erased_type_param_argument` and
/// `callback_only_params_are_pinned_at_every_call_site` — erase every type
/// parameter at once (all of them render as `SmeltUnknown`).
///
/// A type parameter is emitted as a real Rust generic only when EVERY condition
/// below holds for it. A function emits real generics when at least one of its
/// declared type parameters qualifies; any remaining type parameters stay erased:
/// - **unconstrained** — a bounded parameter (`<D extends Date>`, `<O extends {
///   ... }>`) still needs method/property dispatch through the erased boundary
///   that this increment does not model;
/// - **inferable from a plain value parameter** — the parameter must appear in a
///   *direct* (non-callback) parameter position such as `T`, `T[]`, or
///   `Option<T>`, so Rust can infer it from the argument. A type parameter used
///   only in the return type (e.g. a type predicate `data is T`) or only inside
///   a callback parameter cannot be inferred and would produce `E0283`.
///
/// A type parameter that appears inside a callback parameter is *no longer*
/// rejected outright. That gate (Increment 1 of
/// `blocker-logs/estk-callback-generics-plan.md`) existed because
/// `param_type` rendered a callback's parameter halves in an empty
/// type-parameter scope, so a callee whose signature said `Fn(T)` was handed
/// closures declared `|arg: SmeltUnknown|`. The scope, not Rust inference, was
/// the defect; both halves of a callback type now render in the callee's own
/// lexical scope, and call-site adapters render the callee's declared callback
/// under the bindings that call site pinned.
///
/// That relaxation is bounded by §4.4 of the plan: only a **direct, required,
/// borrowed, non-rest** `Type::Function` parameter can carry a type parameter
/// into the emitted signature and still have every call site render an argument
/// that matches it. See [`callback_occurrences_are_liftable`] for the rule and
/// for why each excluded shape is excluded.
///
/// `owned_callback_params` is the emitter's own ownership fixpoint
/// (`emitter::compute_owned_callback_params`), threaded in rather than
/// recomputed so the gate and the renderer cannot grow two different notions of
/// "owned callback".
///
/// These are the deferred "bounded / higher-order / return-only" slices of #99.
pub(crate) fn function_emits_rust_generics(
    mir: &Mir,
    function: &MirFunction,
    owned_callback_params: &HashSet<(FuncId, LocalId)>,
    bounds: &BoundTraitPlan,
    erased_arguments: &ErasedArgumentMemo,
) -> bool {
    !liftable_type_params(mir, function, owned_callback_params, bounds, erased_arguments).is_empty()
}

/// Whether every value whose type mentions `name` is only ever *moved* in this
/// body — never inspected.
///
/// This asks MIR the question the body-cleanliness trial
/// ([`FunctionEmitter::renders_real_generics`]) can only approximate. That trial
/// records whether the render touched the erased carrier. That works while the
/// decision is per-function, but it cannot say which type parameter a use came
/// from, so under per-parameter lifting it cannot tell "erased because `K` did
/// not lift" (correct) from "the lifted `T` leaked into an erased carrier"
/// (broken). MIR gives that attribution for free:
/// every operand names the local it reads, and that local carries a type.
///
/// The rule is inverted rather than enumerated. `Rvalue` has ~180 variants, and
/// classifying each as parametric-or-not would be a large table that drifts as
/// variants are added. But a value of an opaque type `T` supports exactly one
/// operation — being moved — so it is enough to recognise the rvalues that
/// relocate their operands and treat every other *read* of a `T`-typed operand
/// as an inspection. Unlisted variants therefore fall through to "inspects",
/// which is the safe direction: a new `Rvalue` cannot silently start erasing a
/// lifted type parameter.
fn type_param_only_moved(mir: &Mir, function: &MirFunction, name: Symbol) -> bool {
    let mut seen: HashSet<u32> = HashSet::new();
    body_only_moves(mir, &function.locals, &function.blocks, name, &mut seen)
}

/// Walk one MIR body — a function's or a closure's — for inspections of `name`.
///
/// Split out of [`type_param_only_moved`] because a closure is a *separate* MIR
/// body with its own locals and blocks, so walking `function.blocks` alone never
/// sees inside one. es-toolkit's `flatten` is exactly that shape: `T` lifted,
/// and the `Array.isArray(item)` that inspects it lives in the inner recursive
/// closure, emitting `matches!(item, SmeltUnknown::Array(_))` against a `T` and
/// failing with "expected type parameter `T`, found `SmeltUnknown`".
///
/// `seen` guards against a closure cycle; the ids are `ClosureId`'s inner value.
fn body_only_moves(
    mir: &Mir,
    locals: &[LocalDecl],
    blocks: &[BasicBlock],
    name: Symbol,
    seen: &mut HashSet<u32>,
) -> bool {
    let local_mentions = |local: LocalId| -> bool {
        let Ok(index) = id_index(local.0, "local index does not fit usize") else {
            return false;
        };
        locals
            .get(index)
            .is_some_and(|decl| generic_bindings::type_param_occurs(mir, decl.ty, name))
    };
    let mentions = |operand: &Operand| -> bool {
        let place = match operand {
            Operand::Copy(place) | Operand::Move(place) => place,
            Operand::Const(_) => return false,
        };
        // A projection is attributed to its base local's type, so reading
        // `record[k]` out of a `SmeltRecord<String, SmeltList<T>>` counts as
        // mentioning `T` even though the projected element may drop it. That
        // over-approximates, which errs toward not lifting — the safe direction.
        let base = match place {
            Place::Local(local)
            | Place::Field { base: local, .. }
            | Place::Index { base: local, .. } => *local,
            // A write into a mutable global's cell has no base local to
            // attribute the mention to. The global's own type is fixed at its
            // declaration and is not a lifting candidate, so this contributes
            // no mention.
            Place::Global { .. } => return false,
        };
        local_mentions(base)
    };

    for block in blocks {
        for statement in &block.statements {
            let (Statement::Assign { value, .. } | Statement::AssignPlace { value, .. }) = statement
            else {
                continue;
            };
            // A closure body is part of the enclosing body's dataflow: whatever
            // it does to a captured `T` decides whether that `T` can stay
            // opaque. Recurse before the opacity rules below, which treat
            // `Rvalue::Closure` itself as a move of its captures.
            if let Rvalue::Closure { id, .. } = value
                && seen.insert(id.0)
                && let Some(closure) = mir.closures.iter().find(|candidate| candidate.id == *id)
                && !body_only_moves(mir, &closure.locals, &closure.blocks, name, seen)
            {
                return false;
            }
            if rvalue_preserves_opacity(value) {
                continue;
            }
            // A member read or method call through the parameter's bound
            // dispatches through its generated trait, which is an operation a
            // lifted bounded `T` supports; only its arguments can inspect.
            let through_bound = match value {
                Rvalue::BoundMethod { receiver, .. } | Rvalue::BoundField { receiver, .. } => {
                    Some(receiver)
                }
                _ => None,
            };
            let mut reads_t = false;
            value.reads(|operand| {
                if through_bound.is_some_and(|receiver| std::ptr::eq(receiver, operand)) {
                    return;
                }
                if mentions(operand) {
                    reads_t = true;
                }
            });
            // A `T`-typed destination is evidence of relocation ONLY when the
            // value being stored is itself `T`-typed: `item = arr[i]` moves a
            // `T` out of a `SmeltList<T>`, and some operand carries it. When the
            // destination is `T` but nothing read was, the slot is being filled
            // from somewhere else — an erased helper, a defaulted constant — and
            // that is a leak, not a move.
            if statement_destination_mentions(statement, &local_mentions) {
                if reads_t {
                    continue;
                }
                return false;
            }
            if reads_t {
                return false;
            }
        }
        // Branching on a `T` needs JavaScript truthiness or an equality an
        // opaque `T` cannot answer. `Return` is a move and stays off this list.
        if let Some(
            Terminator::Switch { cond: probe, .. } | Terminator::Match { scrutinee: probe, .. },
        ) = &block.terminator
            && mentions(probe)
        {
            return false;
        }
        // A call binds its result into `dest`. If that slot is `T`-typed, the
        // callee has to be producing a `T`, and the only evidence available here
        // is that some argument carried one in. Calls are terminators, so the
        // statement walk above cannot see them.
        if let Some(Terminator::Call { callee, args, dest, .. }) = &block.terminator
            && local_mentions(*dest)
        {
            let mut mentioned = args.iter().any(&mentions);
            if let smelt_mir::Callee::Op(op) = callee {
                op.reads(|operand| mentioned |= mentions(operand));
            }
            if !mentioned {
                return false;
            }
        }
    }
    true
}

/// Whether a statement's destination local carries `name`, for
/// [`body_only_moves`]. Takes the membership test as a closure so the same rule
/// serves a function body and a closure body, which have different local tables.
fn statement_destination_mentions(
    statement: &Statement,
    local_mentions: &impl Fn(LocalId) -> bool,
) -> bool {
    let local = match statement {
        Statement::Assign { dest, .. } => *dest,
        Statement::AssignPlace { place, .. } => match place {
            Place::Local(local)
            | Place::Field { base: local, .. }
            | Place::Index { base: local, .. } => *local,
            // No destination LOCAL: the destination is a `thread_local!` cell.
            Place::Global { .. } => return false,
        },
        // The container the entry update writes through is its destination.
        Statement::DictEntryUpdate { base, .. } => *base,
    };
    local_mentions(local)
}

/// Whether an rvalue only relocates its operands, leaving their types opaque.
///
/// The whitelist for [`type_param_only_moved`]; see that function for why this
/// is a whitelist with a conservative default rather than a full classification
/// of `Rvalue`. Every entry must be an operation that moves a value without
/// reading into it: if the emitter renders it by calling a method on the value,
/// comparing it, or coercing it to a key, it does not belong here.
fn rvalue_preserves_opacity(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        // `Rvalue::Use` is deliberately NOT here. A move is opacity-preserving
        // only when source and destination agree about `T`; a `T` moved into a
        // slot of another type is an erasure, not a relocation, and the general
        // destination rule below decides that correctly. Whitelisting `Use`
        // short-circuited that check, which is how es-toolkit's `unzipWith`
        // leaked: `group` comes from `new Array(n)` so its type is
        // `List<Unknown>`, a `T` element is moved into it, and the erased list
        // is then handed to an `F0: Fn(SmeltList<T>)` bound.
        //
        // Container *construction* stays listed: `const out: T[] = []` has a
        // `T`-typed destination and no `T`-typed operand, which the general rule
        // would otherwise read as filling a `T` slot from nothing.
        Rvalue::List(_)
            | Rvalue::Tuple(_)
            | Rvalue::ListPush { .. }
            | Rvalue::DictEntryPush { .. }
            | Rvalue::ListExtend { .. }
            // Capturing into a closure environment.
            | Rvalue::Closure { .. }
            // Container-level queries read the *container*, never an element, so
            // a `SmeltList<T>` operand leaves `T` opaque even though the result
            // (a length) drops it.
            | Rvalue::Len { .. }
            // Handing a `T` to a callback moves it into the callee. Whether that
            // callee's signature can carry `T` is a separate question, answered
            // by `callback_occurrences_are_liftable`. `ListCallback` is the
            // same shape one level up: `arr.some(cb)` passes elements to `cb`
            // rather than reading them. Operations that compare elements
            // themselves live in other variants (`ListContains`) and stay off
            // this list.
            | Rvalue::ClosureCall { .. }
            | Rvalue::ClosureCallSpread { .. }
            | Rvalue::ListCallback { .. }
            // Handing a `SmeltFuture<T>` to the executor moves it; the future is
            // an opaque handle here and its `T` is never read. The other async
            // operations either read the value or keep `T` in their result type,
            // where the destination rule already covers them.
            | Rvalue::AsyncOp {
                op: AsyncOp::SpawnLocal | AsyncOp::CreateTask,
                ..
            }
    )
}

/// The subset of a free function's source type parameters that can be emitted
/// as real Rust generics, leaving the rest to erase.
///
/// Increment 5. The decision used to be per *function*: one constrained
/// parameter demoted every sibling, so `groupBy<T, K extends PropertyKey>`
/// erased `T` as well and emitted
/// `fn group_by(arr: SmeltList<SmeltUnknown>, key: &dyn Fn(SmeltUnknown) -> SmeltUnknown)`
/// even though `T` is directly inferable from `arr`. 215 of es-toolkit's 800
/// generic exported functions carry a constrained parameter, so the all-or-
/// nothing rule was erasing a large amount of shape that the signature itself
/// could have carried.
///
/// A constrained parameter is still not liftable — the emitter has no rendering
/// for its bound — but it now excludes only itself. Every parameter absent from
/// this set lowers through the emitter's existing erasure path, which is the
/// same path a fully-erased function already takes, so a partially-lifted
/// signature needs no new rendering.
///
/// The two function-level safety gates are unchanged and still all-or-nothing:
/// if a call site passes an erased argument, or a callback-only parameter is
/// unpinned at some call site, nothing lifts.
pub(crate) fn liftable_type_params(
    mir: &Mir,
    function: &MirFunction,
    owned_callback_params: &HashSet<(FuncId, LocalId)>,
    bounds: &BoundTraitPlan,
    erased_arguments: &ErasedArgumentMemo,
) -> HashSet<Symbol> {
    if function.type_params.is_empty() {
        return HashSet::new();
    }

    let param_types: Vec<TypeId> = function
        .params
        .iter()
        .filter_map(|param| {
            function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)
                .map(|local| local.ty)
        })
        .collect();

    if called_with_erased_type_param_argument_memoized(mir, function, erased_arguments)
        || !callback_only_params_are_pinned_at_every_call_site(
            mir,
            function,
            owned_callback_params,
        )
    {
        return HashSet::new();
    }

    let base: HashSet<Symbol> = function
        .type_params
        .iter()
        .filter(|type_param| {
            // A constrained parameter erases — but only it (before Increment 5
            // this returned `false` for the whole function) — unless its bound
            // is a class or interface with a generated trait that every use and
            // every call site can go through (bounded-generics plan,
            // increment 3; see `bounded_type_param_liftable`).
            if type_param.constraint.is_some()
                && !bounded_type_param_liftable(mir, function, type_param, bounds)
            {
                return false;
            }
            let name = type_param.name;
        // The parameter must have an inference source in the emitted signature.
        // Either a direct value parameter position ...
        let directly = param_types
            .iter()
            .any(|&param_ty| type_param_preserved_in_emitted_type(mir, param_ty, name));
        // ... or (Increment 3) a position of a callback that Increment 2 emits
        // as `&F{n}` with an `F{n}: Fn(..) + ?Sized` bound, which rustc infers
        // through from the closure's own type. The disjunct deliberately calls
        // the renderer's own eligibility predicate rather than a second copy of
        // it, so a parameter can only be declared inferable through a callback
        // that really will carry an `Fn` bound.
            (directly
                || type_param_inferable_through_callback(
                    mir,
                    function,
                    owned_callback_params,
                    name,
                )
                .is_some())
                // ... and every callback position it *also* occupies must be one
                // the renderer can express (§4.4). This stays ANDed onto BOTH
                // branches: it is the renderability rule and applies to every
                // occurrence, however the parameter got inferred.
                && callback_occurrences_are_liftable(mir, function, owned_callback_params, name)
        })
        .map(|type_param| type_param.name)
        .collect();

    // The MIR opacity analysis governs PARTIAL lifts only.
    //
    // When every declared parameter clears the checks above, the function is one
    // the pre-Increment-5 rule already lifted, and `renders_real_generics`'
    // carrier trial decides it exactly as before — applying a second, stricter
    // filter here could demote functions that are generic today, which is a
    // regression this change has no business making.
    //
    // A partial lift is new ground: the trial cannot judge it (the parameters
    // that did not lift put erased tokens in the body by design), so opacity is
    // established per parameter from MIR instead.
    if base.len() == function.type_params.len() {
        return base;
    }
    base.into_iter()
        .filter(|name| {
            type_param_only_moved(mir, function, *name)
                && !type_param_used_as_map_key(mir, function, *name)
        })
        .collect()
}

/// Whether `name` is used as a map key anywhere in this function's locals.
///
/// A `SmeltJsMap`/`SmeltRecord` key is compared through the erased JavaScript
/// key-equality projection, which needs `SmeltJsKeyEq` — a trait a bare `T` does
/// not carry, and which generated unions do not implement. The generic bounds
/// cannot support it, and [`class_type_param_used_as_map_key`] applies the same rule to a class's
/// fields; free functions had no equivalent, which is how es-toolkit's `keyBy`
/// lifted a key-position parameter and failed at its call sites with
/// "the trait bound `SmeltUnion480: SmeltJsKeyEq` is not satisfied".
///
/// Emitting the bound instead of refusing the lift would not help: the error is
/// at the *instantiation*, where the concrete type genuinely lacks the impl.
fn type_param_used_as_map_key(mir: &Mir, function: &MirFunction, name: Symbol) -> bool {
    function
        .locals
        .iter()
        .any(|local| type_param_in_dict_key(mir, local.ty, name))
}

/// Return whether every type parameter that is reachable *only* through a
/// callback is pinned at every static call site of `function`.
///
/// Increment 3's safety valve, and deliberately **not** §4.3's widened
/// `called_with_erased_type_param_argument`. §7 records that the literal
/// widening — demote whenever any argument leaves any callee parameter unbound
/// or bound to an erased type — was implemented during Increment 1 and cut the
/// es-toolkit lift from 15 definitions to 2, because spec files hoist their
/// arguments into `_smelt_tmp_N` locals typed `List<Unknown>` and one such call
/// site demoted a definition crate-wide.
///
/// This valve avoids that collapse three independent ways:
///
/// * **Scope.** It returns `true` immediately when the function has no
///   callback-only type parameter, which is the case for every definition
///   Increments 1 and 2 lift (each pins its `T` from a value parameter). It
///   therefore cannot move a byte of their output.
/// * **Predicate.** It does *not* require `Concrete`. `Erased` and
///   `Unsupported(..)` are accepted: an erased callback argument still renders
///   a definite adapter whose return spells `SmeltUnknown`, which pins the
///   parameter perfectly well. Demanding `Concrete` is exactly what collapsed
///   the §4.3 version.
/// * **Breadth.** It examines one argument position per callback-only
///   parameter, not an existential over every argument of every call site.
///
/// What is left are the structural holes where *nothing* can pin the parameter,
/// which §1.3 describes and which no textual annotation can repair:
///
/// a. the callback argument is **omitted**, so the trailing-default loop
///    renders `borrowed_default_function_expr` and no closure carries the type;
/// b. the argument's static type is **invisible** to the shared analysis
///    (`operand_type` returns `Err`), so which renderer runs cannot be
///    predicted;
/// c. two arguments demand **incompatible** instantiations of a parameter that
///    has no other pinning source (`Conflict`).
fn callback_only_params_are_pinned_at_every_call_site(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
) -> bool {
    let param_types: Vec<TypeId> = function
        .params
        .iter()
        .filter_map(|param| {
            function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)
                .map(|local| local.ty)
        })
        .collect();
    // Only parameters with no direct value position at all are at risk; the
    // rest are pinned by an ordinary argument and need no inference power from
    // the callback position (§1.3).
    let callback_only: Vec<(Symbol, usize)> = function
        .type_params
        .iter()
        .filter(|type_param| {
            !param_types
                .iter()
                .any(|&param_ty| type_param_preserved_in_emitted_type(mir, param_ty, type_param.name))
        })
        .filter_map(|type_param| {
            type_param_inferable_through_callback(mir, function, owned, type_param.name)
                .map(|index| (type_param.name, index))
        })
        .collect();
    if callback_only.is_empty() {
        return true;
    }

    for caller in &mir.functions {
        for block in &caller.blocks {
            let Some(Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if *target != function.id {
                continue;
            }
            let bindings = generic_bindings::collect_bindings(mir, function, caller, args);
            for &(name, index) in &callback_only {
                // (a) the callback argument is omitted entirely.
                let Some(arg) = args.get(index) else {
                    return false;
                };
                // (b) the argument's static type is not resolvable.
                if generic_bindings::operand_type(mir, caller, arg).is_err() {
                    return false;
                }
                // (c) irreconcilable evidence for a parameter with no fallback.
                if matches!(
                    bindings.get(name),
                    Some(generic_bindings::TypeParamBinding::Conflict { .. })
                ) {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether the class- or interface-bounded type parameter `type_param` of
/// `function` can be a real Rust generic carrying its bound's trait
/// (`A: AnimalLike`, bounded-generics plan increment 3).
///
/// Every condition is structural, and each one guards a way the lifted
/// signature would fail to compile or change behaviour:
///
/// 1. the bound has a planned trait ([`BoundTraitPlan`]), and every member read
///    through this parameter is one of its members;
/// 2. the parameter occurs in value parameters only bare (`a: A`), so each call
///    site's argument type IS the instantiation (`A[]` or a callback would
///    need the trait on a type inferred through a container);
/// 3. no place projects a field or index off a value of the parameter's type
///    (a write `a.legs = 1` has no trait setter, and a raw place read would
///    render the erased member lookup against a generic `A`);
/// 4. every static call — from a function or a closure — passes, in each bare
///    position, a type that implements the trait, the function's own `A`
///    (recursion), or a type parameter of the calling free function bounded
///    by the same bound. That last case lifts only if the caller's parameter
///    lifts too, which `EmitContext::populate_generic_functions` enforces
///    crate-wide (see [`bound_forwarding_callers`]).
///
/// Anything else keeps the erased fallback, so `D extends Date` (no class
/// item, no trait) erases exactly as before.
fn bounded_type_param_liftable(
    mir: &Mir,
    function: &MirFunction,
    type_param: &smelt_hir::TypeParamDef,
    bounds: &BoundTraitPlan,
) -> bool {
    let Some(bound) = crate::bound_traits::constraint_bound(mir, type_param.constraint) else {
        return false;
    };
    // `D extends Date` goes through the prelude's `DateLike`, which only
    // `SmeltDate` implements; a class or interface bound needs its planned trait.
    let date_bound = crate::bound_traits::constraint_is_date(mir, type_param.constraint);
    let planned = bounds.trait_for(bound);
    if planned.is_none() && !date_bound {
        return false;
    }
    let implemented_by = |ty: TypeId| match planned {
        Some(trait_plan) => trait_plan.implemented_by(mir, ty),
        None => {
            matches!(mir.types.get(ty), Some(Type::Class { name, args }) if *name == bound && args.is_empty())
        }
    };
    let name = type_param.name;
    let is_bare = |ty: TypeId| {
        matches!(mir.types.get(ty), Some(Type::TypeParam { name: found }) if *found == name)
    };
    let mut bare_positions = Vec::new();
    for (index, param) in function.params.iter().enumerate() {
        let Some(ty) = id_index(param.0, "local index does not fit usize")
            .ok()
            .and_then(|slot| function.locals.get(slot))
            .map(|local| local.ty)
        else {
            return false;
        };
        if is_bare(ty) {
            bare_positions.push(index);
        } else if generic_bindings::type_param_occurs(mir, ty, name) {
            return false;
        }
    }
    if bare_positions.is_empty() {
        return false;
    }
    let mut seen = HashSet::new();
    if !body_uses_bound_only_through_trait(
        mir,
        &function.locals,
        &function.blocks,
        &BoundUse { name, bound, planned },
        &mut seen,
    ) {
        return false;
    }

    let callers = mir
        .functions
        .iter()
        .map(|caller| (Some(caller), caller.locals.as_slice(), caller.blocks.as_slice()))
        .chain(
            mir.closures
                .iter()
                .map(|closure| (None, closure.locals.as_slice(), closure.blocks.as_slice())),
        );
    for (caller, locals, blocks) in callers {
        for block in blocks {
            let Some(Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if *target != function.id {
                continue;
            }
            for &position in &bare_positions {
                let Some(Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) =
                    args.get(position)
                else {
                    return false;
                };
                let Some(arg_ty) = id_index(local.0, "local index does not fit usize")
                    .ok()
                    .and_then(|index| locals.get(index))
                    .map(|decl| decl.ty)
                else {
                    return false;
                };
                if implemented_by(arg_ty) {
                    continue;
                }
                let Some(Type::TypeParam { name: arg_param }) = mir.types.get(arg_ty) else {
                    return false;
                };
                let forwarded_by_caller = caller.is_some_and(|calling| {
                    (calling.id == function.id && *arg_param == name)
                        || (matches!(calling.origin, smelt_mir::HirOrigin::Body(_))
                            && calling.type_params.iter().any(|declared| {
                                declared.name == *arg_param
                                    && crate::bound_traits::constraint_bound(mir, declared.constraint)
                                        == Some(bound)
                            }))
                });
                if !forwarded_by_caller {
                    return false;
                }
            }
        }
    }
    true
}

/// One bounded type parameter and its planned trait, for
/// [`body_uses_bound_only_through_trait`].
struct BoundUse<'plan> {
    /// The type parameter's name.
    name: Symbol,
    /// Its class or interface bound.
    bound: Symbol,
    /// The bound's planned trait; `None` for `Date`, whose `DateLike` carries
    /// no bound members (the frontend dispatches a `D extends Date` receiver
    /// as a `Date`).
    planned: Option<&'plan crate::bound_traits::BoundTrait>,
}

/// Whether one body (and every closure it creates) touches values of the type
/// parameter only through the bound's trait: each `BoundMethod` / `BoundField`
/// on such a receiver names a member of the planned trait, and no place
/// projects a field or index off a local of that type.
fn body_uses_bound_only_through_trait(
    mir: &Mir,
    locals: &[LocalDecl],
    blocks: &[BasicBlock],
    bound_use: &BoundUse<'_>,
    seen: &mut HashSet<u32>,
) -> bool {
    let local_is_param = |local: LocalId| {
        id_index(local.0, "local index does not fit usize")
            .ok()
            .and_then(|index| locals.get(index))
            .is_some_and(|decl| {
                matches!(
                    mir.types.get(decl.ty),
                    Some(Type::TypeParam { name: found }) if *found == bound_use.name
                )
            })
    };
    let projects = |place: &Place| match place {
        Place::Field { base, .. } | Place::Index { base, .. } => local_is_param(*base),
        Place::Local(_) | Place::Global { .. } => false,
    };
    let operand_projects = |operand: &Operand| match operand {
        Operand::Copy(place) | Operand::Move(place) => projects(place),
        Operand::Const(_) => false,
    };
    let operand_is_param = |operand: &Operand| match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
            local_is_param(*local)
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => false,
    };
    for block in blocks {
        for statement in &block.statements {
            let value = match statement {
                Statement::Assign { value, .. } => value,
                Statement::AssignPlace { place, value } => {
                    if projects(place) {
                        return false;
                    }
                    value
                }
                Statement::DictEntryUpdate { .. } => continue,
            };
            if let Rvalue::Closure { id, .. } = value
                && seen.insert(id.0)
                && let Some(closure) = mir.closures.iter().find(|candidate| candidate.id == *id)
                && !body_uses_bound_only_through_trait(
                    mir,
                    &closure.locals,
                    &closure.blocks,
                    bound_use,
                    seen,
                )
            {
                return false;
            }
            let through_bound = match value {
                Rvalue::BoundMethod {
                    receiver,
                    bound,
                    method: member,
                    ..
                }
                | Rvalue::BoundField {
                    receiver,
                    bound,
                    field: member,
                } => Some((receiver, *bound, *member)),
                _ => None,
            };
            if let Some((receiver, bound, member)) = through_bound
                && operand_is_param(receiver)
                && (bound != bound_use.bound
                    || bound_use.planned.is_none_or(|planned| planned.member(member).is_none()))
            {
                return false;
            }
            let mut projected = false;
            value.reads(|operand| {
                if operand_projects(operand) {
                    projected = true;
                }
            });
            if projected {
                return false;
            }
        }
        if let Some(terminator) = &block.terminator {
            let projected = match terminator {
                Terminator::Call { callee, args, .. } => {
                    let mut projects = args.iter().any(operand_projects);
                    if let smelt_mir::Callee::Op(op) = callee {
                        op.reads(|operand| projects |= operand_projects(operand));
                    }
                    projects
                }
                Terminator::Switch { cond: probe, .. }
                | Terminator::Match {
                    scrutinee: probe, ..
                } => operand_projects(probe),
                _ => false,
            };
            if projected {
                return false;
            }
        }
    }
    true
}

/// The call sites that forward a calling free function's own type parameter
/// into a bare bounded position of `function`: `(caller, caller's parameter)`
/// pairs.
///
/// [`bounded_type_param_liftable`] accepts such an argument when the caller's
/// parameter has the same bound, but the lifted signature is only callable
/// with it while the CALLER's parameter is itself a real generic carrying the
/// same trait. Whether it is depends on the caller's own trial, so
/// `EmitContext::populate_generic_functions` demotes `function` whenever a
/// caller listed here did not lift that parameter.
pub(crate) fn bound_forwarding_callers(mir: &Mir, function: &MirFunction) -> Vec<(FuncId, Symbol)> {
    let bounded = crate::bound_traits::declared_bounds(mir, &function.type_params);
    if bounded.is_empty() {
        return Vec::new();
    }
    let bare_positions: Vec<usize> = function
        .params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ty = function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)?
                .ty;
            matches!(mir.types.get(ty), Some(Type::TypeParam { name }) if bounded.contains_key(name))
                .then_some(index)
        })
        .collect();
    let mut found = Vec::new();
    for caller in mir.static_callers(function.id) {
        if caller.id == function.id {
            continue;
        }
        for block in &caller.blocks {
            let Some(Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if *target != function.id {
                continue;
            }
            for &position in &bare_positions {
                if let Some(arg) = args.get(position)
                    && let Ok(arg_ty) = generic_bindings::operand_type(mir, caller, arg)
                    && let Some(Type::TypeParam { name }) = mir.types.get(arg_ty)
                    && caller.type_params.iter().any(|declared| declared.name == *name)
                {
                    found.push((caller.id, *name));
                }
            }
        }
    }
    found
}

/// Return whether any call site in the crate passes an *erased* argument into a
/// position typed as one of `function`'s own type parameters.
///
/// Rust must be able to infer a unique concrete type argument at every call
/// site. When a generic free function is invoked with an argument whose static
/// type is already erased — `SmeltUnknown`, a union (also emitted as
/// `SmeltUnknown`), or a type parameter from a *different* scope — the emitted
/// argument does not pin the callee's type parameter, so monomorphization fails
/// with `E0283` ("type annotations needed"). Such a function cannot safely emit
/// real generics; it must fall back to the fully erased signature so its
/// parameter accepts the erased value directly.
///
/// This scans every function's `Call` terminators for calls that resolve to
/// `function` and checks the argument at each of its type-parameter positions.
fn called_with_erased_type_param_argument_memoized(
    mir: &Mir,
    function: &MirFunction,
    memo: &ErasedArgumentMemo,
) -> bool {
    let known = memo.borrow().get(&function.id).copied();
    known.unwrap_or_else(|| {
        let answer = called_with_erased_type_param_argument(mir, function);
        memo.borrow_mut().insert(function.id, answer);
        answer
    })
}

/// Whether some call passes an erased value at a bare type-parameter position.
fn called_with_erased_type_param_argument(mir: &Mir, function: &MirFunction) -> bool {
    // Parameter positions whose declared type is exactly one of the function's
    // own type parameters (`x: T`). Nested shapes (`T[]`) still bind `T` from
    // the argument's element type, so only bare-parameter positions are checked.
    let own_params: HashSet<Symbol> =
        function.type_params.iter().map(|param| param.name).collect();
    let bare_type_param_positions: Vec<usize> = function
        .params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ty = function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)?
                .ty;
            matches!(mir.types.get(ty), Some(Type::TypeParam { name }) if own_params.contains(name))
                .then_some(index)
        })
        .collect();
    if bare_type_param_positions.is_empty() {
        return false;
    }

    for caller in mir.static_callers(function.id) {
        for block in &caller.blocks {
            let Some(Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if *target != function.id {
                continue;
            }
            for &position in &bare_type_param_positions {
                let Some(arg) = args.get(position) else {
                    continue;
                };
                // A RECURSIVE call passing the function's own `T` (`mk(x)`
                // inside `mk<T>(x: T)`) pins `T` to itself: the parameter is
                // from this scope, not another one, so Rust infers it.
                if caller.id == function.id
                    && generic_bindings::operand_type(mir, caller, arg).is_ok_and(|ty| {
                        matches!(
                            mir.types.get(ty),
                            Some(Type::TypeParam { name }) if own_params.contains(name)
                        )
                    })
                {
                    continue;
                }
                // A calling free function forwarding its OWN type parameter,
                // bounded by the same class or interface as this position's,
                // pins the parameter as long as the caller's parameter is a
                // real generic too — which `populate_generic_functions`
                // enforces through `bound_forwarding_callers`.
                if forwards_same_bound(mir, function, position, caller, arg) {
                    continue;
                }
                if operand_type_is_erased(mir, caller, arg) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether argument `arg` of a call from `caller` into position `position` of
/// `function` is the caller's own type parameter bounded by the same class or
/// interface as the callee's bare parameter at that position.
fn forwards_same_bound(
    mir: &Mir,
    function: &MirFunction,
    position: usize,
    caller: &MirFunction,
    arg: &Operand,
) -> bool {
    let declared_bound = function
        .params
        .get(position)
        .and_then(|param| function.locals.get(usize::try_from(param.0).ok()?))
        .and_then(|local| match mir.types.get(local.ty) {
            Some(Type::TypeParam { name }) => function
                .type_params
                .iter()
                .find(|declared| declared.name == *name)
                .and_then(|declared| crate::bound_traits::constraint_bound(mir, declared.constraint)),
            _ => None,
        });
    let Some(callee_bound) = declared_bound else {
        return false;
    };
    matches!(caller.origin, smelt_mir::HirOrigin::Body(_))
        && generic_bindings::operand_type(mir, caller, arg).is_ok_and(|ty| {
            matches!(mir.types.get(ty), Some(Type::TypeParam { name }) if caller
                .type_params
                .iter()
                .any(|declared| declared.name == *name
                    && crate::bound_traits::constraint_bound(mir, declared.constraint) == Some(callee_bound)))
        })
}

/// Return whether an operand's static type is erased for codegen purposes
/// (`SmeltUnknown`, `Never`, a union — all emitted as `SmeltUnknown` — or a type
/// parameter that is not resolvable to a concrete type at the call site).
///
/// The place arm and the erasure test are shared with
/// [`crate::generic_bindings`] so this scan and the call-site binding matcher
/// cannot grow different notions of "erased argument". The three-valued binding
/// result collapses onto this boolean as follows:
///
/// | shared result | here | why |
/// | --- | --- | --- |
/// | resolved type | [`generic_bindings::actual_type_is_erased`] | the shared classifier |
/// | `Err(ProjectedOperand)` | erased | a field/index projection carries no type of its own |
/// | `Err(ShapeMismatch)` | erased | the caller local is missing, so nothing pins the parameter |
/// | `Err(MissingLiteralType)` | unreachable | constants short-circuit below |
///
/// The `Operand::Const` arm is deliberately *not* delegated. This scan treats
/// every literal as pinning, while `generic_bindings` resolves a real `TypeId`
/// and therefore classifies a JS symbol literal (`Type::Unknown`) as erased.
/// Widening this arm would demote functions that are generic today, so it stays
/// frozen until the increment that rewrites this predicate's caller.
fn operand_type_is_erased(mir: &Mir, caller: &MirFunction, operand: &Operand) -> bool {
    // Literal constants (numbers, strings, booleans) are concrete and pin
    // the type parameter, so they never force erasure.
    if matches!(operand, Operand::Const(_)) {
        return false;
    }
    match generic_bindings::operand_type(mir, caller, operand) {
        Ok(ty) => generic_bindings::actual_type_is_erased(mir, ty),
        // A missing local or a field/index projection is conservatively erased.
        Err(
            generic_bindings::BindingUnsupportedReason::ProjectedOperand
            | generic_bindings::BindingUnsupportedReason::ShapeMismatch,
        ) => true,
        Err(_) => false,
    }
}

/// Return whether `name` survives into the *emitted Rust* rendering of `ty`,
/// in a position Rust can infer a type argument from.
///
/// The question — "does the emitted Rust type still mention this parameter?" —
/// is identical inside and outside a callback, so this one walk is applied at
/// two sites: to a value parameter's own type (the *direct* half of the gate,
/// `T`, `T[]`, `Set<T>`, `Option<T>`, a tuple element, a dict key/value), and
/// by [`type_param_inferable_through_callback`] to each parameter and the
/// return of a callback that will be emitted as an `F{n}: Fn(..) + ?Sized`
/// bound. Writing a second enumeration for the callback half would let the two
/// drift; §4.3 of `blocker-logs/estk-callback-generics-plan.md` is really
/// guarding against that drift, so the callback half reuses this walk verbatim
/// and inherits its exclusions for free.
///
/// The walk must mirror what codegen actually EMITS for a type
/// (see `FunctionEmitter::rust_type`): it only descends through shapes
/// that preserve the type parameter in the emitted Rust. It intentionally does
/// NOT descend into:
/// - **`Union`** — a union erases to `SmeltUnknown` in emission, so a `T`
///   inside `T | string` disappears from the emitted signature and cannot be
///   inferred (`E0283`). A *concrete* union renders as its generated enum
///   with arguments resolved through the render position's substitution, but
///   inference through an enum argument is not modelled here, so the walk
///   stays conservative and treats the union as opaque;
/// - **`Function`** — a callback boundary. Reached by
///   [`type_param_in_callback`] as an *occurrence* walk, and as an inference
///   source only through [`type_param_inferable_through_callback`], which
///   descends exactly one level. A function type nested *inside* a callback is
///   no inference source: `callback_occurrences_are_liftable` already refuses
///   the shape, and a nested `&dyn Fn` / `Rc<dyn Fn>` argument position is
///   invariant.
///
/// Known over-approximation, shared by both callers and deliberately left
/// alone: a `Type::Class` whose name does not resolve renders `SmeltUnknown`
/// and drops its arguments, yet this walk descends into them. Tightening it
/// would move bytes that belong to Increment 1, so it belongs in its own
/// change.
///
/// This is deliberately *narrower* than
/// [`generic_bindings::match_types`](crate::generic_bindings), which does bind
/// through `Type::Function` at any depth.
fn type_param_preserved_in_emitted_type(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::TypeParam { name: param_name }) => *param_name == name,
        // Unions erase to `SmeltUnknown` and functions are a callback boundary,
        // so neither preserves the type parameter in a directly-inferable value
        // position.
        Some(Type::Union(_) | Type::Function(_)) | None => false,
        Some(other) => other
            .children()
            .any(|child| type_param_preserved_in_emitted_type(mir, child, name)),
    }
}

/// Return whether `name` appears anywhere inside a function-typed sub-shape of
/// `ty` (a callback parameter or return).
///
/// The walk descends through value wrappers to find a nested `Type::Function`,
/// then checks whether `name` occurs anywhere within that function type using
/// the shared [`generic_bindings::type_param_occurs`] walk.
///
/// This used to be the callback gate itself — any occurrence demoted the
/// function. Increment 1 of the callback-generics plan deleted that
/// all-or-nothing early return; the walk stayed, and is now the occurrence half
/// of the §4.4 eligibility rule implemented by
/// [`callback_occurrences_are_liftable`]: it answers "does `name` reach a
/// function-typed sub-shape of `ty` at all", and the caller decides whether the
/// position that occurrence sits in is one the renderer can express.
///
/// Increment 3 additionally turns this into
/// `type_param_inferable_through_callback` by restricting it to positions Rust
/// can *infer* from — a callback parameter or return, but not a `T` buried in a
/// union inside a callback parameter, since a union erases and never appears in
/// the emitted `Fn` bound. That restriction is deferred: it only matters once a
/// callback position becomes an inference source. Here the walk is used as a
/// *demotion* trigger, so being deliberately wide (it descends unions, tuples,
/// class arguments and generator shapes) is the conservative direction.
pub(crate) fn type_param_in_callback(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(function @ Type::Function(_)) => function
            .children()
            .any(|child| generic_bindings::type_param_occurs(mir, child, name)),
        Some(other) => other.children().any(|child| type_param_in_callback(mir, child, name)),
        None => false,
    }
}

/// Return the parameter index of the callback that lets Rust infer `name`, if
/// any.
///
/// Increment 3 of `blocker-logs/estk-callback-generics-plan.md`. A type
/// parameter that reaches no direct value parameter can still be inferred when
/// it occupies a position of a callback that Increment 2 emits as
/// `&F{n}` with an `F{n}: Fn(..) + ?Sized` bound: rustc infers the parameter
/// *through* that bound from the closure's own type. This is precisely why the
/// increment waited for Option B. A `&dyn Fn(..)` argument position is an
/// unsize coercion, which is far weaker — rustc must know the target type
/// before it can unsize, and `dyn Fn` is invariant in its argument types — so a
/// callback-only parameter cannot be inferred through the old representation
/// at all.
///
/// The two rules are therefore coupled, and the coupling is mechanical rather
/// than restated: step 1 below is the *same call*
/// [`callback_generic_params`] makes when it decides which parameters receive
/// an `F{n}` name. So "inferable through a callback" and "emitted as an `F{n}`
/// with an `Fn` bound" are the same set by construction. If they were allowed
/// to disagree, the gate would declare a `T` whose only home in the signature
/// was a `&dyn Fn(T)` argument position — E0283 at every call site.
///
/// The index rather than a bool is returned so
/// [`callback_only_params_are_pinned_at_every_call_site`] can scan exactly the
/// one argument position that carries the parameter. One decision flows through
/// the gate, the valve and the representation; there is no place for a second
/// opinion to form.
fn type_param_inferable_through_callback(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
    name: Symbol,
) -> Option<usize> {
    for (index, param) in function.params.iter().enumerate() {
        // The shape half of §4.4, asked through the renderer's own predicate.
        if !callback_param_shape_is_liftable(mir, function, owned, index, *param) {
            continue;
        }
        let local_index = id_index(param.0, "local index does not fit usize").ok()?;
        let local = function.locals.get(local_index)?;
        let Some(Type::Function(callback)) = mir.types.get(local.ty) else {
            continue;
        };
        // An erased-unknown-rest callback (`(...args: T[]) => unknown`) is the
        // shape `rust_type` renders as the concrete struct
        // `SmeltErasedFunction`. The *parameter* declaration of a borrowed one
        // still goes through `param_type`, so the callee would advertise
        // a real `Fn` bound — but every call site hands it a
        // `SmeltErasedFunction` VALUE (`function_shape_adapter` bails out
        // early when both sides are that shape), and that struct implements no
        // `Fn` trait, so there is nothing to infer through and the argument
        // would not even satisfy the bound. This exclusion lives
        // in the *inference* predicate rather than in the shared shape
        // predicate because it is an inference question, not a representation
        // one: moving it into `callback_param_shape_is_liftable` would
        // retroactively change Increment 2's `F{n}` set.
        if crate::emitter::is_erased_unknown_rest_function_in(&mir.types, callback) {
            continue;
        }
        // A callback *parameter* position, excluding the callback's own packed
        // rest parameter (reshaped by the rest-vector adapter rather than
        // passed positionally, so its declaration does not correspond to the
        // bound's argument slot), or the callback's return position.
        let inferable = callback
            .params
            .iter()
            .enumerate()
            .any(|(nested_index, nested)| {
                callback.rest != Some(nested_index)
                    && type_param_preserved_in_emitted_type(mir, *nested, name)
            })
            || type_param_preserved_in_emitted_type(mir, callback.return_ty, name);
        if inferable {
            return Some(index);
        }
    }
    None
}


/// Return whether every callback occurrence of `name` in `function`'s parameter
/// list sits in a position the current callback representation can express.
///
/// §4.4 of `blocker-logs/estk-callback-generics-plan.md` scopes callback-borne
/// generics to a **direct, required, borrowed, non-rest** `Type::Function`
/// parameter. A parameter that mentions `name` inside a function type in any
/// other shape demotes the whole function to erasure, because the callee
/// signature and the call-site argument would be rendered by two different
/// paths that cannot agree:
///
/// * **optional** (`Type::Optional(Function)`) — lowers to
///   `Option<Rc<dyn Fn(T) -> _>>`. An omitted argument is `None`, which carries
///   no closure and therefore no `T`; the call site must name a concrete
///   element type for the `None` and picks the erased one (`E0308`, and `E0631`
///   on the synthesized default closure). This is also exactly the shape
///   `emitter::callee_param_is_owned_callback_sink` classifies as an owned sink.
/// * **owned or escaping** — the emitter's ownership fixpoint decided this
///   parameter is stored, returned or captured, so it lowers to an owned
///   `Rc<dyn Fn(..)>` with a `'static` bound rather than a borrowed `&dyn Fn`.
///   The borrowed-callback argument ladder in `emitter::call` — the one that
///   renders the substituted adapter — declines those positions, so the
///   substituted signature would be met with an erased argument.
/// * **nested inside a container, or inside another function type** — the
///   occurrence is behind a wrapper the adapter renderers do not walk into, so
///   the callee would advertise `T` in a position no call site substitutes.
/// * **rest** (`function.rest == Some(index)`) — a packed rest parameter is
///   emitted as an erased sequence of callables; there is no single declared
///   callback whose type the call site can be substituted against.
///
/// `owned` is the emitter's `compute_owned_callback_params` fixpoint, threaded
/// in by the caller. It is deliberately *the same* set the renderer consults
/// (`FunctionEmitter::is_owned_callback_param`) so the gate cannot disagree with
/// the representation it is predicting.
///
/// Only parameter positions are examined. A callback in the *return* type is
/// produced by the callee's own body rather than supplied by a call site, so it
/// has no argument to disagree with.
fn callback_occurrences_are_liftable(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
    name: Symbol,
) -> bool {
    for (index, param) in function.params.iter().enumerate() {
        let Ok(local_index) = id_index(param.0, "local index does not fit usize") else {
            return false;
        };
        let Some(local) = function.locals.get(local_index) else {
            return false;
        };
        if !type_param_in_callback(mir, local.ty, name) {
            // No callback occurrence in this parameter at all; a direct value
            // position such as `T` or `T[]` is handled by
            // `type_param_preserved_in_emitted_type`.
            continue;
        }
        // There IS a callback occurrence here, so this parameter must be the one
        // eligible shape. The shape half of the rule lives in
        // `callback_param_shape_is_liftable` so the Increment-2 renderer asks
        // exactly this question and the two cannot grow two notions of it.
        if !callback_param_shape_is_liftable(mir, function, owned, index, *param) {
            return false;
        }
        let Some(Type::Function(callback)) = mir.types.get(local.ty) else {
            return false;
        };
        // Direct callback parameter — but the occurrence must be at its own
        // top level, not inside a further nested function type.
        if callback
            .params
            .iter()
            .any(|nested| type_param_in_callback(mir, *nested, name))
            || type_param_in_callback(mir, callback.return_ty, name)
        {
            return false;
        }
    }
    true
}

/// The *shape* half of §4.4: is parameter `index` a **direct, required,
/// borrowed, non-rest** `Type::Function` parameter?
///
/// Name-independent on purpose. [`callback_occurrences_are_liftable`] asks it
/// about the parameters that mention a given type parameter, and Increment 2's
/// `&F{n}` renderer ([`callback_generic_params`]) asks it about every parameter;
/// sharing one predicate is what keeps the gate and the representation from
/// growing two different notions of "liftable callback shape".
///
/// * **direct / required** — the local's type must be *exactly* `Type::Function`.
///   `MirFunction` has no separate optionality flag, so an optional parameter is
///   spelled `Type::Optional(Function)` and fails this test, which is precisely
///   the "required" half of the rule.
/// * **non-rest** — `function.rest == Some(index)` is the enclosing function's
///   packed rest parameter, which is emitted as an erased sequence of callables.
///   (Note this is the *enclosing* function's rest, not the callback's own: an
///   erased-unknown-rest callback type is still a perfectly liftable parameter.)
/// * **borrowed** — `owned` is the emitter's `compute_owned_callback_params`
///   fixpoint; an owned parameter lowers to `Rc<dyn Fn(..)>`, not to a borrow.
/// * **mentions a type parameter** — the parameter's type must mention a declared
///   type parameter of the function; otherwise there is nothing to carry into the
///   emitted signature.
pub(crate) fn callback_param_shape_is_liftable(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
    index: usize,
    param: LocalId,
) -> bool {
    let Ok(local_index) = id_index(param.0, "local index does not fit usize") else {
        return false;
    };
    let Some(local) = function.locals.get(local_index) else {
        return false;
    };
    if !matches!(mir.types.get(local.ty), Some(Type::Function(_))) {
        return false;
    }
    if function.rest == Some(index) {
        return false;
    }
    // The swap exists to carry a type parameter into the `Fn` bound. A callback
    // whose type is already fully concrete (`cb: (v: number) => boolean` inside
    // a generic function) has nothing to carry: it un-erases nothing, it buys a
    // monomorphized copy per call site, and — the reason this is a correctness
    // rule rather than a taste one — `&F0` with `F0: ?Sized` cannot unsize-coerce
    // to a `&dyn Fn(..)` parameter of some other callee, so forwarding it fails
    // with E0277. Such a parameter keeps `&dyn Fn(..)`, exactly as before.
    if !function
        .type_params
        .iter()
        .any(|type_param| generic_bindings::type_param_occurs(mir, local.ty, type_param.name))
    {
        return false;
    }
    !owned.contains(&(function.id, param))
}

/// The generated `F{n}` type-parameter name for each callback parameter of a
/// generic free function, in declaration order.
///
/// Increment 2 of `blocker-logs/estk-callback-generics-plan.md` renders a
/// direct required borrowed callback parameter as `&F{n}` with an
/// `F{n}: Fn(..) + ?Sized` bound (Option B) instead of `&dyn Fn(..)`, so a
/// hand-written Rust team's monomorphized dispatch replaces dynamic dispatch.
///
/// Scoping: only a function that *already* declares its own `<..>` list gets
/// `F{n}` parameters. This is a structural rule, not a per-function or
/// per-library exception, and it has two reasons. There is nowhere to declare
/// the name otherwise — [`function_impl_generics_list`] is empty for a
/// non-generic function and class members emit no generics suffix at all — and
/// converting an already-concrete `&dyn Fn()` parameter un-erases nothing while
/// buying a monomorphized copy per call site (plan §7, kill criterion 7).
///
/// Naming is deterministic by declaration index and skips any name that
/// collides with a sanitized source type-parameter identifier, so a source
/// `<F0>` pushes the first generated name to `F1`. Generated names appear after
/// the source generics in the emitted list.
pub(crate) fn callback_generic_params(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
    bounds: &BoundTraitPlan,
    erased_arguments: &ErasedArgumentMemo,
) -> Result<Vec<(LocalId, String)>, EmitError> {
    if !function_emits_rust_generics(mir, function, owned, bounds, erased_arguments) {
        return Ok(Vec::new());
    }
    let mut taken: HashSet<String> = HashSet::new();
    for type_param in &function.type_params {
        let name = mir
            .symbols
            .get(type_param.name)
            .ok_or_else(|| EmitError::new("function type parameter has unknown symbol"))?;
        taken.insert(RustIdent::new(name).into_string());
    }
    // Emitted crate types share the generated names' namespace. A source
    // `class F0` lowers to a Rust struct `F0`, and a generated type parameter
    // of the same name shadows it inside the signature, so a `new F0()` in the
    // body resolves against the type parameter instead of the struct. Reserving
    // every emitted class and interface name is the general rule; reserving only
    // source *type-parameter* names was too narrow.
    for class in &mir.classes {
        taken.insert(class_name_text(mir, class)?);
    }
    for interface in &mir.interfaces {
        let name = mir
            .symbols
            .get(interface.name)
            .ok_or_else(|| EmitError::new("interface has unknown symbol"))?;
        taken.insert(RustIdent::new(name).into_string());
    }
    let mut next = 0usize;
    let mut named = Vec::new();
    for (index, param) in function.params.iter().enumerate() {
        if !callback_param_shape_is_liftable(mir, function, owned, index, *param) {
            continue;
        }
        let name = loop {
            let candidate = format!("F{next}");
            next += 1;
            if !taken.contains(&candidate) {
                break candidate;
            }
        };
        named.push((*param, name));
    }
    Ok(named)
}

/// The bounded *source* type parameters of a generic free function, one entry
/// per declared parameter (`T: Clone + Default + IntoSmeltUnknown + ..`).
///
/// Split out of the former `function_impl_generics_text` so the free-function
/// signature
/// emitter can append Increment 2's generated `F{n}: Fn(..) + ?Sized` bounds —
/// which need the emitter's substitution-aware type lowering and therefore
/// cannot be rendered here — while keeping source generics first, in
/// declaration order. Empty for a function that stays erased.
pub(crate) fn function_impl_generics_list(
    mir: &Mir,
    function: &MirFunction,
    owned_callback_params: &HashSet<(FuncId, LocalId)>,
    bounds: &BoundTraitPlan,
    erased_arguments: &ErasedArgumentMemo,
) -> Result<Vec<String>, EmitError> {
    let liftable = liftable_type_params(mir, function, owned_callback_params, bounds, erased_arguments);
    if liftable.is_empty() {
        return Ok(Vec::new());
    }
    // Every lifted parameter carries the full bound set. Dropping halves of the
    // erasure round-trip was tried and reverted: the emitter inserts conversions
    // in *both* directions at call boundaries, invisibly to any analysis of the
    // MIR body. `flatMap` erases a `U` outbound to call `flatten`, and
    // `pull`/`xor`/`isSubset` un-erase a `T` inbound the same way. The fix is to
    // make the bounds satisfiable instead — generated records and unions now
    // emit `SmeltFromUnknown` (and unions `SmeltJsKeyEq`) rather than the bounds
    // being narrowed to suit them.
    //
    // Only the lifted parameters get a bound; a constrained sibling erases and
    // must not appear in the emitted generic list (Increment 5).
    function
        .type_params
        .iter()
        .filter(|param| liftable.contains(&param.name))
        .map(|param| {
            // A lifted bounded parameter also carries its bound's generated
            // trait (`A: AnimalLike + ..`); `liftable_type_params` only lifts a
            // constrained parameter whose bound has one.
            let bound_trait = if crate::bound_traits::constraint_is_date(mir, param.constraint) {
                "DateLike + ".to_owned()
            } else {
                crate::bound_traits::constraint_bound(mir, param.constraint)
                    .and_then(|bound| bounds.trait_for(bound))
                    .map(|planned| format!("{} + ", planned.trait_name))
                    .unwrap_or_default()
            };
            mir.symbols
                .get(param.name)
                .map(|name| {
                    format!(
                        "{}: {bound_trait}{GENERATED_TYPE_PARAM_BOUNDS}",
                        RustIdent::new(name).into_string()
                    )
                })
                .ok_or_else(|| EmitError::new("function type parameter has unknown symbol"))
        })
        .collect()
}
