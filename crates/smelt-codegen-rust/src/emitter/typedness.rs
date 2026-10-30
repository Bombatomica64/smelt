//! Positive typedness metric: how much of a program reaches Rust typed.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631),
//! `test/aot/typedness/typedness.ts` and `run-typedness.ts`: one line per
//! program saying how many of its places and reads have a type that says
//! something, recorded so that a program may never become less typed.
//!
//! # What is counted
//!
//! Measured on optimized MIR, per source file, for three kinds:
//!
//! - **values** — every local of every function and closure (parameters,
//!   user bindings and temporaries);
//! - **reads** — every assignment whose value is a property or element read:
//!   a `Use` of a field/index place, `OptionalField`, `OptionalIndex`,
//!   `ListIndex`, `TupleIndex` and `DictGet`, classified by the type of the
//!   local the read lands in;
//! - **calls** — every `Call` terminator and every call of a closure value
//!   lowered as a statement (`ClosureCall`, `ClosureCallSpread`), classified
//!   by its result type and, for a call through a value, by the type of the
//!   callee value (the worse of the two wins). Both spellings count because
//!   lowering picks between them by context alone: inside a `try` a closure
//!   call becomes a terminator so a throw reaches the handler, and a call
//!   must not appear or vanish from the metric when it moves.
//!
//! Each is **concrete**, **generic** (it mentions a type parameter that the
//! Rust output carries as a real generic) or **erased** (its Rust type
//! mentions `SmeltUnknown` or `SmeltErasedFunction` anywhere).
//!
//! # Same predicates as codegen
//!
//! The classification renders each type through [`FunctionEmitter::type_text`]
//! with the function's real generic decision applied
//! ([`FunctionEmitter::apply_crate_generic_decision`], the one `emit` uses), so
//! union concreteness, erased type parameters, unresolved classes and `never`
//! are decided by exactly the code that decides them for the emitted crate. A
//! closure's locals are rendered in its owning function's emitter, because the
//! closure body is emitted inside that function and sees its type parameters.
//!
//! "Erased anywhere" is deliberately conservative: a `SmeltList<SmeltUnknown>`
//! counts as erased, so the metric can only under-state typedness.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use smelt_hir::{Type, TypeId};
use smelt_mir::{
    BasicBlock, Callee, ClosureId, HirOrigin, LocalDecl, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};

use super::{EmitContext, FunctionEmitter};
use crate::EmitError;

/// How typed one value, read or call is in the generated Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Typedness {
    /// A concrete Rust type with no erased part.
    Concrete,
    /// Carries a type parameter as a real Rust generic, with no erased part.
    Generic,
    /// The Rust type mentions the erased runtime value somewhere.
    Erased,
}

/// Counts of one kind, by typedness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KindCounts {
    /// Concrete entries.
    pub concrete: usize,
    /// Generic entries.
    pub generic: usize,
    /// Erased entries.
    pub erased: usize,
}

impl KindCounts {
    /// Counts with the given concrete, generic and erased entries.
    #[must_use]
    pub const fn new(concrete: usize, generic: usize, erased: usize) -> Self {
        Self {
            concrete,
            generic,
            erased,
        }
    }

    /// Count one entry.
    pub fn add(&mut self, typedness: Typedness) {
        let slot = match typedness {
            Typedness::Concrete => &mut self.concrete,
            Typedness::Generic => &mut self.generic,
            Typedness::Erased => &mut self.erased,
        };
        *slot = slot.saturating_add(1);
    }

    /// Add another set of counts.
    pub fn merge(&mut self, other: Self) {
        self.concrete = self.concrete.saturating_add(other.concrete);
        self.generic = self.generic.saturating_add(other.generic);
        self.erased = self.erased.saturating_add(other.erased);
    }

    /// Concrete plus generic.
    #[must_use]
    pub const fn typed(self) -> usize {
        self.concrete.saturating_add(self.generic)
    }

    /// Every entry.
    #[must_use]
    pub const fn total(self) -> usize {
        self.typed().saturating_add(self.erased)
    }
}

/// Counts of every kind for one file (or a whole corpus).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TypednessCounts {
    /// Locals of functions and closures.
    pub values: KindCounts,
    /// Property and element reads.
    pub reads: KindCounts,
    /// Call terminators.
    pub calls: KindCounts,
}

impl TypednessCounts {
    /// Add another file's counts.
    pub fn merge(&mut self, other: Self) {
        self.values.merge(other.values);
        self.reads.merge(other.reads);
        self.calls.merge(other.calls);
    }
}

