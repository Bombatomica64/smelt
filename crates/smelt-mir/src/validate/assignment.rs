//! Definite assignment: every read finds its local assigned, and the only way
//! past a binding that may not be is a check that raises when it is not.
//!
//! The facts come from [`crate::definite_assignment`], the same analysis the
//! lowering pass `lower::passes::unbound_bindings` used to decide which source
//! bindings can be read unassigned. This module enforces that decision, so a
//! later pass cannot reintroduce a read the source language would have
//! rejected at run time:
//!
//! * **No local is assumed assigned.** Only the parameters and, in a closure,
//!   the targets of initialized captures ([`CaptureInit`]) are assigned on
//!   entry. A source binding gets no default: a read the analysis cannot prove
//!   assigned is an error. A [`Rvalue::BindingCheck`] (a call terminator) is
//!   the one operation that may name an unassigned binding; past its normal
//!   edge the binding is assigned.
//! * **A [`LocalInit::MaybeUnbound`] local** is written only by
//!   [`Statement::Assign`], since codegen stores it as a `SmeltBinding<T>` that
//!   a whole-value assignment fills. A capture shares its storage with the
//!   closure's copy, so a capture's source and target agree on [`LocalInit`],
//!   and a source that is not definitely assigned at the construction must be
//!   `MaybeUnbound`.
//! * **Every block is reachable.** Lowering and every optimization pass
//!   remove the blocks no edge reaches ([`crate::reachability`]), so a block
//!   the analysis cannot reach is a bug, and every block is checked.

use std::collections::HashSet;

use crate::definite_assignment::{
    Assigned, CaptureInit, DefiniteAssignment, Point, non_capture_reads,
};
use crate::reachability::reachable_blocks;
use crate::{
    Callee, LocalId, LocalInit, Mir, MirBody, Operand, Place, Rvalue, Statement, Terminator,
};

use super::structure::validate_local_exists;
use super::{ValidationError, error, local_index};

/// Checks one body's definite-assignment rules (see the module docs).
pub(super) fn validate_definite_assignment(
    mir: &Mir,
    body: MirBody<'_>,
    captures: &CaptureInit,
    errors: &mut Vec<ValidationError>,
) {
    if body.blocks.get(body.entry.index()).is_none() {
        return;
    }
    let reachable = reachable_blocks(body.blocks, body.entry);
    for block in body.blocks {
        if !reachable.contains(&block.id) {
            errors.push(error(format!(
                "{} has block {:?}, which no edge reaches from the entry",
                super::owner_name(mir, body.owner),
                block.id
            )));
        }
    }
    let analysis = DefiniteAssignment::of(body, captures.entry_defs(body));
    let checker = Checker { mir, body };
    analysis.walk(body, |point, state| match point {
        Point::Statement { statement, .. } => checker.statement(statement, state, errors),
        Point::Terminator { terminator, .. } => checker.terminator(terminator, state, errors),
    });
    checker.writes(errors);
}

/// One body's checks, with what they report against.
#[derive(Clone, Copy)]
struct Checker<'a> {
    /// The program, for names and closure captures.
    mir: &'a Mir,
    /// The body being checked.
    body: MirBody<'a>,
}

