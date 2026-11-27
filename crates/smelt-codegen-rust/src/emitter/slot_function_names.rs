//! The `Function.prototype.name` a function-typed storage slot provably holds.
//!
//! A function's name is fixed when the function value is created
//! ([`super::function_name`]). A read whose operand is a local follows that
//! local back to its closure definition; a read whose operand is a *slot* —
//! a class field (`k.handler`, `K.handler` on a per-evaluation statics frame)
//! or a module global (a static field `K.handler`, a module `let`) — is
//! answered here, from every write to that slot in the whole program.
//!
//! # The rule
//!
//! A slot's name is known when every value the program stores into it is a
//! closure definition, and all of those closures report the same name. The
//! stores are:
//!
//! * a global's initializer (`MirGlobalInit::Call`: the value its nullary
//!   initializer function returns) and every `GlobalSet` of it;
//! * a field's `AssignPlace` (`this.f = ..` in a constructor, `k.f = ..`
//!   anywhere), the field initializers of a `Struct` literal, and a
//!   `Place::Global` field projection.
//!
//! A stored value counts as a closure definition when it is an
//! `Rvalue::Closure`, or a local the storing body assigns exactly once (an
//! `undefined` placeholder aside) followed through plain copies to one. Any
//! other stored value — a parameter, a call result, a value read from another
//! slot, a constant — makes the slot's name unknown, and the read keeps the
//! anonymous `""` it answered before.
//!
//! Field slots are keyed by the field's symbol alone, not by its class, so a
//! subclass writing an inherited field, or two classes sharing a field name,
//! are judged together: a disagreement only loses the static answer, never
//! picks the wrong name.
//!
//! # Requirements
//!
//! Behavior: a slot answered here holds, at every read, a function created
//! by one of the counted closure definitions, so its JavaScript `name` is
//! that definition's. That holds because every typed store into the slot is
//! one of the statements above; an erased value never reaches a typed slot
//! without a typed store (an erased object view of an instance is a snapshot
//! or a live view whose writes land through the class's own setters, which
//! are MIR bodies scanned here).
//! Type preservation: the answer is a string constant; no value changes type.

use std::collections::HashMap;

use smelt_hir::Symbol;
use smelt_mir::{
    BasicBlock, ClosureId, Constant, GlobalProjection, LocalId, Mir, MirGlobalInit, Operand, Place,
    Rvalue, Statement, Terminator,
};

/// How many `a = b` copies are followed back to a closure definition.
const COPY_CHAIN_LIMIT: usize = 16;

/// A storage slot a function value can be read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum FunctionSlot {
    /// A module global, by index into `Mir::globals`.
    Global(u32),
    /// A class field, by field symbol (every class's field of that name).
    Field(Symbol),
}

/// What the program stores into one slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotContents {
    /// Only closures whose shared name is this (`None`: anonymous); the first
    /// one seen is kept to answer its definition.
    Closures(Option<Symbol>, ClosureId),
    /// Something that is not a closure definition, or closures of different
    /// names.
    Unknown,
}

/// Every function-typed slot's stores, computed once per program.
#[derive(Debug, Default)]
pub(crate) struct SlotFunctions {
    /// Contents per slot; a slot never stored to is absent.
    slots: HashMap<FunctionSlot, SlotContents>,
}

impl SlotFunctions {
    /// Scan every function and closure body of `mir`.
    pub(crate) fn of(mir: &Mir) -> Self {
        let mut slots = Self::default();
        for (position, global) in mir.globals.iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                continue;
            };
            let contents = match &global.init {
                MirGlobalInit::Call(func) => usize::try_from(func.0)
                    .ok()
                    .and_then(|func_index| mir.functions.get(func_index))
                    .map_or(SlotContents::Unknown, |function| {
                        returned_closure(mir, &function.blocks)
                    }),
                MirGlobalInit::Constant(_) => SlotContents::Unknown,
            };
            slots.record(FunctionSlot::Global(index), contents);
        }
        for function in &mir.functions {
            slots.scan_body(mir, &function.blocks);
        }
        for closure in &mir.closures {
            slots.scan_body(mir, &closure.blocks);
        }
        slots
    }

    /// The closure every store into `slot` creates, when they agree on one
    /// name: the first such closure (its name is the slot's name).
    pub(super) fn closure(&self, slot: FunctionSlot) -> Option<ClosureId> {
        match self.slots.get(&slot)? {
            SlotContents::Closures(_, closure) => Some(*closure),
            SlotContents::Unknown => None,
        }
    }

    /// Merge one store's `contents` into `slot`.
    fn record(&mut self, slot: FunctionSlot, contents: SlotContents) {
        let merged = match (self.slots.get(&slot), contents) {
            (None, first_store) => first_store,
            (Some(SlotContents::Closures(seen, first)), SlotContents::Closures(name, _))
                if *seen == name =>
            {
                SlotContents::Closures(name, *first)
            }
            _ => SlotContents::Unknown,
        };
        self.slots.insert(slot, merged);
    }

    /// Record every slot store in one body.
    fn scan_body(&mut self, mir: &Mir, blocks: &[BasicBlock]) {
        let stores = blocks.iter().flat_map(|block| &block.statements);
        for statement in stores {
            let (slot, value) = match statement {
                Statement::AssignPlace {
                    place: Place::Field { field, .. },
                    value,
                } => (FunctionSlot::Field(*field), value),
                Statement::AssignPlace {
                    place:
                        Place::Global {
                            projection: GlobalProjection::Field(field),
                            ..
                        },
                    value,
                } => (FunctionSlot::Field(*field), value),
                Statement::Assign { value, .. }
                | Statement::AssignPlace { value, .. }
                | Statement::DictEntryUpdate { value, .. } => {
                    self.scan_nested_stores(mir, blocks, value);
                    continue;
                }
                Statement::StorageLive(_) => continue,
            };
            let contents = stored_closure(mir, blocks, value);
            self.record(slot, contents);
            self.scan_nested_stores(mir, blocks, value);
        }
    }

    /// Record the stores an rvalue makes besides its destination: a
    /// `GlobalSet` and a `Struct` literal's field initializers.
    fn scan_nested_stores(&mut self, mir: &Mir, blocks: &[BasicBlock], rvalue: &Rvalue) {
        if let Rvalue::GlobalSet { global, value } = rvalue {
            let contents = operand_closure(mir, blocks, value);
            self.record(FunctionSlot::Global(*global), contents);
        }
        if let Rvalue::Struct { fields, .. } = rvalue {
            for (field, initializer) in fields {
                let contents = operand_closure(mir, blocks, initializer);
                self.record(FunctionSlot::Field(*field), contents);
            }
        }
    }
}