/// Typedness of a lowered crate: per source file, and the total.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TypednessMeasurement {
    /// Counts per source path (as MIR records it), sorted.
    pub files: BTreeMap<String, TypednessCounts>,
}

impl TypednessMeasurement {
    /// The sum over every file.
    #[must_use]
    pub fn total(&self) -> TypednessCounts {
        let mut total = TypednessCounts::default();
        for counts in self.files.values() {
            total.merge(*counts);
        }
        total
    }
}

/// Measure the typedness of optimized MIR; see the module docs.
pub(crate) fn measure(mir: &Mir) -> Result<TypednessMeasurement, EmitError> {
    let context = EmitContext::new(mir)?;
    context.populate_generic_functions(mir)?;
    let owners = closure_owners(mir);
    let mut measurement = TypednessMeasurement::default();
    for (index, function) in mir.functions.iter().enumerate() {
        let emitter = FunctionEmitter::new(mir, &context, function)?;
        emitter.apply_crate_generic_decision();
        let file = function_file(mir, &function.blocks, function.entry);
        let counts = measurement.files.entry(file).or_default();
        measure_body(&emitter, &function.locals, &function.blocks, counts);
        for closure in mir
            .closures
            .iter()
            .filter(|closure| owners.get(&closure.id) == Some(&index))
        {
            measure_body(&emitter, &closure.locals, &closure.blocks, counts);
        }
    }
    // Closures whose owner could not be found (none in well-formed MIR) are
    // measured in a context with no type parameters in scope.
    if let Some(first) = mir.functions.first() {
        let emitter = FunctionEmitter::new(mir, &context, first)?;
        *emitter.suppress_type_params.borrow_mut() = true;
        for closure in mir
            .closures
            .iter()
            .filter(|closure| !owners.contains_key(&closure.id))
        {
            let file = function_file(mir, &closure.blocks, closure.entry);
            let counts = measurement.files.entry(file).or_default();
            measure_body(&emitter, &closure.locals, &closure.blocks, counts);
        }
    }
    Ok(measurement)
}

/// Count one body's values, reads and calls into `counts`.
fn measure_body(
    emitter: &FunctionEmitter<'_>,
    locals: &[LocalDecl],
    blocks: &[BasicBlock],
    counts: &mut TypednessCounts,
) {
    let local_ty = |local: smelt_mir::LocalId| {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| locals.get(index))
            .map(|decl| decl.ty)
    };
    for decl in locals {
        counts.values.add(emitter.typedness_of(decl.ty));
    }
    for block in blocks {
        let mut read = |operand: &Operand| {
            if let Some(typedness) = projected_read(emitter, operand, &local_ty) {
                counts.reads.add(typedness);
            }
        };
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                    value.for_each_operand(&mut read);
                }
                Statement::DictEntryUpdate {
                    index,
                    default,
                    value,
                    ..
                } => {
                    read(index);
                    read(default);
                    value.for_each_operand(&mut read);
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        match &block.terminator {
            Some(Terminator::Call { callee, args, .. }) => {
                if let Callee::Indirect(operand) = callee {
                    read(operand);
                }
                args.iter().for_each(&mut read);
            }
            Some(Terminator::Await { future: operand, .. })
            | Some(Terminator::Switch { cond: operand, .. })
            | Some(Terminator::Match {
                scrutinee: operand, ..
            })
            | Some(Terminator::Return(operand) | Terminator::Throw(operand)) => read(operand),
            Some(Terminator::Goto(_) | Terminator::Unreachable) | None => {}
        }
        for statement in &block.statements {
            if let Statement::Assign { dest, value } = statement
                && is_property_read(value)
                && let Some(ty) = local_ty(*dest)
            {
                counts.reads.add(emitter.typedness_of(ty));
            }
        }
        for statement in &block.statements {
            if let Statement::Assign {
                dest,
                value:
                    Rvalue::ClosureCall { callee, .. } | Rvalue::ClosureCallSpread { callee, .. },
            } = statement
            {
                let result =
                    local_ty(*dest).map_or(Typedness::Erased, |ty| emitter.typedness_of(ty));
                let callee = operand_local(callee)
                    .and_then(local_ty)
                    .map(|ty| emitter.typedness_of(ty))
                    .or_else(|| projected_read(emitter, callee, &local_ty))
                    .unwrap_or(Typedness::Concrete);
                counts.calls.add(result.max(callee));
            }
        }
        if let Some(Terminator::Call { callee, dest, .. }) = &block.terminator {
            let result = local_ty(*dest).map_or(Typedness::Erased, |ty| emitter.typedness_of(ty));
            let callee = match callee {
                Callee::Indirect(operand) => operand_local(operand)
                    .and_then(local_ty)
                    .map_or(Typedness::Concrete, |ty| emitter.typedness_of(ty)),
                Callee::Static(_) | Callee::Builtin(_) | Callee::BaseInit(_) => {
                    Typedness::Concrete
                }
            };
            counts.calls.add(result.max(callee));
        }
    }
}

