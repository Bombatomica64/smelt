//! In-place mutation of a dictionary entry.
//!
//! Lowering a source statement like `groups[key].push(item)` produces a
//! read-modify-write-back triple, because HIR has no notion of "borrow the
//! value stored at this place":
//!
//! ```text
//! %tmp = copy %dict[copy %key]     // copy the whole entry OUT of the dict
//! %len = list_push copy %tmp, ..   // mutate the copy
//! %dict[copy %key] = move %tmp     // copy the whole entry BACK IN
//! ```
//!
//! Both edges of that triple copy the entire value. For a list-valued dict that
//! is a full `Vec` clone per mutation, so growing `n` groups costs O(n^2)
//! element copies — the shape every `groupBy`-style function has.
//!
//! A team hand-writing the same code in Rust would not copy at all; they would
//! write `groups.entry(key).or_default().push(item)`, mutating the stored value
//! through a borrow. This pass performs exactly that rewrite: it retargets the
//! mutation at the *place* and deletes both copies.
//!
//! ```text
//! %len = list_push copy %dict[copy %key], ..
//! ```
//!
//! A `Rvalue::ListPush` whose receiver is a [`Place::Index`] therefore means
//! "append to the list stored at this place, in place"; the Rust backend renders
//! it through the container's entry accessor.
//!
//! # Correctness
//!
//! The rewrite only fires on the exact copy-out/mutate/copy-back triple the
//! lowering emits, under guards that make the two deletions provably dead:
//!
//! * The three statements are **consecutive in one basic block**. Nothing can
//!   therefore run between the copy-out and the mutation, so evaluating the
//!   index at the mutation site instead of at the copy-out site cannot observe a
//!   different container or key.
//! * The copy-back writes **the same place** (same base local, structurally
//!   equal index operand) that the copy-out read.
//! * The aliased local is a **compiler temporary** assigned exactly once and
//!   read exactly twice — once as the mutation receiver, once by the copy-back.
//!   Any other reader would observe the copy that no longer exists, so those
//!   shapes keep the copying form; a closure capture counts as such a read
//!   because [`Rvalue::Closure`] carries its captures as operands. A user
//!   binding (`const group = groups[key]; group.push(item)`) is deliberately
//!   left alone.
//! * The pushed item does not read the aliased local, so dropping the local
//!   cannot change what is appended.
//! * The base local is typed `Dict<_, V>` where `V` is exactly the aliased
//!   local's list type. This keeps the new receiver shape confined to the one
//!   container the backend can mutate through, rather than handing it an erased
//!   or otherwise unsupported base.

use smelt_hir::{Type, TypeInterner};

use crate::{LocalId, Mir, MirFunction, Place, Rvalue, Statement};

use super::Pass;
use super::local_use::{
    local_assignment_count, local_decl, local_is_temp, local_read_count, operand_local,
    operand_place, operand_reads_local,
};

/// Rewrites a dict-entry copy-out/mutate/copy-back triple into one in-place mutation.
#[derive(Debug, Default)]
pub struct DictEntryInPlaceMutation;

impl Pass for DictEntryInPlaceMutation {
    fn name(&self) -> &'static str {
        "dict-entry-in-place-mutation"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= run_function(function, &mir.types);
        }
        changed
    }
}

/// A triple found in one basic block, identified by its first statement.
struct Fusion {
    /// Index of the block holding the triple.
    block: usize,
    /// Index of the copy-out statement inside that block.
    start: usize,
}

/// Fuse every eligible triple in one function. Returns true if it was modified.
fn run_function(function: &mut MirFunction, types: &TypeInterner) -> bool {
    let fusions = collect_fusions(function, types);
    if fusions.is_empty() {
        return false;
    }
    for fusion in fusions.iter().rev() {
        apply_fusion(function, fusion);
    }
    true
}

/// Find the triples that may be fused, in program order.
///
/// Overlapping candidates cannot occur: a triple's middle statement is a
/// mutation and its outer statements are the copies of one single-assignment
/// temporary, so no statement can belong to two triples.
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

/// Whether three consecutive statements are a fusable dict-entry mutation triple.
fn triple_is_fusable(
    function: &MirFunction,
    types: &TypeInterner,
    statements: &[Statement],
) -> bool {
    let [
        Statement::Assign {
            dest: alias,
            value: Rvalue::Use(read_source),
        },
        Statement::Assign {
            value: Rvalue::ListPush { list, item },
            ..
        },
        Statement::AssignPlace {
            place:
                Place::Index {
                    base: write_base,
                    index: write_index,
                },
            value: Rvalue::Use(write_source),
        },
    ] = statements
    else {
        return false;
    };
    let Some(Place::Index {
        base: read_base,
        index: read_index,
    }) = operand_place(read_source)
    else {
        return false;
    };
    if read_base != write_base || read_index != write_index {
        return false;
    }
    if operand_local(list) != Some(*alias) || operand_local(write_source) != Some(*alias) {
        return false;
    }
    if !local_is_temp(function, *alias) {
        return false;
    }
    if operand_reads_local(item, *alias) {
        return false;
    }
    // The index operand is re-evaluated at the mutation site instead of the
    // copy-out site. It must not read the alias, which the copy-out has only
    // just defined.
    if operand_reads_local(read_index, *alias) {
        return false;
    }
    if local_assignment_count(function, *alias) != 1 || local_read_count(function, *alias) != 2 {
        return false;
    }
    dict_entry_types_match(function, types, *read_base, *alias)
}

/// Whether `base` is a dict whose value type is exactly the alias's list type.
fn dict_entry_types_match(
    function: &MirFunction,
    types: &TypeInterner,
    base: LocalId,
    alias: LocalId,
) -> bool {
    let Some(base_decl) = local_decl(function, base) else {
        return false;
    };
    let Some(alias_decl) = local_decl(function, alias) else {
        return false;
    };
    if !matches!(types.get(alias_decl.ty), Some(Type::List(_))) {
        return false;
    }
    matches!(types.get(base_decl.ty), Some(Type::Dict(_, value)) if *value == alias_decl.ty)
}

/// Delete the copy-out and copy-back, retargeting the mutation at the place.
fn apply_fusion(function: &mut MirFunction, fusion: &Fusion) {
    let Some(block) = function.blocks.get_mut(fusion.block) else {
        return;
    };
    let push_index = fusion.start.saturating_add(1);
    let write_back_index = fusion.start.saturating_add(2);
    let Some(Statement::Assign {
        value: Rvalue::Use(source),
        ..
    }) = block.statements.get(fusion.start)
    else {
        return;
    };
    let receiver = source.clone();
    let Some(Statement::Assign {
        value: Rvalue::ListPush { list, .. },
        ..
    }) = block.statements.get_mut(push_index)
    else {
        return;
    };
    *list = receiver;
    block.statements.remove(write_back_index);
    block.statements.remove(fusion.start);
}
