//! Fusion of a dictionary-entry read-modify-write into one container probe.
//!
//! A JavaScript counting accumulator updates one entry through itself:
//!
//! ```text
//! result[key] = (result[key] ?? 0) + 1;
//! ```
//!
//! HIR has no notion of "borrow the value stored at this place", so that source
//! lowers to a read, a compute and a write-back, each naming the entry:
//!
//! ```text
//! bb2:
//!   %8 = closure_call copy %1(copy %0[copy %3])
//!   %4 = move %8
//!   %9 = copy %2[copy %4] ?? 0.0     // read the entry (or its default)
//!   %10 = move %9 + 1.0              // compute the new value
//!   %2[copy %4] = move %10           // write it back
//! ```
//!
//! This pass rewrites the last three statements into one
//! [`Statement::DictEntryUpdate`]:
//!
//! ```text
//!   entry_update %2[copy %4] ?? 0.0 as %9 = move %9 + 1.0
//! ```
//!
//! which the Rust backend renders through the container's entry accessor —
//! `result.entry_or_insert(key, || 0.0)` — so the map is hashed and scanned
//! ONCE instead of twice.
//!
//! # Why it is worth doing
//!
//! The read emits `result.get(&smelt_property_key(key.clone())).unwrap_or(0.0)`
//! and the write-back emits `result.insert(smelt_property_key(key.clone()), v)`:
//! two probes and two key allocations per element. Under callgrind,
//! `SmeltJsMapStore::position` (18.8%) + `smelt_js_member_hash_key` (14.0%) +
//! `same_js_key` (7.8%) + `memcmp` (7.3%) were 47.9% of es-toolkit's `countBy`.
//! A team hand-writing this in Rust writes `*result.entry(key).or_insert(0.0) +=
//! 1.0;` and probes once; this pass is what makes the emitted code do the same.
//!
//! # Why the rewrite preserves behaviour
//!
//! The fused form seeds an absent entry with the coalesce's own fallback and
//! then overwrites it, where the source form computed with the fallback and
//! inserted the result. Both end with the same value under the same key. The
//! key set is also identical: the source form's `insert` always creates the
//! entry, and `entry_or_insert` always creates it too. Insertion ORDER matches
//! because the two spellings create the key at the same program point —
//! condition 1 leaves no statement between the read and the write-back, and
//! condition 7 forbids the stored rvalue from touching the container, so
//! nothing can observe the map between the probe and the store. For a key that
//! is already present, both spellings overwrite in place and neither moves it.
//!
//! # Correctness conditions
//!
//! The rewrite only fires when all of the following hold.
//!
//! 1. The three statements are **consecutive in one basic block**. Nothing can
//!    run between them, so the container and the key cannot change, and
//!    evaluating the key once instead of twice cannot observe a difference.
//! 2. The read is a [`Rvalue::OptionalCoalesce`] whose optional operand reads
//!    `base[index]` and whose fallback is a **constant**. Requiring the
//!    coalesce spelling is what supplies the seed: a bare entry read carries no
//!    "value when absent" to hand the entry accessor. Requiring a constant
//!    fallback is a borrow condition, not a convenience — `SmeltJsMap`'s
//!    `entry_or_insert` calls the seed closure while it holds `store
//!    .borrow_mut()`, so a seed that could reach the container would panic with
//!    `already borrowed`.
//! 3. The write-back is a [`Statement::AssignPlace`] to a [`Place::Index`] with
//!    the **same base local and a structurally equal key operand** as the read,
//!    storing the middle statement's destination.
//! 4. `current` (the read's destination) and the middle statement's destination
//!    are both [`crate::LocalKind::Temp`], each **assigned exactly once and read
//!    exactly once** in the whole function. A second reader of either would
//!    observe a value the rewrite folds away; a closure capture counts as such a
//!    read because [`Rvalue::Closure`] carries its captures as operands.
//! 5. `base` is typed `Dict<_, V>`, and `current` and the middle destination are
//!    both typed exactly `V`. Equality with `V` — rather than "convertible to" —
//!    is what rules out an optional-valued dict, where `??` could fire on a
//!    stored null rather than on absence and the fused form would bind the
//!    stored null instead of the fallback.
//! 6. `V` is a **primitive scalar** (`Bool`, `Int`, `Float`, `String`). See
//!    condition 7 for why: over a primitive, an arithmetic/logical rvalue is
//!    closed arithmetic with no user dispatch, and the backend's
//!    `(*slot).clone()` is a trivial copy rather than a container clone.
//! 7. **The borrow hazard**, the load-bearing condition. The backend holds a
//!    `RefMut` guard into the container's `RefCell` store while it evaluates the
//!    stored rvalue (see the `entry_or_insert` docstrings in
//!    `smelt-codegen-rust/src/lib.rs`: the caller must not touch the same map
//!    while holding the guard). `list_push_text` sidesteps this by
//!    materializing the pushed item BEFORE the accessor call; this pass cannot,
//!    because the stored value depends on the entry. So instead the rvalue must
//!    be provably unable to reach the container:
//!
//!    * it is a [`Rvalue::Use`], [`Rvalue::Unary`] or [`Rvalue::Binary`] — an
//!      allowlist of three rvalues that the backend renders as a Rust
//!      expression over their operands, never as a call into user code. Every
//!      other rvalue is declined, including ones that look pure: a method call,
//!      a closure call or a container operation could re-enter the map through
//!      a value it is handed;
//!    * **every** operand is either an [`Operand::Const`] or a read of
//!      `Place::Local(current)` — nothing else. Not `base`; not a field or
//!      index projection (which names some other local's interior); not any
//!      other local, because a `SmeltJsMap`/`SmeltRecord` clone SHARES its
//!      store, so a second local can name the same `RefCell` and a syntactic
//!      "does it mention `base`" test would miss it. Aliasing is a runtime
//!      property, so the only safe rule is to admit no locals at all beyond the
//!      one the statement itself binds. Combined with condition 6 the operands
//!      are scalars, which own nothing and can reach nothing.
//! 8. The key operand does not read `base`. The key is an argument of the
//!    accessor call, and `SmeltJsMap::entry_or_insert` takes `&mut self`, so a
//!    key expression that also borrowed the container would not even compile
//!    (E0502). It must not read `current` or the middle destination either,
//!    neither of which is defined yet where the fused statement evaluates it.
//!
//! Conditions 2, 7 and 8 together mean nothing evaluated inside the entry's
//! borrow — and nothing evaluated as an argument to the accessor — can name the
//! container. A shape this pass cannot prove safe keeps the two-probe form,
//! which is slower but always correct.

