//! Dictionary adoption: which record types adopt the typed dictionaries
//! converted to them.
//!
//! # The defect this closes
//!
//! An object literal whose values share one type, kept as a dictionary
//! because its uses rule out a struct (its keys are enumerated, it is
//! asserted to `unknown`), lowers to `SmeltRecord<JsString, V>`. Passed where
//! a record type is expected, it was converted to that record by COPYING its
//! entries, so the callee wrote a copy:
//!
//! ```text
//! interface Counter { hits: number; misses: number }
//! function bump(c: Counter) { c.hits += 1 }
//! const counts = { hits: 0, misses: 0 };  for (const k in counts) {}
//! bump(counts);  counts.hits   // 1, was 0
//! ```
//!
//! # The rule
//!
//! A record type is an *adopter* when a body converts a string-keyed
//! dictionary of a concrete value type to it, the record being a plain object
//! shape (a non-generic interface or object type, not a class, with no call
//! signature, accessor or index store: [`adoptable`]) and the dictionary
//! outliving the conversion (a dictionary literal read once, the way an
//! annotated object literal is lowered, is not: [`fresh_dictionaries`]). The
//! flows are found the way the IR spells them ([`record_flows`]): an
//! assignment or move between locals, an element of a list, tuple or
//! dictionary literal, a field of a record literal, a write to a field or an
//! element, an argument against the callee's parameter, a `return` against
//! the body's return type, looking through `Optional`, lists, tuples,
//! dictionaries, a union's one record arm and same-named record properties.
//!
//! An adopter is a reference record with views (the classification lifts it,
//! and the runtime demand carries the view runtime), and the conversion makes
//! the dictionary a live typed view of the record it built
//! (`SmeltRecord::smelt_adopt_typed`): the two are one object from then on.
//!
//! # Requirements
//!
//! - **Behavior**: after `r = convert(d)`, a write through `r` is read back
//!   through `d` and the other way round, `d` and `r` are `===`, and `d`'s
//!   key order is the object's.
//! - **Type preservation**: the dictionary keeps its value type and the
//!   record its struct; only the view's one-value converters cross the
//!   erased carrier, inside the runtime.
//! - **Pay-for-use**: a program that converts no lasting typed dictionary to
//!   a record has no adopter and emits as before.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the classification is shared with the emitter, classify and runtime demand"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{BasicBlock, LocalDecl, Mir, Operand, Place, Rvalue, Statement, Terminator};

/// Whether the record named `name` can adopt a dictionary: a non-generic
/// object shape (an interface or a lowered object type, not a class) with no
/// call signature, no accessor-backed property and no dynamic index store,
/// which is what a live record is (`emitter::live_view`).
pub(crate) fn adoptable(mir: &Mir, name: Symbol) -> bool {
    crate::classes::find_class(mir, name).is_none()
        && !crate::classify::class_has_index_store(mir, name)
        && mir.interfaces.iter().any(|interface| {
            interface.name == name
                && interface.type_params.is_empty()
                && !crate::classify::is_callable_shape(mir, interface)
                && !crate::classify::has_accessor_fields(mir, interface)
        })
}

/// What the flow scan finds (see [`record_flows`]).
#[derive(Debug, Default)]
pub(crate) struct RecordFlows {
    /// Every adoptable record a string-keyed dictionary of a concrete value
    /// type is converted to: the record built there ADOPTS the dictionary,
    /// which becomes a live view of it (`SmeltRecord::smelt_adopt_typed`), so
    /// the record must be a reference record with views.
    pub(crate) adopters: HashSet<Symbol>,
}

