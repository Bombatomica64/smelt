//! The IR's own traversal: what each statement, terminator and rvalue reads and
//! defines.
//!
//! Three levels answer the same question at growing width:
//!
//! * [`Rvalue::reads`] / [`Rvalue::reads_mut`] enumerate an rvalue's operands in
//!   evaluation order;
//! * [`Statement::reads`] / [`Statement::reads_mut`] add what the statement's
//!   target reads, and [`Statement::defs`] names the local it defines;
//! * [`Terminator::reads`] / [`Terminator::reads_mut`] and [`Terminator::defs`]
//!   do the same for control flow.
//!
//! The `read_locals` and `reads_local` helpers flatten any of these to the locals
//! involved, including the base and index of a projected place. Optimization
//! passes, the validator and the emitter all ask through this module, so a new
//! variant is described once and every analysis sees it.

use std::collections::{HashMap, HashSet};

use crate::{
    BasicBlock, BlockId, Callee, ClosureId, FuncId, GlobalProjection, LocalDecl, LocalId,
    LocalKind, MirClosure, MirFunction, Operand, Place, Rvalue, Statement, Terminator,
};

/// One thing a [`Statement`] reads.
#[derive(Debug, Clone, Copy)]
pub enum Read<'a> {
    /// A local reached directly as the base of an assignment target.
    Base(LocalId),
    /// An operand, either of the stored rvalue or of the target's index.
    Operand(&'a Operand),
}

/// One thing a [`Statement`] reads, in rewritable form.
#[derive(Debug)]
pub enum ReadMut<'a> {
    /// A local reached directly as the base of an assignment target.
    Base(&'a mut LocalId),
    /// An operand, either of the stored rvalue or of the target's index.
    Operand(&'a mut Operand),
}

/// Visits the operands of one op field through `$visit`, in order.
///
/// `$mutability` carries the `mut` for the kinds that borrow a sub-field.
macro_rules! visit_op_field {
    ($visit:ident, $mutability:tt, one, $field:ident) => { $visit($field); };
    ($visit:ident, $mutability:tt, opt, $field:ident) => {
        if let Some(operand) = $field {
            $visit(operand);
        }
    };
    ($visit:ident, $mutability:tt, many, $field:ident) => {
        for operand in $field {
            $visit(operand);
        }
    };
    ($visit:ident, ($($mutability:tt)?), splice, $field:ident) => {
        for item in $field {
            $visit(& $($mutability)? item.value);
        }
    };
    ($visit:ident, $mutability:tt, symbol_pairs, $field:ident) => {
        for (_, operand) in $field {
            $visit(operand);
        }
    };
    ($visit:ident, $mutability:tt, { $key:ty }, $field:ident) => {
        for (_, operand) in $field {
            $visit(operand);
        }
    };
    ($visit:ident, $mutability:tt, [$data:ty], $field:ident) => { let _ = $field; };
    ($visit:ident, $mutability:tt, (sym $label:literal), $field:ident) => { let _ = $field; };
    ($visit:ident, $mutability:tt, (ty), $field:ident) => { let _ = $field; };
}

/// Expands to the one operand walk over an [`Rvalue`], shared by
/// [`Rvalue::reads`] and [`Rvalue::reads_mut`] so the two cannot disagree: the
/// arms of the forms only MIR has, then one generated arm per op-table entry.
///
/// `$rvalue` is matched through default binding modes, so the same arms bind
/// shared or mutable references. There is no wildcard arm: a new variant is a
/// compile error here.
macro_rules! walk_rvalue_operands {
    ($rvalue:expr, $visit:ident, $mutability:tt, $(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
    match $rvalue {
        Self::Use(operand) => $visit(operand),
        Self::DictEntryPush { entry, item } => {
            $visit(entry);
            $visit(item);
        }
        Self::GeneratorYield { value, .. } => $visit(value),
        Self::List(items) | Self::Set(items) | Self::Tuple(items) => {
            for item in items {
                $visit(item);
            }
        }
        Self::Dict(entries) => {
            for (key, entry_value) in entries {
                $visit(key);
                $visit(entry_value);
            }
        }
        Self::Closure { captures, .. } => {
            for capture in captures {
                $visit(capture);
            }
        }
        Self::Binary { lhs, rhs, .. } => {
            $visit(lhs);
            $visit(rhs);
        }
        Self::BoundField { receiver, .. } => $visit(receiver),
        Self::UnionMethod { receiver, args, .. }
        | Self::BoundMethod { receiver, args, .. } => {
            $visit(receiver);
            for arg in args {
                $visit(arg);
            }
        }
        Self::GlobalGet { .. } => {}
        Self::GlobalSet { value, .. } => {
            $visit(value);
        }
        Self::Unary { operand, .. } => $visit(operand),
        Self::Struct { fields, .. } => {
            for (_, field_value) in fields {
                $visit(field_value);
            }
        }
        Self::ExternalClassInstance { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::Await(operand) => $visit(operand),
        $(
            Self::$name $({ $($field),* })? => {
                $($(visit_op_field!($visit, $mutability, $kind, $field);)*)?
            }
        )*
    }
    };
}

/// Generates the operand walks from the op table.
macro_rules! define_rvalue_walks {
    ($($table:tt)*) => {
        impl Rvalue {
            /// Visit every operand read by this rvalue, in evaluation order.
            pub fn reads(&self, mut visit: impl FnMut(&Operand)) {
                walk_rvalue_operands!(self, visit, (), $($table)*);
            }

            /// Visit every operand read by this rvalue mutably, in evaluation order.
            pub fn reads_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
                walk_rvalue_operands!(self, visit, (mut), $($table)*);
            }
        }
    };
}

smelt_hir::for_each_op!(define_rvalue_walks);

/// Generates [`Rvalue::is_fallible`] from the op table's `fallible` facts.
macro_rules! define_rvalue_is_fallible {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl Rvalue {
            /// Whether this rvalue can throw a catchable error, and so has to be
            /// the callee of a call terminator rather than an assignment.
            #[must_use]
            #[expect(
                unused_variables,
                reason = "each arm binds every field; a fact reads the ones it needs"
            )]
            pub const fn is_fallible(&self) -> bool {
                match self {
                    $(
                        Self::$name $({ $($field,)* })? => {
                            smelt_hir::op_fallible!($($fact $fact_value)*)
                        }
                    )*
                    // The hand-written variants outside the op table: none of
                    // them throws. Listed, not `_`, so a new variant must
                    // state its fallibility here.
                    Self::Use(..)
                    | Self::DictEntryPush { .. }
                    | Self::GeneratorYield { .. }
                    | Self::List(..)
                    | Self::Set(..)
                    | Self::Dict(..)
                    | Self::Tuple(..)
                    | Self::Binary { .. }
                    | Self::Unary { .. }
                    | Self::UnionMethod { .. }
                    | Self::BoundMethod { .. }
                    | Self::BoundField { .. }
                    | Self::Struct { .. }
                    | Self::ExternalClassInstance { .. }
                    | Self::Closure { .. }
                    | Self::GlobalGet { .. }
                    | Self::GlobalSet { .. }
                    | Self::Await(..) => false,
                }
            }
        }
    };
}

