//! Elision of a guarded default insert that a following entry mutation subsumes.
//!
//! JavaScript accumulator loops seed a missing group before appending to it:
//!
//! ```text
//! if (!Object.hasOwn(result, key)) { result[key] = []; }
//! result[key].push(item);
//! ```
//!
//! After [`super::DictEntryInPlaceMutation`] has fused the read-modify-write
//! triple of the last line, that source lowers to:
//!
//! ```text
//! bb2:
//!   %10 = dict_contains_key copy %2, copy %5
//!   %11 = !move %10
//!   switch move %11 ? bb5 : bb6
//! bb5:
//!   %12 = []
//!   %2[copy %5] = move %12
//!   goto bb6
//! bb6:
//!   %14 = list_push copy %2[copy %5], move %4
//! ```
//!
//! and this pass rewrites `bb2`'s terminator to `goto bb6` and deletes `%10`
//! and `%11`, leaving `bb5` unreachable.
//!
//! # Why the seeding store is redundant
//!
//! The Rust backend renders a [`Rvalue::ListPush`] whose receiver is a
//! [`Place::Index`] as `m.entry_or_insert(k, || <default for the value type>)
//! .borrow_mut().push(item)` (see `list_push_text` in
//! `smelt-codegen-rust/src/emitter/list_mutation.rs`). `entry_or_insert` already
//! means "the entry at `k`, inserting the value type's default first when `k` is
//! absent". So the guarded store performs exactly the work the mutation is about
//! to perform anyway: under either spelling the entry holds a freshly built
//! empty container by the time the push runs, and the map ends up with the same
//! keys in the same insertion order — the key is inserted at the same program
//! point either way, because the guard and the mutation are adjacent.
//!
//! The one value the two forms could disagree on is the JavaScript object
//! identity (`SmeltList::id`) of the empty container. Both spellings mint a
//! brand-new id for a container that nothing else can yet reference — the guard
//! stores its list and immediately falls through, and `entry_or_insert` builds
//! its default inside the accessor — so no program can observe which one it got.
//!
//! # Why it is worth doing
//!
//! `contains_key`, the guarded `insert` and `entry_or_insert` each hash the key
//! and scan its bucket, so the source shape costs three probes per element where
//! one suffices. `SmeltJsMapStore::position` + `smelt_js_member_hash_key` +
//! `same_js_key` were 43% of es-toolkit's `group_by` under callgrind. A team
//! hand-writing this in Rust would write `result.entry(key).or_default()
//! .push(item)` and probe once; this pass is what makes the emitted code do the
//! same.
//!
//! # Correctness conditions
//!
//! The rewrite only fires when all of the following hold.
//!
//! 1. The guard block's terminator is [`Terminator::Switch`].
//! 2. The switch condition reads a temp `%c` whose single assignment is the
//!    statement immediately before the terminator and is a [`Rvalue::Unary`]
//!    logical `!` over an operand reading a temp `%d`.
//! 3. `%d`'s single assignment is the statement immediately before that one, and
//!    is a [`Rvalue::DictContainsKey`].
//! 4. `%c` and `%d` are both [`crate::LocalKind::Temp`], each assigned exactly
//!    once and read exactly once in the whole function. Any second reader would
//!    observe a value the rewrite deletes.
//! 5. The `then_block` has exactly one predecessor (the guard block) and its
//!    terminator is `goto else_block` — the taken branch rejoins immediately at
//!    the untaken successor, so the two paths differ only by the seeding store.
//!    The single predecessor is what makes leaving the block behind harmless:
//!    once the guard's terminator no longer names it, nothing reaches it.
//! 6. The `then_block` carries no phis and exactly two statements: a temp `%t`
//!    assigned the value type's default aggregate, then an
//!    [`Statement::AssignPlace`] storing `%t` into `dict[key]`, where the base
//!    and index read the same place as the [`Rvalue::DictContainsKey`]'s `dict`
//!    and `key` operands. `%t` is a temp assigned once and read once.
//! 7. The default aggregate is an EMPTY [`Rvalue::List`] and the dict's value
//!    type is exactly `%t`'s list type. Only list values are accepted: the
//!    backend's `entry_or_insert` spelling is reached solely from `list_push_text`,
//!    whose entry path requires the dict's value type to equal the pushed list's
//!    type, so a dict- or set-valued entry can never reach the mutation shape
//!    condition 8 demands. Restricting the pass to lists therefore costs nothing
//!    and keeps the "the two defaults agree" claim checkable against a single
//!    emitter site — `default_value(Type::List(item))` builds
//!    `SmeltList::new(Vec::<item>::new())`, which is what an empty list literal
//!    builds too. A non-empty literal, a call, or a copy of another local is
//!    rejected: those are values `entry_or_insert` would not reproduce.
//! 8. The `else_block` (the join) begins with an ENTRY-CREATING use of the same
//!    dict at the same key: its first statement assigns a [`Rvalue::ListPush`]
//!    whose receiver is `dict[key]`. This condition is load-bearing. If the join
//!    instead only READ `m[k]`, the backend would emit
//!    `.get(k).cloned().unwrap_or(missing)`, which does not insert, and deleting
//!    the guarded write would leave the key absent from the map — an observable
//!    change. Only the `entry_or_insert` spelling makes the elision sound.
//! 9. Nothing between the [`Rvalue::DictContainsKey`] and the switch can modify
//!    the dict or the key. Conditions 2 and 3 already force those three to be
//!    the last three items of the guard block, back to back, so there is no
//!    statement in between to check.
//!
//! The join block is required to carry no phis: the edge from `then_block` goes
//! dead, and a phi would need its incoming entry for that edge dropped rather
//! than left dangling.