use smelt_hir::{Type, TypeInterner};

use crate::{LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement};

use super::Pass;
use super::local_use::{
    local_assignment_count, local_decl, local_is_temp, local_read_count, operand_local,
    operand_place,
};

/// Fuses a dict-entry read/compute/write-back triple into one entry update.
#[derive(Debug, Default)]
pub struct DictEntryUpdate;

impl Pass for DictEntryUpdate {
    fn name(&self) -> &'static str {
        "dict-entry-update"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= run_function(function, &mir.types);
        }
        changed
    }
}

/// A fusable triple, identified by its first statement.
struct Fusion {
    /// Index of the block holding the triple.
    block: usize,
    /// Index of the entry read inside that block.
    start: usize,
}

/// Fuse every eligible triple in one function. Returns true if it was modified.
fn run_function(function: &mut MirFunction, types: &TypeInterner) -> bool {
    let fusions = collect_fusions(function, types);
    if fusions.is_empty() {
        return false;
    }
    // Applied back to front so the earlier fusions' recorded indices stay valid
    // while later ones remove statements.
    for fusion in fusions.iter().rev() {
        apply_fusion(function, fusion);
    }
    true
}

/// Find the triples that may be fused, in program order.
///
/// Overlapping candidates cannot occur: each statement of a triple has a
/// distinct shape (an entry read, an arithmetic assignment, an index store) and
/// the two temporaries are single-assignment, so no statement can be claimed by
/// two triples.
fn collect_fusions(function: &MirFunction, types: &TypeInterner) -> Vec<Fusion> {
    let mut fusions = Vec::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (start, window) in block.statements.windows(3).enumerate() {
            if triple_is_fusable(function, types, window) {
                fusions.push(Fusion {
                    block: block_index,
                    start,
                });
            }
        }
    }
    fusions
}

/// Whether three consecutive statements are a fusable entry read-modify-write.
fn triple_is_fusable(
    function: &MirFunction,
    types: &TypeInterner,
    statements: &[Statement],
) -> bool {
    // Condition 1 is structural: the caller only offers consecutive windows.
    let [
        Statement::Assign {
            dest: current,
            value:
                Rvalue::OptionalCoalesce {
                    optional,
                    fallback: default,
                },
        },
        Statement::Assign {
            dest: stored_dest,
            value: stored,
        },
        Statement::AssignPlace {
            place:
                Place::Index {
                    base: write_base,
                    index: write_index,
                    ..
                },
            value: Rvalue::Use(write_source),
        },
    ] = statements
    else {
        return false;
    };
    // Condition 2: the read names an entry and carries a constant seed.
    let Some(Place::Index {
        base: read_base,
        index: read_index,
        ..
    }) = operand_place(optional)
    else {
        return false;
    };
    if !matches!(default, Operand::Const(_)) {
        return false;
    }
    // Condition 3: the write-back targets the same entry and stores the middle
    // statement's result.
    if read_base != write_base || read_index != write_index {
        return false;
    }
    if operand_local(write_source) != Some(*stored_dest) {
        return false;
    }
    // Condition 4: both temporaries are private to the triple.
    if !single_use_temp(function, *current) || !single_use_temp(function, *stored_dest) {
        return false;
    }
    // Conditions 5 and 6: the entry type is a scalar the triple agrees on.
    if !dict_entry_types_match(function, types, *read_base, *current, *stored_dest) {
        return false;
    }
    // Condition 7: nothing evaluated under the entry borrow can reach the map.
    if !rvalue_is_borrow_safe(stored, *current) {
        return false;
    }
    // Condition 8: the key is evaluated as an accessor argument.
    key_is_borrow_safe(read_index, *read_base, *current, *stored_dest)
}