smelt_hir::for_each_op!(define_rvalue_is_fallible);

/// Generates [`Rvalue::mutated_receiver`] from the op table's `mutates` facts.
macro_rules! define_rvalue_mutated_receiver {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl Rvalue {
            /// The collection operand this rvalue mutates in place, if any.
            ///
            /// Copies and non-mutating projections (`DictCopy`, `SetCopy`, a
            /// non-mutating `ListSplice`) answer `None`.
            #[must_use]
            #[expect(
                unused_variables,
                reason = "each arm binds every field; a fact reads the ones it needs"
            )]
            pub const fn mutated_receiver(&self) -> Option<&Operand> {
                match self {
                    $(
                        Self::$name $({ $($field,)* })? => {
                            smelt_hir::op_mutates!($($fact $fact_value)*)
                        }
                    )*
                    Self::DictEntryPush { entry, .. } => Some(entry),
                    // The other hand-written variants build or read values and
                    // mutate no collection operand. Listed, not `_`, so a new
                    // variant must state what it mutates here.
                    Self::Use(..)
                    | Self::GeneratorYield { .. }
                    | Self::List(..)
                    | Self::Set(..)
                    | Self::Dict(..)
                    | Self::Tuple(..)
                    | Self::Binary { .. }
                    | Self::Unary { .. }
                    | Self::UnionMethod { .. }
                    | Self::BoundMethod { .. }
                    | Self::BoundField { .. }
                    | Self::Struct { .. }
                    | Self::ExternalClassInstance { .. }
                    | Self::Closure { .. }
                    | Self::GlobalGet { .. }
                    | Self::GlobalSet { .. }
                    | Self::Await(..) => None,
                }
            }
        }
    };
}

