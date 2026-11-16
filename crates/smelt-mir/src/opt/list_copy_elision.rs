//! List copy elision: drop list copies and the temporaries around them that no
//! program can observe.
//!
//! JavaScript's `Array.from(list)` and `[...list]` build a NEW array, so the
//! frontend lowers them to [`Rvalue::ListCopy`], whose result never shares the
//! source's buffer. A hand-written port copies too, but it does not copy a
//! vector it just built, it does not bind a narrowed value to a second name only
//! to copy it, it does not convert a copy it just made, and it does not copy a
//! vector it is about to slice. This pass removes exactly those four shapes,
//! each by a local, structural argument:
//!
//! * **Fresh source.** `s = <fresh list>; .. c = list_copy s` where `s` is a
//!   temporary defined once and read once by the copy, and its definition mints
//!   a buffer no one else holds ([`rvalue_mints_fresh_list`]). Copying a buffer
//!   with exactly one name, which is never read again, is unobservable: the copy
//!   becomes `c = move s`, which the temporary passes then merge.
//! * **Coercion source.** `s = copy x; c = list_copy s` in adjacent statements,
//!   where `s` is a temporary defined once and read once, and the first
//!   statement is the coercion of an optional list (`x: T[] | undefined`,
//!   narrowed) or of an erased value (`x: unknown`, or a type parameter `x: T`
//!   asserted `as T[]`) to the list type. The copy reads `x` itself:
//!   `list_copy x`. Codegen's list copy performs the same coercion on a non-list
//!   receiver before copying (see `list_copy_expr`). Nothing runs between the
//!   two statements, so `x` holds the same value.
//! * **Coercion destination.** `c = list_copy s; d = copy c` in adjacent
//!   statements, where `c` is a temporary defined once and read once and `d` is
//!   a list of another element type (a `T[]` copy joined into a `unknown[]`
//!   binding). The copy is made at `d`'s type: `d = list_copy s`, which codegen
//!   renders as the conversion followed by the copy. Converting a fresh copy
//!   and copying a converted value both give a new array holding the converted
//!   elements.
//! * **Element-only consumer.** `c = list_copy s; r = op(.., c, ..)` in adjacent
//!   statements, where `c` is a temporary defined once and read once, by `op`,
//!   and `op` only reads that list's elements: the op table gives the operand
//!   the `Read` role or a non-mutating `Receiver` role
//!   ([`reads_list_elements_only`]), so the op keeps no handle on the buffer and
//!   does not write it, and `op` takes no function-typed operand, so it runs no
//!   source code that could write `s` while it reads. `op` then reads `s`
//!   directly and the copy goes.
//!
//! # Requirements
//!
//! Behaviour: every element read sees the elements the copy would have held,
//! and no later write to either list is seen through the other. The copy is
//! dropped only where no such write can happen while both names are live: the
//! fresh source has no other name, the coercions forward across no statement,
//! and the element-only consumer is the copy's only reader and runs no code.
//!
//! Type preservation: every surviving local keeps its type; only a temporary
//! disappears, and the one conversion a rewrite moves (an optional unwrap, an
//! erased read, a list element conversion) is still performed, by the copy.

use std::collections::HashSet;

use smelt_hir::{Type, TypeId, TypeInterner};

use super::Pass;
use crate::list_escape::{reads_list_elements_only, rvalue_mints_fresh_list};
use crate::{BasicBlock, Body, LocalDecl, LocalId, Mir, Operand, Place, Rvalue, Statement};

/// The list copy elision pass; see the module docs.
#[derive(Debug, Default)]
pub struct ListCopyElision;

impl Pass for ListCopyElision {
    /// Applies the pass to every function and closure body of `mir`, returning
    /// true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= elide_body(&function.locals, &mut function.blocks, &mir.types);
        }
        for closure in &mut mir.closures {
            changed |= elide_body(&closure.locals, &mut closure.blocks, &mir.types);
        }
        changed
    }
}

/// One proven rewrite inside a block.
#[derive(Debug)]
enum Rewrite {
    /// The copy at this index reads a fresh single-use temporary: make it a move.
    FreshSource {
        /// Index of `c = list_copy s`.
        copy_at: usize,
        /// The temporary `s`.
        source: LocalId,
    },
    /// The copy at `copy_at` reads the coercion one statement before it.
    CoercionSource {
        /// Index of `c = list_copy s`; the coercion `s = copy x` is just before.
        copy_at: usize,
        /// The coerced operand `x`.
        coerced: Operand,
    },
    /// The copy at `copy_at` is only converted, by the statement just after it,
    /// into `dest`: copy straight into `dest`.
    CoercionDestination {
        /// Index of `c = list_copy s`; the conversion `d = copy c` is just after.
        copy_at: usize,
        /// The conversion's destination `d`.
        dest: LocalId,
    },
    /// The copy at `copy_at` is read only by the element-only op just after it.
    ElementConsumer {
        /// Index of `c = list_copy s`; the consumer is just after.
        copy_at: usize,
        /// The copy's destination `c`.
        copy: LocalId,
        /// The copied operand `s`.
        source: Operand,
    },
}