/// The local an operand reads, when it reads a whole local.
fn operand_local(operand: &Operand) -> Option<smelt_mir::LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        _ => None,
    }
}

/// The typedness of an operand that reads a field or element of a local
/// (`base.field`, `base[index]`), or `None` for any other operand.
///
/// Classified by the projected type: the element of a list, set or tuple, the
/// value of a dictionary, the declared type of a class or interface field
/// (with the receiver's type arguments substituted for the owner's
/// parameters). A base whose projection the table cannot answer (an erased
/// receiver, a string) is classified by the base type itself.
fn projected_read(
    emitter: &FunctionEmitter<'_>,
    operand: &Operand,
    local_ty: &impl Fn(smelt_mir::LocalId) -> Option<TypeId>,
) -> Option<Typedness> {
    let (Operand::Copy(place) | Operand::Move(place)) = operand else {
        return None;
    };
    let (base, field) = match place {
        Place::Field { base, field } => (*base, Some(*field)),
        Place::Index { base, .. } => (*base, None),
        Place::Local(_) | Place::Global { .. } => return None,
    };
    let base_ty = local_ty(base)?;
    let projected = emitter.projected_type(base_ty, field);
    Some(projected.map_or_else(
        || emitter.typedness_of(base_ty),
        |types| {
            types
                .into_iter()
                .map(|ty| emitter.typedness_of(ty))
                .max()
                .unwrap_or(Typedness::Concrete)
        },
    ))
}

/// Whether an rvalue reads a property or an element.
fn is_property_read(value: &Rvalue) -> bool {
    matches!(
        value,
        Rvalue::OptionalField { .. }
        | Rvalue::OptionalIndex { .. }
        | Rvalue::ListIndex { .. }
        | Rvalue::TupleIndex { .. }
        | Rvalue::DictGet { .. }
    )
}

/// The source file a body belongs to: its entry block's span file, by path.
fn function_file(mir: &Mir, blocks: &[BasicBlock], entry: smelt_mir::BlockId) -> String {
    blocks
        .iter()
        .find(|block| block.id == entry)
        .or_else(|| blocks.first())
        .and_then(|block| mir.file_paths.get(&block.span.file))
        .cloned()
        .unwrap_or_else(|| "<synthesized>".to_owned())
}

/// Map every closure to the index of the function whose emitted body contains
/// it (directly, or through an enclosing closure).
fn closure_owners(mir: &Mir) -> HashMap<ClosureId, usize> {
    /// The closures a body constructs.
    fn created(blocks: &[BasicBlock]) -> impl Iterator<Item = ClosureId> + '_ {
        blocks.iter().flat_map(|block| {
            block.statements.iter().filter_map(|statement| match statement {
                Statement::Assign {
                    value: Rvalue::Closure { id, .. },
                    ..
                }
                | Statement::AssignPlace {
                    value: Rvalue::Closure { id, .. },
                    ..
                } => Some(*id),
                _ => None,
            })
        })
    }
    let mut owners = HashMap::new();
    for (index, function) in mir.functions.iter().enumerate() {
        for id in created(&function.blocks) {
            owners.entry(id).or_insert(index);
        }
    }
    // Nested closures inherit their parent's owner; iterate to a fixpoint.
    loop {
        let mut changed = false;
        for closure in &mir.closures {
            let Some(owner) = owners.get(&closure.id).copied() else {
                continue;
            };
            for id in created(&closure.blocks) {
                if let std::collections::hash_map::Entry::Vacant(entry) = owners.entry(id) {
                    entry.insert(owner);
                    changed = true;
                }
            }
        }
        if !changed {
            return owners;
        }
    }
}