smelt_hir::for_each_op!(define_rvalue_mutated_receiver);

/// Expands to the one operand walk over a [`Terminator`], shared by
/// [`Terminator::reads`] and [`Terminator::reads_mut`].
macro_rules! walk_terminator_operands {
    ($terminator:expr, $visit:ident, $op_reads:ident) => {
        match $terminator {
            Terminator::Goto(_) => {}
            Terminator::Call { callee, args, .. } => {
                match callee {
                    Callee::Indirect(operand) => $visit(operand),
                    Callee::Op(op, _) => op.$op_reads(&mut $visit),
                    Callee::Static(_) | Callee::Builtin(_) | Callee::BaseInit(_) => {}
                }
                for arg in args {
                    $visit(arg);
                }
            }
            Terminator::Await { future: operand, .. }
            | Terminator::Switch { cond: operand, .. }
            | Terminator::Match {
                scrutinee: operand, ..
            }
            | Terminator::Return(operand)
            | Terminator::Throw(operand) => $visit(operand),
        }
    };
}

impl Operand {
    /// The place this operand reads, if it reads one.
    #[must_use]
    pub const fn place(&self) -> Option<&Place> {
        match self {
            Self::Copy(place) | Self::Move(place) => Some(place),
            Self::Const(_) => None,
        }
    }

    /// The local this operand reads directly, ignoring projections.
    #[must_use]
    pub const fn local(&self) -> Option<LocalId> {
        match self.place() {
            Some(Place::Local(local)) => Some(*local),
            Some(Place::Field { .. } | Place::Index { .. } | Place::Global { .. }) | None => None,
        }
    }

    /// Visit every local this operand reads, through any projection.
    pub fn read_locals(&self, visit: &mut dyn FnMut(LocalId)) {
        if let Some(place) = self.place() {
            place.read_locals(visit);
        }
    }

    /// Whether this operand reads `local`, through any projection.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        self.place().is_some_and(|place| place.reads_local(local))
    }
}

impl Place {
    /// Visit every local read when this place is read as a value: its base and
    /// any index operand. A global-rooted place has no base local, but its index
    /// operand still reads one.
    pub fn read_locals(&self, visit: &mut dyn FnMut(LocalId)) {
        match self {
            Self::Local(local) | Self::Field { base: local, .. } => visit(*local),
            Self::Index { base, index, .. } => {
                visit(*base);
                index.read_locals(visit);
            }
            Self::Global { projection, .. } => match projection {
                GlobalProjection::Field(_) => {}
                GlobalProjection::Index { index, .. } => index.read_locals(visit),
            },
        }
    }

    /// Whether reading this place as a value reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.read_locals(&mut |read| found |= read == local);
        found
    }

    /// Whether assigning to this place reads `local` before writing. A whole
    /// local is a definition and reads nothing; a projection reads its base,
    /// which stays live, and any index operand.
    #[must_use]
    pub fn target_reads_local(&self, local: LocalId) -> bool {
        !matches!(self, Self::Local(_)) && self.reads_local(local)
    }
}

impl Rvalue {
    /// Visit every local this rvalue reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|operand| operand.read_locals(&mut visit));
    }

    /// Whether any operand of this rvalue reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.reads(|operand| found |= operand.reads_local(local));
        found
    }
}

impl Read<'_> {
    /// Visit every local this read touches, through any projection.
    pub fn read_locals(self, visit: &mut dyn FnMut(LocalId)) {
        match self {
            Self::Base(local) => visit(local),
            Self::Operand(operand) => operand.read_locals(visit),
        }
    }
}

