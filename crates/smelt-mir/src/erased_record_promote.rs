//! Promote erased-and-mutated record locals to an erased value representation.
//!
//! JavaScript objects are reference values: passing an object somewhere and then
//! mutating the original binding must be observable through the alias. Smelt
//! represents a statically-typed object literal as a `SmeltRecord<K, V>` with a
//! concrete value type `V`. Erasing such a record to `SmeltUnknown::Object`
//! (which happens when it crosses a generic/`unknown` boundary) *copies* its
//! entries, because the typed `V` cannot share storage with the erased
//! `SmeltUnknown` map. A later mutation of the original binding is then invisible
//! through the erased alias — breaking reference identity (see remeda
//! `constant` "returns identity (doesn't clone)").
//!
//! When a record local is *both* mutated and erased-by-reference, lowering its
//! value type to `Unknown` lets the erasure share the underlying
//! `Rc<RefCell<..>>` (via `SmeltObject::from_unknown_record`), so the alias
//! observes the mutation. This is exactly the dynamic-boundary case where a
//! tagged `SmeltUnknown` representation is appropriate.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Type, TypeId};

use crate::{BodyOwner, Callee, FuncId, LocalId, Mir, MirBody, Operand, Place, Rvalue, SiteLookup,
    Statement, Terminator};

/// One local of one body: the unit promotion decides on.
///
/// Promotion is decided over the whole program, not per body, because a
/// closure capture ties a local of the constructing body to a local of the
/// closure body, and the two must keep one type (closure ABI invariant 4).
type Node = (BodyOwner, LocalId);

/// Each statically callable function's parameter types, positionally; `None`
/// for a parameter whose local is out of range.
type ParamTypes = HashMap<FuncId, Vec<Option<TypeId>>>;

/// What one body contributes to the promotion decision.
#[derive(Default)]
struct BodyFacts {
    /// Locals both mutated through a place and erased by reference.
    seeds: Vec<LocalId>,
    /// Undirected `dest = copy/move src` edges.
    copy_edges: Vec<(LocalId, LocalId)>,
}

/// Rewrite erased-and-mutated record locals to use an erased value type.
///
/// A seed's whole connected component is promoted: copy edges inside a body,
/// and capture edges between a constructing body's source local and the
/// closure body's target local. A promoted capture's `ty` follows its source
/// local, so after this pass every capture's type, source local type and
/// target local type are still one type (audit finding C2). A parameter is
/// never promoted (that would change the function's ABI); a capture tied to a
/// parameter therefore keeps BOTH ends unpromoted.
///
/// Types are interned in sorted node order so the type table does not depend
/// on hash iteration order (audit finding C14).
pub fn promote_erased_mutated_records(mir: &mut Mir) {
    // Parameter value types for each statically-known function, used to detect
    // arguments that are erased to `unknown`/a generic type parameter.
    //
    // One entry per parameter, `None` for an out-of-range parameter local, so
    // argument `i` always pairs with parameter `i` (audit C13: a `filter_map`
    // here shifted every later parameter past a bad one).
    let func_param_tys: ParamTypes = mir
        .functions
        .iter()
        .map(|function| {
            let body = function.body();
            let tys = function
                .params
                .iter()
                .map(|local| body.local_decl(*local).map(|decl| decl.ty))
                .collect::<Vec<_>>();
            (function.id, tys)
        })
        .collect();

    let mut seeds: Vec<Node> = Vec::new();
    let mut edges: Vec<(Node, Node)> = Vec::new();
    let mut params: HashSet<Node> = HashSet::new();
    for body in mir.bodies() {
        let facts = analyze_body(&mir.types, &func_param_tys, body);
        seeds.extend(facts.seeds.into_iter().map(|local| (body.owner, local)));
        edges.extend(
            facts
                .copy_edges
                .into_iter()
                .map(|(dest, src)| ((body.owner, dest), (body.owner, src))),
        );
        params.extend(body.params.iter().map(|local| (body.owner, *local)));
    }
    if seeds.is_empty() {
        return;
    }
    let capture_edges = capture_edges(mir);
    edges.extend(capture_edges.iter().copied());

    let mut promoted: HashSet<Node> = seeds.into_iter().collect();
    expand_copy_component(&mut promoted, &edges);
    promoted.retain(|node| !params.contains(node));
    // A capture whose one end is a parameter cannot move its other end alone.
    let mut changed = true;
    while changed {
        changed = false;
        for (source, target) in &capture_edges {
            if promoted.contains(source) != promoted.contains(target) {
                changed |= promoted.remove(source) | promoted.remove(target);
            }
        }
    }

    let mut ordered: Vec<Node> = promoted.into_iter().collect();
    ordered.sort_by_key(|(owner, local)| (owner_key(*owner), local.0));
    let mut retyped: HashMap<Node, TypeId> = HashMap::new();
    for node in ordered {
        let (owner, local) = node;
        let Some(old_ty) = mir.body(owner).and_then(|body| body.local_decl(local)).map(|decl| decl.ty)
        else {
            continue;
        };
        let Some(new_ty) = promoted_type(&mut mir.types, old_ty) else {
            continue;
        };
        if let Some(decl) =
            body_locals_mut(mir, owner).and_then(|locals| locals.get_mut(local.index()))
        {
            decl.ty = new_ty;
            retyped.insert(node, new_ty);
        }
    }
    sync_promoted_capture_types(mir, &retyped);
}

