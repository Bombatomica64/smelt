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

/// Return the collection operand an rvalue mutates in place, if any.
///
/// Several MIR rvalues model JavaScript collection methods that mutate their
/// receiver in place (`map.set(...)`, `set.add(...)`, `arr.push(...)`, ...).
/// The generated Rust for these calls a `&mut`-taking method on the receiver,
/// so when the receiver is a class instance field (`this.<field>`) the owning
/// method must obtain mutable access to that field. Returning the mutated
/// receiver operand lets the reference-class classifier treat such a call the
/// same as a direct `this.<field> = …` write. Non-mutating projections
/// (`DictCopy`, `SetCopy`, a non-mutating `ListSplice`) return `None`.
fn mutated_collection_operand(rvalue: &Rvalue) -> Option<&Operand> {
    match rvalue {
        Rvalue::DictSet { dict, .. }
        | Rvalue::DictRemoveKey { dict, .. }
        | Rvalue::DictSetDefault { dict, .. }
        | Rvalue::DictClear { dict }
        | Rvalue::DictPop { dict, .. }
        | Rvalue::DictUpdate { dict, .. } => Some(dict),
        Rvalue::DictAssign { target, .. } => Some(target),
        Rvalue::SetAdd { set, .. }
        | Rvalue::SetRemove { set, .. }
        | Rvalue::SetClear { set } => Some(set),
        Rvalue::ListPush { list, .. }
        | Rvalue::ListExtend { list, .. }
        | Rvalue::ListInsert { list, .. }
        | Rvalue::ListUnshift { list, .. }
        | Rvalue::ListPop { list }
        | Rvalue::ListShift { list }
        | Rvalue::ListFill { list, .. } => Some(list),
        Rvalue::ListSplice {
            list, mutate: true, ..
        } => Some(list),
        _ => None,
    }
}

/// Compute the set of class symbols that must be emitted as reference classes.
///
/// Scans every MIR function for the V1 classification triggers documented on
/// this module. The result is consulted by the emitter to choose between the
/// handle-newtype representation and the current by-value struct.
pub(crate) fn reference_classes(mir: &Mir) -> HashSet<Symbol> {
    let mut references = HashSet::new();
    for function in &mir.functions {
        collect_field_write_triggers(mir, function, &mut references);
        collect_field_mutation_triggers(mir, function, &mut references);
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

/// Record classes mutated through an in-place collection method on a field.
///
/// A call such as `this.data.set(k, v)` or `this.items.push(x)` lowers to a
/// `&mut`-taking collection method on `this.<field>`. If the owning class is
/// left as a by-value struct its methods take `&self`, so the generated field
/// mutation fails to borrow-check (`E0596`). Detecting the mutation here lifts
/// the class to the reference (handle) representation, whose interior
/// mutability lets `&self` methods mutate the shared field — matching the
/// direct-field-write trigger in [`collect_field_write_triggers`]. The same
/// constructor exclusion applies: a constructor populating the instance it is
/// building is construction, not post-construction mutation.
fn collect_field_mutation_triggers(
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
            let Statement::Assign { value, .. } = statement else {
                continue;
            };
            let Some(mutated) = mutated_collection_operand(value) else {
                continue;
            };
            // Only an in-place mutation of an instance *field* matters; a
            // mutation of a plain local collection borrows that local mutably
            // and needs no shared cell.
            let base = match operand_field_base(mutated) {
                Some(base) => base,
                None => continue,
            };
            let Some(name) = class_name_of_local(mir, function, base) else {
                continue;
            };
            if constructing_class == Some(name) {
                continue;
            }
            references.insert(name);
        }
    }
}

/// Return the base local of an operand that reads through a field/index
/// projection (`this.<field>`), or `None` for a bare local or constant.
fn operand_field_base(operand: &Operand) -> Option<LocalId> {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Const(_) => return None,
    };
    match place {
        Place::Field { base, .. } | Place::Index { base, .. } => Some(*base),
        Place::Local(_) => None,
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