impl Statement {
    /// Visit everything this statement reads: the target's base and index (a
    /// whole-local target is a definition and contributes nothing), then the
    /// stored rvalue's operands.
    pub fn reads(&self, mut visit: impl FnMut(Read<'_>)) {
        match self {
            Self::Assign { value, .. } => value.reads(|operand| visit(Read::Operand(operand))),
            Self::AssignPlace { place, value } => {
                match place {
                    Place::Local(_) => {}
                    Place::Field { base, .. } => visit(Read::Base(*base)),
                    Place::Index { base, index, .. } => {
                        visit(Read::Base(*base));
                        visit(Read::Operand(index));
                    }
                    Place::Global { projection, .. } => {
                        if let GlobalProjection::Index { index, .. } = projection {
                            visit(Read::Operand(index));
                        }
                    }
                }
                value.reads(|operand| visit(Read::Operand(operand)));
            }
            // The container is mutated in place through its base, exactly like
            // an index target; `current` is the definition and is read only
            // through the stored rvalue.
            Self::DictEntryUpdate {
                base,
                index,
                default,
                current: _,
                value,
            } => {
                visit(Read::Base(*base));
                visit(Read::Operand(index));
                visit(Read::Operand(default));
                value.reads(|operand| visit(Read::Operand(operand)));
            }
        }
    }

    /// Visit everything [`Self::reads`] visits, in rewritable form.
    pub fn reads_mut(&mut self, mut visit: impl FnMut(ReadMut<'_>)) {
        match self {
            Self::Assign { value, .. } => {
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
            Self::AssignPlace { place, value } => {
                match place {
                    Place::Local(_) => {}
                    Place::Field { base, .. } => visit(ReadMut::Base(base)),
                    Place::Index { base, index, .. } => {
                        visit(ReadMut::Base(base));
                        visit(ReadMut::Operand(index));
                    }
                    Place::Global { projection, .. } => {
                        if let GlobalProjection::Index { index, .. } = projection {
                            visit(ReadMut::Operand(index));
                        }
                    }
                }
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
            Self::DictEntryUpdate {
                base,
                index,
                default,
                current: _,
                value,
            } => {
                visit(ReadMut::Base(base));
                visit(ReadMut::Operand(index));
                visit(ReadMut::Operand(default));
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
        }
    }

    /// The rvalue this statement evaluates.
    #[must_use]
    pub const fn value(&self) -> &Rvalue {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value,
        }
    }

    /// The rvalue this statement evaluates, in rewritable form.
    pub const fn value_mut(&mut self) -> &mut Rvalue {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value,
        }
    }

    /// The local this statement defines, if any. A projected assignment target
    /// mutates its base without defining it.
    #[must_use]
    pub const fn defs(&self) -> Option<LocalId> {
        match self {
            Self::Assign { dest: local, .. }
            | Self::AssignPlace {
                place: Place::Local(local),
                ..
            }
            | Self::DictEntryUpdate { current: local, .. } => Some(*local),
            Self::AssignPlace { .. } => None,
        }
    }

    /// Visit every local this statement reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|read| read.read_locals(&mut visit));
    }

    /// Whether this statement reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.read_locals(|read| found |= read == local);
        found
    }
}

impl Terminator {
    /// Visit every operand this terminator reads, including an indirect callee.
    pub fn reads(&self, mut visit: impl FnMut(&Operand)) {
        walk_terminator_operands!(self, visit, reads);
    }

    /// Visit every operand [`Self::reads`] visits, in rewritable form.
    pub fn reads_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
        walk_terminator_operands!(self, visit, reads_mut);
    }

    /// The local this terminator defines: a call's or await's destination.
    #[must_use]
    pub const fn defs(&self) -> Option<LocalId> {
        match self {
            Self::Call { dest, .. } | Self::Await { dest, .. } => Some(*dest),
            Self::Goto(_)
            | Self::Switch { .. }
            | Self::Match { .. }
            | Self::Return(_)
            | Self::Throw(_) => None,
        }
    }

    /// Every control-flow edge out of this terminator, each with what it
    /// defines.
    ///
    /// A call's or await's `dest` is written only when it completes normally,
    /// so it is defined on its `target` edge and NOT on its unwind edge; the
    /// unwind edge instead defines the handler's exception local. Dataflow that
    /// meets per edge through this cannot treat `dest` as defined in a catch
    /// block. The edges are in [`Self::successors`] order.
    #[must_use]
    pub fn edges(&self) -> Vec<Edge> {
        match self {
            Self::Goto(target) => vec![Edge { target: *target, defines: None }],
            Self::Call { dest, target, unwind, .. } | Self::Await { dest, target, unwind, .. } => {
                unwind
                    .iter()
                    .map(|handler| Edge {
                        target: handler.catch_block,
                        defines: handler.exception_local,
                    })
                    .chain(std::iter::once(Edge { target: *target, defines: Some(*dest) }))
                    .collect()
            }
            Self::Switch { then_block, else_block, .. } => vec![
                Edge { target: *then_block, defines: None },
                Edge { target: *else_block, defines: None },
            ],
            Self::Match { arms, default, .. } => arms
                .iter()
                .map(|arm| arm.target)
                .chain(*default)
                .map(|target| Edge { target, defines: None })
                .collect(),
            Self::Return(_) | Self::Throw(_) => Vec::new(),
        }
    }