/// A sort key for a body owner: functions first, then closures, by id.
const fn owner_key(owner: BodyOwner) -> (u8, u32) {
    match owner {
        BodyOwner::Function(id) => (0, id.0),
        BodyOwner::Closure(id) => (1, id.0),
    }
}

/// The locals of the body `owner` names, mutably.
fn body_locals_mut(mir: &mut Mir, owner: BodyOwner) -> Option<&mut Vec<crate::LocalDecl>> {
    match owner {
        BodyOwner::Function(id) => mir.functions.get_mut(id.index()).map(|f| &mut f.locals),
        BodyOwner::Closure(id) => mir.closures.get_mut(id.index()).map(|c| &mut c.locals),
    }
}

/// `Dict(K, V)` → `Dict(K, unknown)`, or `None` when `ty` is not a record with
/// a typed value.
fn promoted_type(types: &mut smelt_hir::TypeInterner, ty: TypeId) -> Option<TypeId> {
    let Some(Type::Dict(key_ty, value_ty)) = types.get(ty).cloned() else {
        return None;
    };
    if matches!(types.get(value_ty), Some(Type::Unknown)) {
        return None;
    }
    let unknown_ty = types.intern(Type::Unknown);
    Some(types.intern(Type::Dict(key_ty, unknown_ty)))
}

/// Every capture as an undirected edge from the constructing body's source
/// local to the closure body's target local.
fn capture_edges(mir: &Mir) -> Vec<(Node, Node)> {
    let mut edges = Vec::new();
    for (lookup, closure) in mir.closure_sites().into_iter().zip(&mir.closures) {
        let SiteLookup::One(site) = lookup else {
            continue;
        };
        for capture in &closure.captures {
            if let Some(target) = capture.target_local {
                edges.push((
                    (site.owner, capture.source_local),
                    (BodyOwner::Closure(closure.id), target),
                ));
            }
        }
    }
    edges
}

/// A capture to retype: closure position, capture position, new type.
type CaptureRetype = (usize, usize, TypeId);

/// Give each capture whose source local was promoted the source's new type.
fn sync_promoted_capture_types(mir: &mut Mir, retyped: &HashMap<Node, TypeId>) {
    let updates: Vec<CaptureRetype> = mir
        .closure_sites()
        .into_iter()
        .zip(&mir.closures)
        .enumerate()
        .flat_map(|(closure_index, (lookup, closure))| {
            let owner = match lookup {
                SiteLookup::One(site) => Some(site.owner),
                SiteLookup::Unconstructed | SiteLookup::Many => None,
            };
            closure.captures.iter().enumerate().filter_map(move |(position, capture)| {
                let ty = retyped.get(&(owner?, capture.source_local))?;
                Some((closure_index, position, *ty))
            })
        })
        .collect();
    for (closure_index, position, ty) in updates {
        if let Some(capture) = mir
            .closures
            .get_mut(closure_index)
            .and_then(|closure| closure.captures.get_mut(position))
        {
            capture.ty = ty;
        }
    }
}