use smelt_hir::{Type, TypeInterner, UnaryOp};

use crate::{BasicBlock, BlockId, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

use super::Pass;
use super::local_use::{
    local_assignment_count, local_decl, local_is_temp, local_read_count, operand_local,
    operand_place,
};

/// The dict entry a guard tests: the container local and the key it is indexed by.
#[derive(Clone, Copy)]
struct Entry<'a> {
    /// Local holding the dictionary.
    base: LocalId,
    /// Place the key operand reads.
    key: &'a Place,
}

/// The blocks and entry a candidate guard is made of.
struct GuardShape<'a> {
    /// Index of the block ending in the guard's switch.
    guard: usize,
    /// Block taken when the key is absent, holding the seeding store.
    then_block: BlockId,
    /// Block both paths continue into.
    join: BlockId,
    /// The dict entry the guard tests.
    entry: Entry<'a>,
}

/// Deletes a guarded default insert whose join already inserts through the entry.
#[derive(Debug, Default)]
pub struct DictDefaultInsertElision;

impl Pass for DictDefaultInsertElision {
    fn name(&self) -> &'static str {
        "dict-default-insert-elision"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= run_function(function, &mir.types);
        }
        changed
    }
}

/// One elidable guard, identified by the block holding it.
struct Elision {
    /// Index of the guard block whose switch becomes a `goto`.
    guard: usize,
    /// The join block the guard falls through to.
    join: BlockId,
}

/// Elide every eligible guard in one function. Returns true if it was modified.
///
/// Candidates cannot interact: each one only rewrites its own guard block, and a
/// guard block can neither be another candidate's `then_block` (which holds two
/// statements and a `goto`, not a switch) nor lose its leading statement to
/// another candidate's rewrite (which only ever deletes the last two).
fn run_function(function: &mut MirFunction, types: &TypeInterner) -> bool {
    let elisions: Vec<Elision> = (0..function.blocks.len())
        .filter_map(|guard| elision_at(function, types, guard))
        .collect();
    if elisions.is_empty() {
        return false;
    }
    for elision in &elisions {
        apply_elision(function, elision);
    }
    true
}

/// The elision available at `guard`, if that block ends in an elidable guard.
fn elision_at(function: &MirFunction, types: &TypeInterner, guard: usize) -> Option<Elision> {
    let block = function.blocks.get(guard)?;
    // Conditions 1-3: the switch and the two statements that feed it.
    let Some(Terminator::Switch {
        cond,
        then_block,
        else_block,
    }) = block.terminator.as_ref()
    else {
        return None;
    };
    let condition_local = operand_local(cond)?;
    let statement_count = block.statements.len();
    let not_index = statement_count.checked_sub(1)?;
    let contains_index = statement_count.checked_sub(2)?;
    let Some(Statement::Assign {
        dest: not_dest,
        value:
            Rvalue::Unary {
                op: UnaryOp::Not,
                operand: not_operand,
            },
    }) = block.statements.get(not_index)
    else {
        return None;
    };
    if *not_dest != condition_local {
        return None;
    }
    let contains_local = operand_local(not_operand)?;
    let Some(Statement::Assign {
        dest: contains_dest,
        value: Rvalue::DictContainsKey { dict, key },
    }) = block.statements.get(contains_index)
    else {
        return None;
    };
    if *contains_dest != contains_local {
        return None;
    }
    // Condition 4: both temps are single-assignment and single-read.
    if !single_use_temp(function, condition_local) || !single_use_temp(function, contains_local) {
        return None;
    }
    let shape = GuardShape {
        guard,
        then_block: *then_block,
        join: *else_block,
        entry: Entry {
            base: operand_local(dict)?,
            key: operand_place(key)?,
        },
    };
    // Conditions 5-7: the taken branch is nothing but the seeding store.
    if !then_block_only_seeds_default(function, types, &shape) {
        return None;
    }
    // Condition 8: the join immediately mutates the same entry.
    if !join_creates_entry(function, shape.join, shape.entry) {
        return None;
    }
    Some(Elision {
        guard,
        join: *else_block,
    })
}