/// The closure a nullary initializer returns, when every `return` returns one
/// closure name.
fn returned_closure(mir: &Mir, blocks: &[BasicBlock]) -> SlotContents {
    let mut contents = None;
    for block in blocks {
        if let Some(Terminator::Return(operand)) = &block.terminator {
            let returned = operand_closure(mir, blocks, operand);
            contents = Some(match (contents, returned) {
                (None, first_return) => first_return,
                (Some(SlotContents::Closures(seen, first)), SlotContents::Closures(name, _))
                    if seen == name =>
                {
                    SlotContents::Closures(name, first)
                }
                _ => SlotContents::Unknown,
            });
        }
    }
    contents.unwrap_or(SlotContents::Unknown)
}

/// The closure a stored rvalue creates.
fn stored_closure(mir: &Mir, blocks: &[BasicBlock], value: &Rvalue) -> SlotContents {
    if let Rvalue::Closure { id, .. } = value {
        return closure_contents(mir, *id);
    }
    if let Rvalue::Use(operand) = value {
        return operand_closure(mir, blocks, operand);
    }
    SlotContents::Unknown
}

/// The closure an operand of a body holds, followed through the body's
/// single-assignment locals.
fn operand_closure(mir: &Mir, blocks: &[BasicBlock], operand: &Operand) -> SlotContents {
    let (Operand::Copy(Place::Local(mut local)) | Operand::Move(Place::Local(mut local))) =
        operand.clone()
    else {
        return SlotContents::Unknown;
    };
    for _ in 0..COPY_CHAIN_LIMIT {
        let value = single_assigned_value(blocks, local);
        if let Some(Rvalue::Closure { id, .. }) = value {
            return closure_contents(mir, *id);
        }
        let Some(Rvalue::Use(
            Operand::Copy(Place::Local(source)) | Operand::Move(Place::Local(source)),
        )) = value
        else {
            return SlotContents::Unknown;
        };
        local = *source;
    }
    SlotContents::Unknown
}

/// A closure definition's contents: its name and itself.
fn closure_contents(mir: &Mir, id: ClosureId) -> SlotContents {
    usize::try_from(id.0)
        .ok()
        .and_then(|index| mir.closures.get(index))
        .map_or(SlotContents::Unknown, |closure| {
            SlotContents::Closures(closure.name, id)
        })
}

/// The value of the one statement writing `local` in a body, ignoring an
/// `undefined` placeholder (`let f: F;`), or `None` when it is written by
/// anything else (a second write, a dict-entry binding, a caller or capture).
fn single_assigned_value(blocks: &[BasicBlock], local: LocalId) -> Option<&Rvalue> {
    let written_by_terminator = blocks.iter().any(|block| {
        let Some(Terminator::Call { dest, unwind, .. } | Terminator::Await { dest, unwind, .. }) =
            &block.terminator
        else {
            return false;
        };
        *dest == local
            || unwind
                .as_ref()
                .is_some_and(|handler| handler.exception_local == Some(local))
    });
    if written_by_terminator {
        return None;
    }
    let mut found = None;
    for statement in blocks.iter().flat_map(|block| &block.statements) {
        if let Statement::DictEntryUpdate { current, .. } = statement
            && *current == local
        {
            return None;
        }
        let value = if let Statement::Assign { dest, value } = statement
            && *dest == local
        {
            value
        } else if let Statement::AssignPlace {
            place: Place::Local(dest),
            value,
        } = statement
            && *dest == local
        {
            value
        } else {
            continue;
        };
        if matches!(value, Rvalue::Use(Operand::Const(Constant::Undefined))) {
            continue;
        }
        if found.replace(value).is_some() {
            return None;
        }
    }
    found
}