/// Collect one body's promotion seeds and copy edges.
fn analyze_body(
    types: &smelt_hir::TypeInterner,
    func_param_tys: &ParamTypes,
    body: MirBody<'_>,
) -> BodyFacts {
    let mut mutated: HashSet<LocalId> = HashSet::new();
    let mut erased: HashSet<LocalId> = HashSet::new();
    // Undirected copy edges between locals (`dest = copy/move src`). A local that
    // is both mutated and erased seeds promotion, and its copy-edge component is
    // promoted with it so a record literal feeding a binding is promoted alongside
    // the binding.
    let mut copy_edges: Vec<(LocalId, LocalId)> = Vec::new();

    let note_erased_operand = |erased_set: &mut HashSet<LocalId>, operand: &Operand| {
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand {
            erased_set.insert(*local);
        }
    };

    for block in body.blocks {
        for statement in &block.statements {
            match statement {
                Statement::AssignPlace { place, value } => {
                    match place {
                        Place::Field { base, .. } | Place::Index { base, .. } => {
                            mutated.insert(*base);
                        }
                        // This analysis tracks mutated LOCALS so an erased
                        // record can be promoted back to a struct. A
                        // global-rooted write mutates a `thread_local!` cell,
                        // which is not a local and is never a promotion
                        // candidate, so it contributes nothing here.
                        Place::Local(_) | Place::Global { .. } => {}
                    }
                    // A value erased through an explicit cast escapes by reference.
                    if let Rvalue::UnknownCast {
                        value: cast_value, ..
                    } = value
                    {
                        note_erased_operand(&mut erased, cast_value);
                    }
                }
                Statement::Assign { dest, value } => {
                    // Only alias copies and explicit erasure casts feed this
                    // analysis; every other rvalue is irrelevant here, so an
                    // if-let chain avoids an exhaustive `Rvalue` listing.
                    if let Rvalue::Use(
                        Operand::Copy(Place::Local(src)) | Operand::Move(Place::Local(src)),
                    ) = value
                    {
                        copy_edges.push((*dest, *src));
                    } else if let Rvalue::UnknownCast {
                        value: cast_value, ..
                    } = value
                    {
                        note_erased_operand(&mut erased, cast_value);
                    }
                }
                // The fused dict-entry update writes THROUGH its base, so the
                // base counts as mutated exactly as an `AssignPlace` to
                // `base[index]` does. Nothing here erases a value: `value`
                // reads only `current` and constants by construction.
                Statement::DictEntryUpdate { base, .. } => {
                    mutated.insert(*base);
                }
            }
        }
        if let Some(Terminator::Call { callee, args, .. }) = &block.terminator
            && let Callee::Static(func_id) = callee
            && let Some(param_tys) = func_param_tys.get(func_id)
        {
            for (index, arg) in args.iter().enumerate() {
                let Some(Some(param_ty)) = param_tys.get(index) else {
                    continue;
                };
                if matches!(
                    types.get(*param_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) {
                    note_erased_operand(&mut erased, arg);
                }
            }
        }
    }

    let mut seeds: Vec<LocalId> = mutated.intersection(&erased).copied().collect();
    seeds.sort_by_key(|local| local.0);
    BodyFacts { seeds, copy_edges }
}

/// Grow `seed` to include every local reachable through undirected copy edges.
fn expand_copy_component(seed: &mut HashSet<Node>, copy_edges: &[(Node, Node)]) {
    let mut adjacency: HashMap<Node, Vec<Node>> = HashMap::new();
    for (dest, src) in copy_edges {
        adjacency.entry(*dest).or_default().push(*src);
        adjacency.entry(*src).or_default().push(*dest);
    }
    let mut stack: Vec<Node> = seed.iter().copied().collect();
    while let Some(local) = stack.pop() {
        let Some(neighbors) = adjacency.get(&local) else {
            continue;
        };
        for neighbor in neighbors {
            if seed.insert(*neighbor) {
                stack.push(*neighbor);
            }
        }
    }
}