impl FunctionEmitter<'_> {
    /// Apply the crate-wide decision whether this free function emits real
    /// Rust generics (see `EmitContext::populate_generic_functions`): a free
    /// function outside that set renders its type parameters erased. The same
    /// rule [`FunctionEmitter::emit`] applies before emitting a body.
    pub(super) fn apply_crate_generic_decision(&self) {
        if matches!(self.function.origin, HirOrigin::Body(_))
            && !self.context.is_generic_function(self.function.id)
        {
            *self.suppress_type_params.borrow_mut() = true;
        }
    }

    /// The type(s) a field (`Some(name)`) or element (`None`) read of a value
    /// of type `base` produces, when the type table answers it.
    fn projected_type(&self, base: TypeId, field: Option<smelt_hir::Symbol>) -> Option<Vec<TypeId>> {
        let base = self.flatten_optional_inner(base);
        match (self.mir.types.get(base)?, field) {
            (Type::List(item) | Type::Set(item), None) => Some(vec![*item]),
            (Type::Tuple(items), None) => Some(items.clone()),
            (Type::Dict(_, value) | Type::JsMap(_, value), _) => Some(vec![*value]),
            (Type::Class { name, args }, Some(field)) => {
                let (params, fields) = self
                    .mir
                    .classes
                    .iter()
                    .find(|class| class.name == *name)
                    .map(|class| (&class.type_params, &class.fields))
                    .or_else(|| {
                        self.mir
                            .interfaces
                            .iter()
                            .find(|interface| interface.name == *name)
                            .map(|interface| (&interface.type_params, &interface.fields))
                    })?;
                let declared = fields.iter().find(|candidate| candidate.name == field)?.ty;
                // A field reads at the receiver's instantiation: its owner's
                // parameters are the receiver's arguments, however deep they
                // sit in the field's type (`swap: () => Pair<T>` read off a
                // `Pair<f64>` is `() => Pair<f64>`, which is what the emitted
                // struct field is). MIR's `generic_records` pass interns the
                // substituted types.
                let substitutions: std::collections::HashMap<smelt_hir::Symbol, TypeId> = params
                    .iter()
                    .map(|def| def.name)
                    .zip(args.iter().copied())
                    .collect();
                Some(vec![crate::classes::substitute_existing_type(
                    self.mir,
                    declared,
                    &substitutions,
                )])
            }
            _ => None,
        }
    }

    /// Classify one type as the Rust output renders it in this function.
    pub(crate) fn typedness_of(&self, ty: TypeId) -> Typedness {
        let Ok(text) = self.type_text(ty) else {
            return Typedness::Erased;
        };
        if mentions_erased(&text) {
            Typedness::Erased
        } else if self.type_mentions_param(ty, 0) {
            Typedness::Generic
        } else {
            Typedness::Concrete
        }
    }

    /// Whether `ty` mentions a type parameter anywhere (bounded depth).
    fn type_mentions_param(&self, ty: TypeId, depth: usize) -> bool {
        /// Interned types are trees, but cap the walk defensively.
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return false;
        }
        let next = depth.saturating_add(1);
        let any = |items: &[TypeId]| items.iter().any(|item| self.type_mentions_param(*item, next));
        match self.mir.types.get(ty) {
            Some(Type::TypeParam { .. }) => true,
            Some(
                Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item),
            ) => self.type_mentions_param(*item, next),
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => any(&[*key, *value]),
            Some(Type::Tuple(items) | Type::Union(items)) => any(items),
            Some(Type::Class { args, .. }) => any(args),
            Some(Type::Function(function)) => {
                any(&function.params) || self.type_mentions_param(function.return_ty, next)
            }
            Some(Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            }) => any(&[*yield_ty, *return_ty, *next_ty]),
            Some(Type::GeneratorResult {
                yield_ty,
                return_ty,
            }) => any(&[*yield_ty, *return_ty]),
            _ => false,
        }
    }
}

/// Whether rendered Rust type text mentions the erased runtime value.
pub(crate) fn mentions_erased(text: &str) -> bool {
    ["SmeltUnknown", "SmeltErasedFunction"].iter().any(|erased| {
        text.match_indices(erased).any(|(at, _)| {
            let after = text[at + erased.len()..].chars().next();
            let before = text[..at].chars().next_back();
            !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                && !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erasure is detected as a whole identifier, nested or not.
    #[test]
    fn mentions_erased_matches_whole_identifiers() {
        assert!(mentions_erased("SmeltUnknown"));
        assert!(mentions_erased("SmeltList<SmeltUnknown>"));
        assert!(mentions_erased("Option<SmeltErasedFunction>"));
        assert!(!mentions_erased("SmeltUnknownish"));
        assert!(!mentions_erased("SmeltList<f64>"));
        assert!(!mentions_erased("i32"));
    }

    /// Counting and totals.
    #[test]
    fn kind_counts_add_and_total() {
        let mut counts = KindCounts::default();
        counts.add(Typedness::Concrete);
        counts.add(Typedness::Generic);
        counts.add(Typedness::Erased);
        counts.add(Typedness::Erased);
        assert_eq!(counts.typed(), 2);
        assert_eq!(counts.total(), 4);
    }
}