/// Applies every provable rewrite in one body; true if modified.
fn elide_body(locals: &[LocalDecl], blocks: &mut [BasicBlock], types: &TypeInterner) -> bool {
    // Counted once: each rewrite removes one single-use temporary together with
    // its only read and moves no other local's reads in or out of the body, so
    // the facts of every other local stay right.
    let single_use = Body { locals, blocks }.single_use_temps();
    let fresh = fresh_single_use_temps(blocks, &single_use);
    let facts = BodyFacts {
        locals,
        types,
        single_use: &single_use,
        fresh: &fresh,
    };
    let mut changed = false;
    for block in blocks.iter_mut() {
        let mut at = 0;
        while at < block.statements.len() {
            match find_rewrite(&facts, block, at) {
                Some(rewrite) => {
                    apply(block, rewrite);
                    changed = true;
                    // A removal may have made the previous statement the
                    // neighbour of a new copy; look again from just before.
                    at = at.saturating_sub(1);
                }
                None => at = at.saturating_add(1),
            }
        }
    }
    changed
}

/// What the rules read about one body, beyond the block being rewritten.
struct BodyFacts<'a> {
    /// The body's local declarations.
    locals: &'a [LocalDecl],
    /// The crate's types.
    types: &'a TypeInterner,
    /// Temporaries assigned once and read once.
    single_use: &'a HashSet<LocalId>,
    /// Single-use temporaries whose definition mints a fresh list.
    fresh: &'a HashSet<LocalId>,
}

/// Single-use temporaries whose one definition mints a fresh list buffer.
fn fresh_single_use_temps(
    blocks: &[BasicBlock],
    single_use: &HashSet<LocalId>,
) -> HashSet<LocalId> {
    blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match statement {
            Statement::Assign { dest, value }
                if single_use.contains(dest) && rvalue_mints_fresh_list(value) =>
            {
                Some(*dest)
            }
            Statement::Assign { .. }
            | Statement::AssignPlace { .. }
            | Statement::DictEntryUpdate { .. }
            | Statement::StorageLive(_) => None,
        })
        .collect()
}

/// Whether `ty` is a list, or an optional list, the two typed receivers
/// codegen's list copy converts into another list type.
fn is_list_or_optional_list(types: &TypeInterner, ty: TypeId) -> bool {
    match types.get(ty) {
        Some(Type::List(_)) => true,
        // Every optional layer (`T[] | null | undefined` is two).
        Some(Type::Optional(..)) => matches!(
            types.get(smelt_hir::type_normalize::peel_optional(types, ty).0),
            Some(Type::List(_))
        ),
        _ => false,
    }
}

/// The type of `local`, if it is declared.
fn local_ty(locals: &[LocalDecl], local: LocalId) -> Option<TypeId> {
    locals
        .get(usize::try_from(local.0).ok()?)
        .map(|decl| decl.ty)
}

/// The rewrite the statement at `at` admits, if it is a list copy one applies to.
fn find_rewrite(facts: &BodyFacts<'_>, block: &BasicBlock, at: usize) -> Option<Rewrite> {
    let BodyFacts {
        locals,
        types,
        single_use,
        fresh,
    } = *facts;
    let Some(Statement::Assign {
        dest: copy,
        value: Rvalue::ListCopy { list },
    }) = block.statements.get(at)
    else {
        return None;
    };
    let copy_ty = local_ty(locals, *copy)?;
    let source_local = list.local();
    let source_ty = source_local.and_then(|local| local_ty(locals, local));
    let same_list_ty = source_ty == Some(copy_ty);
    if let Some(source) = source_local
        && same_list_ty
        && fresh.contains(&source)
    {
        return Some(Rewrite::FreshSource {
            copy_at: at,
            source,
        });
    }
    if let Some(source) = source_local
        && single_use.contains(&source)
        && let Some(coerced) = coercion_before(facts, block, at, source, copy_ty)
    {
        return Some(Rewrite::CoercionSource {
            copy_at: at,
            coerced,
        });
    }
    if single_use.contains(copy)
        && source_ty.is_some_and(|ty| is_list_or_optional_list(types, ty))
        && let Some(dest) = coercion_after(locals, block, at, *copy, types)
    {
        return Some(Rewrite::CoercionDestination { copy_at: at, dest });
    }
    if same_list_ty
        && single_use.contains(copy)
        && let Some(consumer) = block.statements.get(at.saturating_add(1))
        && element_only_consumer(locals, consumer, *copy, types)
    {
        return Some(Rewrite::ElementConsumer {
            copy_at: at,
            copy: *copy,
            source: list.clone(),
        });
    }
    None
}