    /// Visit every local this terminator reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|operand| operand.read_locals(&mut visit));
    }

    /// Whether this terminator reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.reads(|operand| found |= operand.reads_local(local));
        found
    }
}

/// One control-flow edge out of a [`Terminator`]: where it goes and the local,
/// if any, that taking it defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// The successor block.
    pub target: BlockId,
    /// The local this edge writes: a call's `dest` on its normal edge, a
    /// handler's exception local on an unwind edge.
    pub defines: Option<LocalId>,
}

/// Which table entry a [`Body`] belongs to.
///
/// MIR has two kinds of executable body, [`MirFunction`] and [`MirClosure`],
/// that share one CFG shape. Every whole-program walk that has to see "every
/// body" goes through [`crate::Mir::bodies`], which yields both kinds tagged with
/// this owner, so a check written once covers closures as well as functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyOwner {
    /// The body of `mir.functions[id]`.
    Function(FuncId),
    /// The body of `mir.closures[id]`.
    Closure(ClosureId),
}

/// One body's locals and blocks, whether a function's or a closure's.
///
/// Definition and read counts mean the same in both, so they live here and
/// [`MirFunction`] and [`MirClosure`] only hand out the view. A pass that
/// only has the two slices may build one directly.
#[derive(Debug, Clone, Copy)]
pub struct Body<'a> {
    /// Every local of the body.
    pub locals: &'a [LocalDecl],
    /// Every block of the body.
    pub blocks: &'a [BasicBlock],
}

/// One whole body as the program holds it: which table entry it belongs to,
/// its signature, locals, blocks and entry.
///
/// Can only be made from a real table entry (`#[non_exhaustive]`), by
/// [`MirFunction::as_mir_body`], [`MirClosure::as_mir_body`] or
/// [`crate::Mir::bodies`], so its fields always describe one real body.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MirBody<'a> {
    /// The table entry this body belongs to.
    pub owner: BodyOwner,
    /// Parameter locals, in order.
    pub params: &'a [LocalId],
    /// Every local of the body.
    pub locals: &'a [LocalDecl],
    /// Every block of the body.
    pub blocks: &'a [BasicBlock],
    /// The entry block.
    pub entry: BlockId,
    /// The closure's captures, whose target locals are bound on entry; empty
    /// for a function.
    pub captures: &'a [crate::MirClosureCapture],
}

impl<'a> MirBody<'a> {
    /// The locals-and-blocks view, for counts.
    #[must_use]
    pub const fn view(self) -> Body<'a> {
        Body { locals: self.locals, blocks: self.blocks }
    }

    /// The declaration of `local`, if the index is in range.
    #[must_use]
    pub fn local_decl(self, local: LocalId) -> Option<&'a LocalDecl> {
        self.view().local_decl(local)
    }

    /// The locals bound before the entry block runs: the parameters and, in a
    /// closure, every capture's target local.
    pub fn defined_on_entry(self) -> impl Iterator<Item = LocalId> + 'a {
        self.params
            .iter()
            .copied()
            .chain(self.captures.iter().filter_map(|capture| capture.target_local))
    }
}

/// A mutable view of one body: its locals and blocks may be rewritten, its
/// owner, parameters and entry may not.
///
/// Yielded by [`crate::Mir::bodies_mut`], the one walk over every function and
/// closure body that a rewriting pass uses.
#[derive(Debug)]
pub struct MirBodyMut<'a> {
    /// The table entry this body belongs to.
    pub owner: BodyOwner,
    /// Parameter locals, in order.
    pub params: &'a [LocalId],
    /// Every local of the body.
    pub locals: &'a mut Vec<LocalDecl>,
    /// Every block of the body.
    pub blocks: &'a mut Vec<BasicBlock>,
    /// The entry block.
    pub entry: BlockId,
}

impl crate::Mir {
    /// Every executable body: each function in table order, then each closure
    /// in table order.
    ///
    /// Use this, not `mir.functions`, for any check or analysis that must hold
    /// in closure bodies too (validation, escape analysis, capture checks).
    pub fn bodies(&self) -> impl Iterator<Item = MirBody<'_>> {
        self.functions
            .iter()
            .map(MirFunction::as_mir_body)
            .chain(self.closures.iter().map(MirClosure::as_mir_body))
    }

    /// [`Self::bodies`], mutably.
    pub fn bodies_mut(&mut self) -> impl Iterator<Item = MirBodyMut<'_>> {
        self.functions
            .iter_mut()
            .map(MirFunction::as_mir_body_mut)
            .chain(self.closures.iter_mut().map(MirClosure::as_mir_body_mut))
    }

    /// The body `owner` names, if it is in range.
    #[must_use]
    pub fn body(&self, owner: BodyOwner) -> Option<MirBody<'_>> {
        match owner {
            BodyOwner::Function(id) => self.function(id).map(MirFunction::as_mir_body),
            BodyOwner::Closure(id) => self.closure(id).map(MirClosure::as_mir_body),
        }
    }
}