/// Whether a local is a temporary that is assigned once and read once.
fn single_use_temp(function: &MirFunction, local: LocalId) -> bool {
    local_is_temp(function, local)
        && local_assignment_count(function, local) == 1
        && local_read_count(function, local) == 1
}

/// Whether `base` is a dict whose value type is a scalar shared by both temps.
///
/// Demanding exact equality with the dict's value type (conditions 5 and 6) is
/// what keeps the seeded entry and the coalesce fallback interchangeable: an
/// optional-valued dict would let `??` fire on a stored null, which
/// `entry_or_insert` does not treat as absence.
fn dict_entry_types_match(
    function: &MirFunction,
    types: &TypeInterner,
    base: LocalId,
    current: LocalId,
    stored_dest: LocalId,
) -> bool {
    let (Some(base_decl), Some(current_decl), Some(stored_decl)) = (
        local_decl(function, base),
        local_decl(function, current),
        local_decl(function, stored_dest),
    ) else {
        return false;
    };
    if current_decl.ty != stored_decl.ty {
        return false;
    }
    if !matches!(
        types.get(current_decl.ty),
        Some(Type::Bool | Type::Int | Type::Float | Type::String)
    ) {
        return false;
    }
    matches!(types.get(base_decl.ty), Some(Type::Dict(_, value)) if *value == current_decl.ty)
}

/// Whether an rvalue may be evaluated while the entry's borrow guard is held.
///
/// See condition 7: an allowlist of three operand-shaped rvalues, every operand
/// of which must be a constant or a direct read of the bound entry value.
fn rvalue_is_borrow_safe(value: &Rvalue, current: LocalId) -> bool {
    if !matches!(
        value,
        Rvalue::Use(_) | Rvalue::Unary { .. } | Rvalue::Binary { .. }
    ) {
        return false;
    }
    let mut safe = true;
    value.for_each_operand(|operand| {
        safe &= match operand {
            Operand::Const(_) => true,
            Operand::Copy(place) | Operand::Move(place) => *place == Place::Local(current),
        };
    });
    safe
}

/// Whether the key operand may be evaluated as an argument of the accessor call.
///
/// See condition 8: it may not name the container (the accessor borrows it
/// mutably) nor either temporary of the triple (neither is live at that point).
fn key_is_borrow_safe(
    index: &Operand,
    base: LocalId,
    current: LocalId,
    stored_dest: LocalId,
) -> bool {
    match operand_place(index) {
        None => true,
        Some(Place::Local(local)) => *local != base && *local != current && *local != stored_dest,
        // A field or index projection reads some other local's interior, which
        // may share a store with the container; decline rather than guess.
        Some(Place::Field { .. } | Place::Index { .. }) => false,
    }
}

/// Replace the triple with one [`Statement::DictEntryUpdate`].
fn apply_fusion(function: &mut MirFunction, fusion: &Fusion) {
    let Some(block) = function.blocks.get_mut(fusion.block) else {
        return;
    };
    let stored_index = fusion.start.saturating_add(1);
    let write_index = fusion.start.saturating_add(2);
    let Some(Statement::Assign {
        dest: current,
        value:
            Rvalue::OptionalCoalesce {
                optional,
                fallback: default,
            },
    }) = block.statements.get(fusion.start)
    else {
        return;
    };
    let Some(Place::Index { base, index, .. }) = operand_place(optional) else {
        return;
    };
    let fused_base = *base;
    let fused_index = (**index).clone();
    let fused_default = default.clone();
    let fused_current = *current;
    let Some(Statement::Assign { value, .. }) = block.statements.get(stored_index) else {
        return;
    };
    let fused_value = value.clone();
    block.statements.remove(write_index);
    block.statements.remove(stored_index);
    let Some(slot) = block.statements.get_mut(fusion.start) else {
        return;
    };
    *slot = Statement::DictEntryUpdate {
        base: fused_base,
        index: fused_index,
        default: fused_default,
        current: fused_current,
        value: fused_value,
    };
}