/// The operand `x` when the statement just before `at` is `source = copy x`,
/// a coercion of an optional `copy_ty` or of an erased value to `copy_ty`.
fn coercion_before(
    facts: &BodyFacts<'_>,
    block: &BasicBlock,
    at: usize,
    source: LocalId,
    copy_ty: TypeId,
) -> Option<Operand> {
    let BodyFacts { locals, types, .. } = *facts;
    let previous = block.statements.get(at.checked_sub(1)?)?;
    let Statement::Assign {
        dest,
        value:
            Rvalue::Use(coerced @ (Operand::Copy(Place::Local(_)) | Operand::Move(Place::Local(_)))),
    } = previous
    else {
        return None;
    };
    if *dest != source || local_ty(locals, source) != Some(copy_ty) {
        return None;
    }
    let coerced_ty = local_ty(locals, coerced.local()?)?;
    let coerces = match types.get(coerced_ty) {
        Some(Type::Optional(..)) => {
            smelt_hir::type_normalize::peel_optional(types, coerced_ty).0 == copy_ty
        }
        Some(Type::Unknown | Type::TypeParam { .. }) => true,
        _ => false,
    };
    coerces.then(|| coerced.clone())
}

/// The destination `d` when the statement just after `at` is `d = copy|move
/// copy`, a conversion of the copy into a list of another element type.
fn coercion_after(
    locals: &[LocalDecl],
    block: &BasicBlock,
    at: usize,
    copy: LocalId,
    types: &TypeInterner,
) -> Option<LocalId> {
    let Statement::Assign {
        dest,
        value:
            Rvalue::Use(operand @ (Operand::Copy(Place::Local(_)) | Operand::Move(Place::Local(_)))),
    } = block.statements.get(at.checked_add(1)?)?
    else {
        return None;
    };
    if operand.local() != Some(copy) {
        return None;
    }
    let copy_ty = local_ty(locals, copy)?;
    let dest_ty = local_ty(locals, *dest)?;
    let is_list = |ty: TypeId| matches!(types.get(ty), Some(Type::List(_)));
    (dest_ty != copy_ty && is_list(dest_ty) && is_list(copy_ty)).then_some(*dest)
}

/// Whether `consumer` reads `copy` once, as a list it only reads the elements
/// of, and runs no source code.
fn element_only_consumer(
    locals: &[LocalDecl],
    consumer: &Statement,
    copy: LocalId,
    types: &TypeInterner,
) -> bool {
    let Statement::Assign { value, .. } = consumer else {
        return false;
    };
    let mut found = None;
    let mut index = 0usize;
    let mut runs_code = false;
    value.reads(|operand| {
        if operand.local() == Some(copy) {
            found = Some(index);
        }
        let callable = operand
            .local()
            .and_then(|local| local_ty(locals, local))
            .is_some_and(|ty| matches!(types.get(ty), Some(Type::Function(_))));
        runs_code |= callable;
        index = index.saturating_add(1);
    });
    let Some(position) = found else {
        return false;
    };
    !runs_code && !value.is_fallible() && reads_list_elements_only(value, position)
}

/// Applies `rewrite` to `block`.
fn apply(block: &mut BasicBlock, rewrite: Rewrite) {
    match rewrite {
        Rewrite::FreshSource { copy_at, source } => {
            if let Some(Statement::Assign { value, .. }) = block.statements.get_mut(copy_at) {
                *value = Rvalue::Use(Operand::Move(Place::Local(source)));
            }
        }
        Rewrite::CoercionSource { copy_at, coerced } => {
            if let Some(Statement::Assign {
                value: Rvalue::ListCopy { list },
                ..
            }) = block.statements.get_mut(copy_at)
            {
                *list = coerced;
            }
            if let Some(coercion_at) = copy_at.checked_sub(1) {
                block.statements.remove(coercion_at);
            }
        }
        Rewrite::CoercionDestination { copy_at, dest } => {
            if let Some(Statement::Assign {
                dest: copy_dest, ..
            }) = block.statements.get_mut(copy_at)
            {
                *copy_dest = dest;
            }
            block.statements.remove(copy_at.saturating_add(1));
        }
        Rewrite::ElementConsumer {
            copy_at,
            copy,
            source,
        } => {
            if let Some(consumer) = block.statements.get_mut(copy_at.saturating_add(1))
                && let Statement::Assign { value, .. } = consumer
            {
                value.reads_mut(|operand| {
                    if operand.local() == Some(copy) {
                        *operand = source.clone();
                    }
                });
            }
            block.statements.remove(copy_at);
        }
    }
}