/// The dictionary-to-record flows of every body (see
/// the module docs and [`RecordFlows`]).
pub(crate) fn record_flows(mir: &Mir) -> RecordFlows {
    let mut pairs = RecordFlows::default();
    let mut scan = |locals: &[LocalDecl], blocks: &[BasicBlock], return_ty: TypeId| {
        let mut flows = Flows {
            mir,
            locals,
            fresh: fresh_dictionaries(blocks),
            pairs: &mut pairs,
        };
        for block in blocks {
            for statement in &block.statements {
                flows.statement(statement);
            }
            match &block.terminator {
                Some(Terminator::Call { callee, args, .. }) => {
                    let params = flows.callee_params(callee);
                    for (operand, param) in args.iter().zip(params) {
                        flows.operand_into(operand, param);
                    }
                }
                Some(Terminator::Return(operand)) => flows.operand_into(operand, return_ty),
                _ => {}
            }
        }
    };
    for function in &mir.functions {
        scan(&function.locals, &function.blocks, function.return_ty);
    }
    for closure in &mir.closures {
        scan(&closure.locals, &closure.blocks, closure.return_ty);
    }
    pairs
}

/// The flow scan of one body.
struct Flows<'a> {
    /// The program.
    mir: &'a Mir,
    /// The body's locals.
    locals: &'a [LocalDecl],
    /// The body's single-use dictionary literals (see [`fresh_dictionaries`]).
    fresh: HashSet<smelt_mir::LocalId>,
    /// Where found flows go.
    pairs: &'a mut RecordFlows,
}