impl<'a> Body<'a> {
    /// How many times each local is defined anywhere in this body: every
    /// [`Statement::defs`] and [`Terminator::defs`]. This is the one answer to
    /// "is this local assigned once".
    #[must_use]
    pub fn assignment_counts(self) -> HashMap<LocalId, usize> {
        let mut counts: HashMap<LocalId, usize> = HashMap::new();
        for block in self.blocks {
            let statements = block.statements.iter().map(Statement::defs);
            let terminator = block.terminator.as_ref().map(Terminator::defs);
            for local in statements.chain(terminator).flatten() {
                let count = counts.entry(local).or_default();
                *count = count.saturating_add(1);
            }
        }
        counts
    }

    /// How many times `local` is defined; see [`Self::assignment_counts`].
    #[must_use]
    pub fn assignment_count(self, local: LocalId) -> usize {
        self.blocks
            .iter()
            .map(|block| {
                let statements = block.statements.iter().map(Statement::defs);
                let terminator = block.terminator.as_ref().map(Terminator::defs);
                statements
                    .chain(terminator)
                    .filter(|defined| *defined == Some(local))
                    .count()
            })
            .fold(0, usize::saturating_add)
    }

    /// How many times `local` is read anywhere in this body.
    #[must_use]
    pub fn read_count(self, local: LocalId) -> usize {
        let mut count: usize = 0;
        let mut tally = |read: LocalId| count = count.saturating_add(usize::from(read == local));
        for block in self.blocks {
            for statement in &block.statements {
                statement.read_locals(&mut tally);
            }
            if let Some(terminator) = &block.terminator {
                terminator.read_locals(&mut tally);
            }
        }
        count
    }

    /// How many times each local is read anywhere in this body.
    #[must_use]
    pub fn read_counts(self) -> HashMap<LocalId, usize> {
        let mut counts: HashMap<LocalId, usize> = HashMap::new();
        let mut tally = |read: LocalId| {
            let count = counts.entry(read).or_default();
            *count = count.saturating_add(1);
        };
        for block in self.blocks {
            for statement in &block.statements {
                statement.read_locals(&mut tally);
            }
            if let Some(terminator) = &block.terminator {
                terminator.read_locals(&mut tally);
            }
        }
        counts
    }

    /// Every temporary assigned once and read once, in one walk of the body.
    #[must_use]
    pub fn single_use_temps(self) -> HashSet<LocalId> {
        let assigned = self.assignment_counts();
        self.read_counts()
            .into_iter()
            .filter(|(local, reads)| *reads == 1 && assigned.get(local) == Some(&1) && self.is_temp(*local))
            .map(|(local, _)| local)
            .collect()
    }

    /// The declaration of `local`, if the index is in range.
    #[must_use]
    pub fn local_decl(self, local: LocalId) -> Option<&'a LocalDecl> {
        self.locals.get(usize::try_from(local.0).unwrap_or(usize::MAX))
    }

    /// Whether `local` is a compiler temporary.
    #[must_use]
    pub fn is_temp(self, local: LocalId) -> bool {
        self.local_decl(local).is_some_and(|decl| decl.kind == LocalKind::Temp)
    }

    /// Whether `local` is a temporary assigned once and read once.
    #[must_use]
    pub fn is_single_use_temp(self, local: LocalId) -> bool {
        self.is_temp(local) && self.assignment_count(local) == 1 && self.read_count(local) == 1
    }
}