impl Checker<'_> {
    /// Whether `local` may be read unassigned.
    fn maybe_unbound(self, local: LocalId) -> bool {
        self.body
            .local_decl(local)
            .is_some_and(|decl| decl.init == LocalInit::MaybeUnbound)
    }

    /// Checks the reads of one statement.
    fn statement(self, statement: &Statement, state: &Assigned, errors: &mut Vec<ValidationError>) {
        if let Rvalue::BindingCheck { .. } = statement.value() {
            errors.push(error(format!(
                "{} has a binding check outside a call terminator",
                super::owner_name(self.mir, self.body.owner)
            )));
        }
        match statement {
            // The entry is located (base, key, seed read) before `current` is
            // bound, and only then is the stored value evaluated.
            Statement::DictEntryUpdate {
                base,
                index,
                default,
                current,
                value,
            } => {
                self.read(*base, state, errors);
                for operand in [index, default] {
                    operand.read_locals(&mut |local| self.read(local, state, errors));
                }
                let mut bound = state.clone();
                bound.insert(*current);
                value.read_locals(|local| self.read(local, &bound, errors));
            }
            Statement::Assign { .. } | Statement::AssignPlace { .. } => {
                let mut reads = Vec::new();
                non_capture_reads(statement, |local| reads.push(local));
                for local in reads {
                    self.read(local, state, errors);
                }
                if let Rvalue::Closure { id, captures } = statement.value() {
                    self.captures(*id, captures, state, errors);
                }
            }
        }
    }

    /// Checks the reads of one terminator.
    fn terminator(
        self,
        terminator: &Terminator,
        state: &Assigned,
        errors: &mut Vec<ValidationError>,
    ) {
        if let Terminator::Call {
            callee: Callee::Op(op, _),
            args,
            ..
        } = terminator
            && let Rvalue::BindingCheck { binding, .. } = op.as_ref()
        {
            if binding.local().is_none() {
                errors.push(error(format!(
                    "{} checks something other than a local binding",
                    super::owner_name(self.mir, self.body.owner)
                )));
            }
            for arg in args {
                arg.read_locals(&mut |local| self.read(local, state, errors));
            }
            return;
        }
        let mut reads = Vec::new();
        terminator.read_locals(|local| reads.push(local));
        for local in reads {
            self.read(local, state, errors);
        }
    }

    /// A read of `local`'s value: it must be definitely assigned here.
    fn read(self, local: LocalId, state: &Assigned, errors: &mut Vec<ValidationError>) {
        if !state.must.contains(&local) {
            errors.push(error(format!(
                "{} reads local {local:?} {} before it is definitely defined",
                super::owner_name(self.mir, self.body.owner),
                local_detail(self.mir, self.body, local)
            )));
        }
    }

    /// The captures of one closure construction.
    fn captures(
        self,
        id: crate::ClosureId,
        operands: &[Operand],
        state: &Assigned,
        errors: &mut Vec<ValidationError>,
    ) {
        let targets = self
            .mir
            .closure(id)
            .map_or(&[][..], |closure| closure.captures.as_slice());
        for (index, operand) in operands.iter().enumerate() {
            let Some(source) = operand.local() else {
                continue;
            };
            let source_unbound = self.maybe_unbound(source);
            if !source_unbound && !state.must.contains(&source) {
                errors.push(error(format!(
                    "{} captures local {source:?} {} into closure {id:?} before it is definitely defined, but it is not MaybeUnbound",
                    super::owner_name(self.mir, self.body.owner),
                    local_detail(self.mir, self.body, source)
                )));
            }
            let target_init = targets
                .get(index)
                .and_then(|capture| capture.target_local)
                .and_then(|target| self.mir.closure(id)?.locals.get(target.index()))
                .map(|decl| decl.init == LocalInit::MaybeUnbound);
            if let Some(target_unbound) = target_init
                && target_unbound != source_unbound
            {
                errors.push(error(format!(
                    "{} captures local {source:?} into closure {id:?} capture {index}, whose target disagrees on whether it may be unassigned",
                    super::owner_name(self.mir, self.body.owner)
                )));
            }
        }
    }

    /// Checks that every write to a `MaybeUnbound` local is an
    /// [`Statement::Assign`].
    fn writes(self, errors: &mut Vec<ValidationError>) {
        let mut written: HashSet<LocalId> = HashSet::new();
        for block in self.body.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::AssignPlace {
                        place: Place::Local(local),
                        ..
                    }
                    | Statement::DictEntryUpdate { current: local, .. } => {
                        written.insert(*local);
                    }
                    Statement::Assign { .. } | Statement::AssignPlace { .. } => {}
                }
                if let Rvalue::GeneratorYield {
                    unwind: Some(handler),
                    ..
                } = statement.value()
                    && let Some(local) = handler.exception_local
                {
                    written.insert(local);
                }
            }
            if let Some(
                Terminator::Call { dest, unwind, .. } | Terminator::Await { dest, unwind, .. },
            ) = &block.terminator
            {
                written.insert(*dest);
                if let Some(local) = unwind.and_then(|handler| handler.exception_local) {
                    validate_local_exists(self.body, local, errors);
                    written.insert(local);
                }
            }
        }
        let mut wrong: Vec<LocalId> = written
            .into_iter()
            .filter(|local| self.maybe_unbound(*local))
            .collect();
        wrong.sort_unstable_by_key(|local| local.0);
        for local in wrong {
            errors.push(error(format!(
                "{} writes MaybeUnbound local {local:?} {} other than by a whole-local assignment",
                super::owner_name(self.mir, self.body.owner),
                local_detail(self.mir, self.body, local)
            )));
        }
    }
}

/// Render a local declaration for definite-assignment diagnostics.
fn local_detail(mir: &Mir, body: MirBody<'_>, local: LocalId) -> String {
    let Some(decl) = body.locals.get(local_index(local)) else {
        return "<unknown local>".to_owned();
    };
    let kind = match decl.kind {
        crate::LocalKind::Param { symbol } => symbol
            .and_then(|param_symbol| mir.symbols.get(param_symbol))
            .map_or_else(|| "param".to_owned(), |name| format!("param `{name}`")),
        crate::LocalKind::Temp => "temp".to_owned(),
        crate::LocalKind::UserBinding(symbol) => {
            format!(
                "binding `{}`",
                mir.symbols.get(symbol).unwrap_or("<unknown>")
            )
        }
    };
    format!(
        "({kind}, span file {} start {} end {})",
        decl.span.file.0, decl.span.start, decl.span.end
    )
}