impl Flows<'_> {
    /// The declared type of `local`.
    fn local_ty(&self, local: smelt_mir::LocalId) -> Option<TypeId> {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| self.locals.get(index))
            .map(|decl| decl.ty)
    }

    /// The type of the whole value `operand` reads, when it reads a local.
    fn operand_ty(&self, operand: &Operand) -> Option<TypeId> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Const(_) => None,
        }
    }

    /// The type of the slot `place` names: a local, a record's field, or an
    /// element of a list or dictionary.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "single-shape recogniser: only a list or dictionary has an element a write lands in"
    )]
    fn place_ty(&self, place: &Place) -> Option<TypeId> {
        match place {
            Place::Local(local) => self.local_ty(*local),
            Place::Field { base, field } => {
                let record = strip_optional(self.mir, self.local_ty(*base)?);
                self.field_ty(record, *field)
            }
            Place::Index { base, .. } => {
                match self
                    .mir
                    .types
                    .get(strip_optional(self.mir, self.local_ty(*base)?))?
                {
                    Type::List(element) => Some(*element),
                    Type::Dict(_, value) => Some(*value),
                    _ => None,
                }
            }
            Place::Global { .. } => None,
        }
    }

    /// The declared type of the property `field` of the record type `record`.
    fn field_ty(&self, record: TypeId, field: Symbol) -> Option<TypeId> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(record) else {
            return None;
        };
        record_fields(self.mir, *name)
            .into_iter()
            .find(|candidate| candidate.name == field)
            .map(|found| found.ty)
    }

    /// The parameter types of a call's callee, when they are known.
    fn callee_params(&self, callee: &smelt_mir::Callee) -> Vec<TypeId> {
        match callee {
            smelt_mir::Callee::Static(function) | smelt_mir::Callee::BaseInit(function) => {
                usize::try_from(function.0)
                    .ok()
                    .and_then(|index| self.mir.functions.get(index))
                    .map(|target| {
                        target
                            .params
                            .iter()
                            .filter_map(|param| {
                                usize::try_from(param.0)
                                    .ok()
                                    .and_then(|index| target.locals.get(index))
                                    .map(|decl| decl.ty)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            }
            smelt_mir::Callee::Indirect(operand) => {
                match self
                    .operand_ty(operand)
                    .and_then(|ty| self.mir.types.get(strip_optional(self.mir, ty)))
                {
                    Some(Type::Function(function)) => function.params.clone(),
                    _ => Vec::new(),
                }
            }
            smelt_mir::Callee::Builtin(_) | smelt_mir::Callee::Op(..) => Vec::new(),
        }
    }

    /// Record the pairs of the value `operand` handed to a slot of type `slot`.
    fn operand_into(&mut self, operand: &Operand, slot: TypeId) {
        if let Some(value) = self.operand_ty(operand) {
            // A dictionary literal read only by this conversion is the
            // record's initializer and nothing else: no one can observe it
            // afterwards, so the record need not adopt it.
            let adopt = !matches!(
                operand,
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                    if self.fresh.contains(local)
            );
            pair_types(self.mir, value, slot, adopt, 0, self.pairs);
        }
    }

    /// Record the pairs of one statement's flows.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "single-shape recogniser: only a use, a container literal or a record literal hands an operand to a typed slot"
    )]
    fn statement(&mut self, statement: &Statement) {
        let (slot_ty, value) = match statement {
            Statement::Assign { dest, value } => (self.local_ty(*dest), value),
            Statement::AssignPlace { place, value } => (self.place_ty(place), value),
            Statement::DictEntryUpdate { .. } | Statement::StorageLive(_) => return,
        };
        let Some(slot) = slot_ty else { return };
        let element_of = |mir: &Mir| match mir.types.get(strip_optional(mir, slot)) {
            Some(Type::List(item) | Type::Dict(_, item)) => Some(*item),
            _ => None,
        };
        match value {
            Rvalue::Use(operand) => self.operand_into(operand, slot),
            Rvalue::List(operands) | Rvalue::Set(operands) => {
                if let Some(element) = element_of(self.mir) {
                    for operand in operands {
                        self.operand_into(operand, element);
                    }
                }
            }
            Rvalue::Dict(entries) => {
                if let Some(element) = element_of(self.mir) {
                    for (_, operand) in entries {
                        self.operand_into(operand, element);
                    }
                }
            }
            Rvalue::Tuple(operands) => {
                if let Some(Type::Tuple(items)) = self.mir.types.get(strip_optional(self.mir, slot))
                {
                    let item_tys = items.clone();
                    for (operand, item) in operands.iter().zip(item_tys) {
                        self.operand_into(operand, item);
                    }
                }
            }
            Rvalue::Struct { class, fields } => {
                let declared = record_fields(self.mir, *class);
                for (name, operand) in fields {
                    if let Some(field) = declared.iter().find(|field| field.name == *name) {
                        self.operand_into(operand, field.ty);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The locals of a body that hold a dictionary literal read exactly once:
/// assigned once, from a dictionary rvalue, and read by one statement or
/// terminator. Lowering builds an annotated object literal that way when it
/// goes through a dictionary first (`const p: Pixel = { x: 3, y: -4 }`).
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "single-shape recogniser: only a dictionary literal or a plain copy of a local makes a fresh dictionary"
)]
fn fresh_dictionaries(blocks: &[BasicBlock]) -> HashSet<smelt_mir::LocalId> {
    // Per local: how often it is assigned, and the dictionary literal or the
    // local it is a move or copy of, when its one assignment is either.
    let mut defs: std::collections::HashMap<
        smelt_mir::LocalId,
        (usize, Option<Option<smelt_mir::LocalId>>),
    > = std::collections::HashMap::new();
    let mut reads: std::collections::HashMap<smelt_mir::LocalId, usize> =
        std::collections::HashMap::new();
    for block in blocks {
        for statement in &block.statements {
            if let Statement::Assign { dest, value } = statement {
                let source = match value {
                    Rvalue::Dict(_) => Some(None),
                    Rvalue::Use(
                        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)),
                    ) => Some(Some(*local)),
                    _ => None,
                };
                let entry = defs.entry(*dest).or_insert((0, source));
                entry.0 = entry.0.saturating_add(1);
            }
            statement.read_locals(|local| {
                let count = reads.entry(local).or_insert(0);
                *count = count.saturating_add(1);
            });
        }
        if let Some(terminator) = &block.terminator {
            terminator.reads(|operand| {
                if let Operand::Copy(place) | Operand::Move(place) = operand {
                    let read = match place {
                        Place::Local(local)
                        | Place::Field { base: local, .. }
                        | Place::Index { base: local, .. } => Some(*local),
                        Place::Global { .. } => None,
                    };
                    if let Some(local) = read {
                        let count = reads.entry(local).or_insert(0);
                        *count = count.saturating_add(1);
                    }
                }
            });
        }
    }
    let single = |local: &smelt_mir::LocalId| reads.get(local).copied().unwrap_or(0) == 1;
    // A dictionary literal read once is fresh, and so is a single-use move or
    // copy of a fresh local (lowering passes a literal argument on through a
    // temporary): the chain ends at the one conversion that reads it.
    let mut fresh: HashSet<smelt_mir::LocalId> = defs
        .iter()
        .filter(|(local, (count, source))| *count == 1 && *source == Some(None) && single(local))
        .map(|(local, _)| *local)
        .collect();
    loop {
        let grown = defs
            .iter()
            .filter(|(local, (count, source))| {
                *count == 1
                    && !fresh.contains(*local)
                    && matches!(source, Some(Some(from)) if fresh.contains(from))
                    && single(local)
            })
            .map(|(local, _)| *local)
            .collect::<Vec<_>>();
        if grown.is_empty() {
            return fresh;
        }
        fresh.extend(grown);
    }
}

/// Every property a record type declares, inherited ones included.
fn record_fields(mir: &Mir, name: Symbol) -> Vec<smelt_mir::MirField> {
    if let Some(class) = crate::classes::find_class(mir, name) {
        return crate::classes::effective_class_fields(mir, class);
    }
    mir.interfaces
        .iter()
        .find(|interface| interface.name == name)
        .map(|interface| crate::classes::effective_interface_fields(mir, interface))
        .unwrap_or_default()
}

/// `ty` with every `Optional` around it removed.
fn strip_optional(mir: &Mir, mut ty: TypeId) -> TypeId {
    while let Some(Type::Optional(inner, _)) = mir.types.get(ty) {
        ty = *inner;
    }
    ty
}

/// Record every adopter a value of type `value_ty` converted to type
/// `slot_ty` reaches: a typed dictionary converted to a record, position by
/// position through options, lists, tuples, dictionaries, a union's one
/// record arm and the same-named properties of two records.
fn pair_types(
    mir: &Mir,
    value_ty: TypeId,
    slot_ty: TypeId,
    adopt: bool,
    depth: usize,
    pairs: &mut RecordFlows,
) {
    if depth > 8 || value_ty == slot_ty {
        return;
    }
    let (value, slot) = (strip_optional(mir, value_ty), strip_optional(mir, slot_ty));
    let deeper = depth.saturating_add(1);
    match (mir.types.get(value), mir.types.get(slot)) {
        // A record property both declare is converted with the record.
        (Some(Type::Class { name: from, .. }), Some(Type::Class { name: to, .. })) => {
            if from == to {
                return;
            }
            let target = record_fields(mir, *to);
            for field in record_fields(mir, *from) {
                if let Some(other) = target.iter().find(|other| other.name == field.name) {
                    pair_types(mir, field.ty, other.ty, adopt, deeper, pairs);
                }
            }
        }
        (Some(Type::Dict(key, item)), Some(Type::Class { name, .. }))
            if adopt
                && matches!(mir.types.get(*key), Some(Type::String))
                && !matches!(mir.types.get(*item), Some(Type::Unknown))
                && adoptable(mir, *name) =>
        {
            pairs.adopters.insert(*name);
        }
        (Some(Type::Dict(_, from)), Some(Type::Dict(_, to))) => {
            pair_types(mir, *from, *to, adopt, deeper, pairs);
        }
        (Some(Type::Tuple(from)), Some(Type::Tuple(to))) => {
            for (from_item, to_item) in from.clone().into_iter().zip(to.clone()) {
                pair_types(mir, from_item, to_item, adopt, deeper, pairs);
            }
        }
        // A union slot holds the value in its own arm when it has one; it is
        // converted only to the one record arm there is otherwise.
        (Some(Type::Class { .. }), Some(Type::Union(arms))) => {
            let stripped = arms
                .iter()
                .map(|arm| strip_optional(mir, *arm))
                .collect::<Vec<_>>();
            if stripped.contains(&value) {
                return;
            }
            let records = stripped
                .iter()
                .filter(|arm| matches!(mir.types.get(**arm), Some(Type::Class { .. })))
                .collect::<Vec<_>>();
            if let [arm] = records.as_slice() {
                pair_types(mir, value, **arm, adopt, deeper, pairs);
            }
        }
        _ => {}
    }
}