impl MirFunction {
    /// This function's locals and blocks.
    #[must_use]
    pub fn body(&self) -> Body<'_> {
        Body { locals: &self.locals, blocks: &self.blocks }
    }

    /// This body with its owner, signature and entry.
    #[must_use]
    pub fn as_mir_body(&self) -> MirBody<'_> {
        MirBody {
            owner: BodyOwner::Function(self.id),
            params: &self.params,
            locals: &self.locals,
            blocks: &self.blocks,
            entry: self.entry,
            captures: &[],
        }
    }

    /// [`Self::as_mir_body`], mutably.
    pub fn as_mir_body_mut(&mut self) -> MirBodyMut<'_> {
        MirBodyMut {
            owner: BodyOwner::Function(self.id),
            params: &self.params,
            locals: &mut self.locals,
            blocks: &mut self.blocks,
            entry: self.entry,
        }
    }

    /// See [`Body::assignment_counts`].
    #[must_use]
    pub fn assignment_counts(&self) -> HashMap<LocalId, usize> {
        self.body().assignment_counts()
    }

    /// See [`Body::assignment_count`].
    #[must_use]
    pub fn assignment_count(&self, local: LocalId) -> usize {
        self.body().assignment_count(local)
    }

    /// See [`Body::read_count`].
    #[must_use]
    pub fn read_count(&self, local: LocalId) -> usize {
        self.body().read_count(local)
    }

    /// See [`Body::is_temp`].
    #[must_use]
    pub fn is_temp(&self, local: LocalId) -> bool {
        self.body().is_temp(local)
    }

    /// See [`Body::is_single_use_temp`].
    #[must_use]
    pub fn is_single_use_temp(&self, local: LocalId) -> bool {
        self.body().is_single_use_temp(local)
    }
}

impl MirClosure {
    /// This closure's locals and blocks.
    #[must_use]
    pub fn body(&self) -> Body<'_> {
        Body { locals: &self.locals, blocks: &self.blocks }
    }

    /// This body with its owner, signature and entry.
    #[must_use]
    pub fn as_mir_body(&self) -> MirBody<'_> {
        MirBody {
            owner: BodyOwner::Closure(self.id),
            params: &self.params,
            locals: &self.locals,
            blocks: &self.blocks,
            entry: self.entry,
            captures: &self.captures,
        }
    }

    /// [`Self::as_mir_body`], mutably.
    pub fn as_mir_body_mut(&mut self) -> MirBodyMut<'_> {
        MirBodyMut {
            owner: BodyOwner::Closure(self.id),
            params: &self.params,
            locals: &mut self.locals,
            blocks: &mut self.blocks,
            entry: self.entry,
        }
    }
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "fixtures index their own one block")]
mod tests {
    //! The read/define contract every analysis relies on, over one function
    //! with locals `%0`..`%2` and a single block.

    use smelt_hir::{BodyId, FileId, Span, Symbol, TypeId};

    use crate::{
        BlockId, Callee, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, MirFunction, Operand,
        Place, Rvalue, Statement, Terminator,
    };

    const A: LocalId = LocalId(0);
    const B: LocalId = LocalId(1);
    const C: LocalId = LocalId(2);

    /// `copy %n`.
    fn copy(local: LocalId) -> Operand {
        Operand::Copy(Place::Local(local))
    }

