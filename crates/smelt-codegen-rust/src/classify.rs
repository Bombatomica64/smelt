//! Reference-class classification.
//!
//! JavaScript objects are reference cells with shared mutable identity, but
//! Smelt emits classes as by-value Rust structs by default. A by-value struct is
//! correct only for *value classes* — objects that are constructed and read but
//! never mutated after construction, aliased-and-observed, or allowed to let
//! `this` escape into a stored closure. Any class that violates those
//! constraints must be emitted as a **reference class**: a thin handle newtype
//! over `Rc<RefCell<Inner>>` whose `Clone` shares identity (see
//! `specs/reference-class-modeling.md`).
//!
//! This module computes, once per crate, the set of class symbols that must be
//! lifted to the handle representation. The decision mirrors the module-globals
//! "lift only if mutated" rule: the heap allocation, refcount, and runtime
//! borrow-check are only paid when shared-mutable identity is actually required.
//!
//! ## Classification rule (V1)
//!
//! A class is a reference class if ANY of the following holds, else it stays a
//! by-value value class:
//! - a non-constructor instance method assigns to `this.<field>` (post-
//!   construction mutation), or
//! - any source path writes an instance field on a class-typed binding after
//!   construction (`obj.<field> = …` or `obj[key] = …`), or
//! - `this` (a method's `self`) is captured by a closure (the escaping-`this`
//!   case that needs a shareable handle).
//!
//! Pure aliasing without any mutation (`const b = a;` where neither `a` nor `b`
//! is ever mutated) is intentionally NOT a trigger: a value class that is only
//! copied and read is byte-for-byte observationally identical under value and
//! reference semantics, so it stays a value class. This is the one narrowing
//! versus the spec's literal "reassigned or aliased and later observed" clause
//! and is sound because the observable difference only appears under mutation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "classification helpers are shared with sibling emitter modules"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type};
use smelt_mir::{HirOrigin, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement};

/// Compute the set of class symbols that must be emitted as reference classes.
///
/// Scans every MIR function for the V1 classification triggers documented on
/// this module. The result is consulted by the emitter to choose between the
/// handle-newtype representation and the current by-value struct.
pub(crate) fn reference_classes(mir: &Mir) -> HashSet<Symbol> {
    let mut references = HashSet::new();
    for function in &mir.functions {
        collect_field_write_triggers(mir, function, &mut references);
        collect_self_capture_triggers(function, &mut references);
    }
    // V1 deviation: a class with a dynamic index-signature store
    // (`[key: string]: T`) keeps its existing by-value struct emission. Its
    // keyed access routes through the synthesized `__smelt_index_store` field
    // by concrete struct access, which is not yet taught to project through the
    // shared cell; lifting such a class would emit `bag.__smelt_index_store`
    // against a handle that has no such field. Aliasing an index-signature class
    // therefore does not share its store yet — a documented V1 limitation, not a
    // regression from prior behavior.
    references.retain(|name| !class_has_index_store(mir, *name));
    references
}

/// Return whether a class carries a synthesized dynamic index-signature store.
fn class_has_index_store(mir: &Mir, name: Symbol) -> bool {
    mir.classes
        .iter()
        .find(|class| class.name == name)
        .is_some_and(|class| {
            class.fields.iter().any(|field| {
                mir.symbols.get(field.name) == Some(smelt_hir::CLASS_INDEX_STORE_FIELD)
            })
        })
}

/// Record classes mutated through a field or index write after construction.
///
/// A write to `base.<field>`/`base[key]` where `base` is class-typed lifts that
/// class, *except* a constructor's writes to the instance it is building (its
/// own class), which are initial construction rather than post-construction
/// mutation. A constructor's `this` is a user binding, not a parameter, so this
/// is keyed on the base local's class matching the class under construction
/// (a constructor mutating a *different* object still lifts that other class).
fn collect_field_write_triggers(
    mir: &Mir,
    function: &MirFunction,
    references: &mut HashSet<Symbol>,
) {
    let constructing_class = match function.origin {
        HirOrigin::ClassConstructor { class, .. } => Some(class),
        _ => None,
    };
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::AssignPlace { place, .. } = statement else {
                continue;
            };
            let base = match place {
                Place::Field { base, .. } | Place::Index { base, .. } => *base,
                Place::Local(_) => continue,
            };
            let Some(name) = class_name_of_local(mir, function, base) else {
                continue;
            };
            // A constructor initializing its own instance is construction, not
            // post-construction mutation.
            if constructing_class == Some(name) {
                continue;
            }
            references.insert(name);
        }
    }
}

/// Record classes whose method captures `this` into a closure.
///
/// A closure that captures the method receiver needs a shareable handle to the
/// same object (the resolver stored in `deferredTasks` is the canonical case),
/// which only the reference representation provides.
fn collect_self_capture_triggers(function: &MirFunction, references: &mut HashSet<Symbol>) {
    let HirOrigin::ClassMethod { class, .. } = function.origin else {
        return;
    };
    let Some(self_local) = function.params.first().copied() else {
        return;
    };
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::Assign {
                value: Rvalue::Closure { captures, .. },
                ..
            } = statement
            else {
                continue;
            };
            if captures
                .iter()
                .any(|capture| operand_base_local(capture) == Some(self_local))
            {
                references.insert(class);
            }
        }
    }
}

/// Return the class symbol of a local's type, if the local is a *nominal class*.
///
/// Only classes declared in `mir.classes` can carry the handle representation;
/// interfaces and other structurally-typed records are also spelled
/// `Type::Class` internally but stay plain value structs, so they are excluded
/// here to avoid routing their field access through a nonexistent cell.
fn class_name_of_local(mir: &Mir, function: &MirFunction, local: LocalId) -> Option<Symbol> {
    let decl = function.locals.get(usize::try_from(local.0).ok()?)?;
    match mir.types.get(decl.ty) {
        Some(Type::Class { name, .. }) if mir.classes.iter().any(|class| class.name == *name) => {
            Some(*name)
        }
        _ => None,
    }
}

/// Return the base local an operand reads from, ignoring field/index projection.
fn operand_base_local(operand: &Operand) -> Option<LocalId> {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Const(_) => return None,
    };
    match place {
        Place::Local(local) => Some(*local),
        Place::Field { base, .. } | Place::Index { base, .. } => Some(*base),
    }
}