/// Whether a local is a temporary that is assigned once and read once.
fn single_use_temp(function: &MirFunction, local: LocalId) -> bool {
    local_is_temp(function, local)
        && local_assignment_count(function, local) == 1
        && local_read_count(function, local) == 1
}

/// Whether `then_block` does nothing but store an empty list into `dict[key]`
/// before rejoining at `join`, and is reachable only from `guard`.
fn then_block_only_seeds_default(
    function: &MirFunction,
    types: &TypeInterner,
    shape: &GuardShape<'_>,
) -> bool {
    let Some(block) = block_at(function, shape.then_block) else {
        return false;
    };
    // Condition 5.
    if !matches!(block.terminator, Some(Terminator::Goto(target)) if target == shape.join) {
        return false;
    }
    if predecessors(function, shape.then_block) != vec![shape.guard] {
        return false;
    }
    // Condition 6.
    if !block.phis.is_empty() {
        return false;
    }
    let [
        Statement::Assign {
            dest: default_local,
            value: default_value,
        },
        Statement::AssignPlace {
            place:
                Place::Index {
                    base: store_base,
                    index: store_index,
                },
            value: Rvalue::Use(store_source),
        },
    ] = block.statements.as_slice()
    else {
        return false;
    };
    if *store_base != shape.entry.base {
        return false;
    }
    if !operand_reads_place(store_index, shape.entry.key) {
        return false;
    }
    if operand_local(store_source) != Some(*default_local) {
        return false;
    }
    if !single_use_temp(function, *default_local) {
        return false;
    }
    // Condition 7.
    matches!(default_value, Rvalue::List(items) if items.is_empty())
        && dict_value_is_default_type(function, types, shape.entry.base, *default_local)
}

/// Whether `base` is a dict whose value type is exactly the default local's list type.
///
/// This mirrors the guard `list_push_text` puts on its `entry_or_insert` path,
/// so the default this pass deletes is the same one the backend synthesizes.
fn dict_value_is_default_type(
    function: &MirFunction,
    types: &TypeInterner,
    base: LocalId,
    default_local: LocalId,
) -> bool {
    let Some(base_decl) = local_decl(function, base) else {
        return false;
    };
    let Some(default_decl) = local_decl(function, default_local) else {
        return false;
    };
    if !matches!(types.get(default_decl.ty), Some(Type::List(_))) {
        return false;
    }
    matches!(types.get(base_decl.ty), Some(Type::Dict(_, value)) if *value == default_decl.ty)
}

/// Whether `join` opens with a list push through `dict[key]`, which inserts the
/// key when it is absent.
fn join_creates_entry(function: &MirFunction, join: BlockId, entry: Entry<'_>) -> bool {
    let Some(block) = block_at(function, join) else {
        return false;
    };
    if !block.phis.is_empty() {
        return false;
    }
    let Some(Statement::Assign {
        value: Rvalue::ListPush { list, .. },
        ..
    }) = block.statements.first()
    else {
        return false;
    };
    let Some(Place::Index { base, index }) = operand_place(list) else {
        return false;
    };
    if *base != entry.base {
        return false;
    }
    operand_reads_place(index, entry.key)
}

/// Whether an operand reads exactly `place`, whether by copy or by move.
fn operand_reads_place(operand: &Operand, place: &Place) -> bool {
    operand_place(operand) == Some(place)
}

/// The block with the given id, if the id is in range.
fn block_at(function: &MirFunction, block: BlockId) -> Option<&BasicBlock> {
    function
        .blocks
        .get(usize::try_from(block.0).unwrap_or(usize::MAX))
}

/// The indices of the blocks whose terminator names `target` as a successor.
fn predecessors(function: &MirFunction, target: BlockId) -> Vec<usize> {
    function
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| {
            block
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator.successors().contains(&target))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Redirect the guard straight at the join and drop the two guard statements.
///
/// The `then_block` is deliberately left in place: block ids are positional, so
/// removing it would renumber every later block. It becomes unreachable, which
/// MIR validation tolerates (its dataflow walk only visits reachable blocks) and
/// which the Rust backend never emits.
fn apply_elision(function: &mut MirFunction, elision: &Elision) {
    let Some(block) = function.blocks.get_mut(elision.guard) else {
        return;
    };
    let Some(not_index) = block.statements.len().checked_sub(1) else {
        return;
    };
    let Some(contains_index) = block.statements.len().checked_sub(2) else {
        return;
    };
    block.terminator = Some(Terminator::Goto(elision.join));
    block.statements.remove(not_index);
    block.statements.remove(contains_index);
}