    /// A body of temporaries running `statements` and ending in `terminator`.
    fn function(statements: Vec<Statement>, terminator: Terminator) -> MirFunction {
        let span = Span::new(FileId(0), 0, 0);
        let mut function =
            MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), TypeId(0), span);
        for _ in 0_i32..3_i32 {
            function.push_local(LocalDecl { ty: TypeId(0), kind: LocalKind::Temp, span });
        }
        function.blocks[0].statements = statements;
        function.blocks[0].terminator = Some(terminator);
        function
    }

    /// `%dest = callee(args)` through an indirect callee.
    fn indirect_call(callee: LocalId, args: Vec<Operand>, dest: LocalId) -> Terminator {
        Terminator::Call {
            callee: Callee::Indirect(copy(callee)),
            args,
            dest,
            target: BlockId(0),
            unwind: None,
        }
    }

    /// A whole-local target is a definition; a projected one reads its base.
    #[test]
    fn whole_local_target_defines_and_projected_target_reads() {
        let whole = Statement::AssignPlace {
            place: Place::Local(A),
            value: Rvalue::Use(copy(B)),
        };
        assert_eq!(whole.defs(), Some(A));
        assert!(!whole.reads_local(A), "a whole-local write reads nothing of it");
        assert!(whole.reads_local(B));

        let projected = Statement::AssignPlace {
            place: Place::Index {
                base: A,
                index: Box::new(copy(C)),
                negative: crate::NegativeIndex::FromEnd,
            },
            value: Rvalue::Use(copy(B)),
        };
        assert_eq!(projected.defs(), None, "a projection mutates, it does not define");
        for local in [A, B, C] {
            assert!(projected.reads_local(local), "{local:?} is read");
        }
    }

    /// An indirect callee is a read like any argument.
    #[test]
    fn read_count_includes_an_indirect_callee() {
        let body = function(Vec::new(), indirect_call(A, vec![copy(A), copy(B)], C));
        assert_eq!(body.read_count(A), 2, "once as callee, once as argument");
        assert_eq!(body.read_count(B), 1);
    }

    /// `splice` mutates its receiver; a non-mutating splice is a copy.
    #[test]
    fn a_dict_entry_push_mutates_through_its_entry_and_reads_it_first() {
        let entry = Operand::Copy(Place::Index {
            base: A,
            index: Box::new(copy(B)),
            negative: crate::NegativeIndex::OutOfRange,
        });
        let push = Rvalue::DictEntryPush { entry: entry.clone(), item: copy(C) };
        assert_eq!(push.mutated_receiver(), Some(&entry));
        let mut read = Vec::new();
        push.read_locals(|local| read.push(local));
        assert_eq!(read, [A, B, C]);
    }

    #[test]
    fn fallibility_is_stated_per_op_not_per_family() {
        use smelt_hir::{UriTranscodeOp, UrlField, UrlOp};
        let uri = |op| Rvalue::UriTranscode { op, operand: copy(A) };
        // Every URI transcoder can throw a `URIError`: a decoder on malformed
        // percent-encoding, an encoder on a lone surrogate (which made the
        // encoders fallible; the URL ops below still differ per op).
        assert!(uri(UriTranscodeOp::Decode).is_fallible());
        assert!(uri(UriTranscodeOp::Encode).is_fallible());
        assert!(uri(UriTranscodeOp::EncodeComponent).is_fallible());
        let url = |op| Rvalue::UrlOp { op, url: copy(A), args: Vec::new() };
        assert!(url(UrlOp::Set(UrlField::Href)).is_fallible());
        assert!(!url(UrlOp::Set(UrlField::Hash)).is_fallible());
        assert!(!url(UrlOp::Get(UrlField::Href)).is_fallible());
        assert!(Rvalue::UrlNew { input: copy(A), base: None }.is_fallible());
        assert!(!Rvalue::Use(copy(A)).is_fallible());
    }

    #[test]
    fn a_call_terminator_reads_the_operands_of_the_op_it_carries() {
        let mut terminator = Terminator::Call {
            callee: Callee::Op(Box::new(Rvalue::UrlNew {
                input: copy(A),
                base: Some(copy(B)),
            }), None),
            args: Vec::new(),
            dest: C,
            target: BlockId(0),
            unwind: None,
        };
        let mut read = Vec::new();
        terminator.reads(|operand| read.extend(operand.local()));
        assert_eq!(read, [A, B]);
        terminator.reads_mut(|operand| *operand = copy(C));
        let mut rewritten = Vec::new();
        terminator.reads(|operand| rewritten.extend(operand.local()));
        assert_eq!(rewritten, [C, C]);
    }

    #[test]
    fn reads_follow_evaluation_order() {
        let fill = Rvalue::ListFill {
            list: copy(A),
            value: copy(B),
            start: Some(copy(C)),
            end: None,
        };
        let mut read = Vec::new();
        fill.reads(|operand| read.extend(operand.local()));
        assert_eq!(read, [A, B, C]);
    }

    #[test]
    fn mutated_receiver_follows_the_splice_mutate_flag() {
        let splice = |mutate| Rvalue::ListSplice {
            list: copy(A),
            start: copy(B),
            delete_count: None,
            items: Vec::new(),
            mutate,
        };
        assert_eq!(splice(true).mutated_receiver().and_then(Operand::local), Some(A));
        assert!(splice(false).mutated_receiver().is_none());
        let unshift = Rvalue::ListUnshift { list: copy(A), items: Vec::new() };
        assert_eq!(unshift.mutated_receiver().and_then(Operand::local), Some(A));
        assert!(Rvalue::Use(copy(A)).mutated_receiver().is_none());
    }

    /// Every kind of definition counts toward "assigned once".
    #[test]
    fn assignment_count_sees_every_definition() {
        let body = function(
            vec![
                Statement::Assign { dest: A, value: Rvalue::Use(copy(B)) },
                Statement::AssignPlace { place: Place::Local(A), value: Rvalue::Use(copy(B)) },
            ],
            indirect_call(B, Vec::new(), A),
        );
        assert_eq!(body.assignment_count(A), 3);
        assert_eq!(body.assignment_counts().get(&A), Some(&3));
        assert_eq!(body.assignment_count(B), 0);
    }
}
