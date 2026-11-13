//! Source bindings that can be read before they are assigned.
//!
//! Both source languages let a program observe a binding with no value, and
//! both raise when it does: Python raises `UnboundLocalError` (`NameError` for
//! a module-level name or a free variable) when a name is read on a path that
//! did not assign it, and JavaScript raises `ReferenceError` when a `let`,
//! `const`, `class` or function binding is read in its temporal dead zone,
//! which a type-checked program can only do through a closure called early or
//! through a `switch` case that skipped the declaration.
//!
//! This pass finds every such read with the definite-assignment analysis of
//! [`crate::definite_assignment`] and gives it the language's behaviour:
//!
//! 1. A source binding that some read (or some closure that captures it) can
//!    reach before its first assignment is marked
//!    [`LocalInit::MaybeUnbound`]. Codegen stores it as `SmeltBinding<T>`,
//!    which knows it is empty; every other binding keeps its plain `T`.
//! 2. Every read of such a binding that the analysis cannot prove assigned is
//!    preceded by an [`Rvalue::BindingCheck`]: a call terminator that raises
//!    the language's error when the binding is unassigned, whose unwind edge is
//!    the handler lowering recorded for the read's position ([`HandlerMarks`]),
//!    so the source's own `try` catches the error. Past a check the binding is
//!    assigned, so the read itself, and every read after it on the same path,
//!    is an ordinary read the validator proves.
//! 3. A write to such a binding is a whole-local [`Statement::Assign`]: a call
//!    that returned into it, or a handler that caught into it, goes through a
//!    fresh temporary and a landing block instead.
//!
//! A closure capture is not a read: capturing shares the binding's storage.
//! When a capture's source is unassigned at the construction, both the source
//! and the closure's copy are marked; the closure's reads are proven when the
//! construction provably cannot run before the source is assigned (a recursive
//! closure, see [`CaptureInit`]), and checked otherwise.
//!
//! A check answers nothing, so it adds no temporary of the binding's type:
//! reads stay reads of the binding itself, which keeps every value the
//! program had, at the type it had.
//!
//! The validator (`validate::assignment`) re-derives all of this and rejects
//! MIR that breaks it, so a later pass cannot silently reintroduce a read of
//! an unassigned binding.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Language, UnboundRead};

use crate::definite_assignment::{CaptureInit, DefiniteAssignment, Point, non_capture_reads};
use crate::{
    BasicBlock, BlockId, BodyOwner, Callee, ExceptionHandler, LocalDecl, LocalId, LocalInit,
    LocalKind, Mir, MirClosureCapture, Operand, Place, Rvalue, Statement, Terminator,
};

use super::super::scope::HandlerMarks;
use super::super::{LowerError, LoweringFacts};

/// Marks every possibly-unassigned source binding and gives each of its reads
/// the source language's behaviour (see the module docs).
///
/// # Errors
///
/// A body whose shape the rewrite cannot handle: a binding defined by an
/// in-place dict-entry update, which lowering never produces.
pub(in crate::lower) fn check_unbound_bindings(
    mir: &mut Mir,
    facts: &LoweringFacts,
) -> Result<(), LowerError> {
    let captures = CaptureInit::of(mir);
    let unbound = possibly_unbound_bindings(mir, &captures);
    if unbound.values().all(HashSet::is_empty) {
        return Ok(());
    }
    let empty_marks = HandlerMarks::default();
    let none_ty = mir.types.intern(smelt_hir::Type::None);
    for (index, function) in mir.functions.iter_mut().enumerate() {
        let owner = BodyOwner::Function(function.id);
        let Some(locals) = unbound.get(&owner).filter(|locals| !locals.is_empty()) else {
            continue;
        };
        let module_body = facts.module_bodies.contains(&function.id);
        let body = BodyParts {
            locals: &mut function.locals,
            blocks: &mut function.blocks,
            entry: function.entry,
            entry_defs: function.params.clone(),
            captures: &[],
            language: function.language,
            module_body,
            none_ty,
        };
        let marks = facts.handler_marks.get(&owner).unwrap_or(&empty_marks);
        rewrite_body(body, locals, marks).map_err(|message| LowerError {
            message: format!("function {index}: {message}"),
            span: None,
        })?;
    }
    for closure in &mut mir.closures {
        let owner = BodyOwner::Closure(closure.id);
        let Some(locals) = unbound.get(&owner).filter(|locals| !locals.is_empty()) else {
            continue;
        };
        let mut entry_defs = closure.params.clone();
        entry_defs.extend(closure.captures.iter().enumerate().filter_map(|(index, capture)| {
            capture.target_local.filter(|_| captures.is_initialized(closure.id, index))
        }));
        let body = BodyParts {
            locals: &mut closure.locals,
            blocks: &mut closure.blocks,
            entry: closure.entry,
            entry_defs,
            captures: &closure.captures,
            language: closure.language,
            module_body: false,
            none_ty,
        };
        let marks = facts.handler_marks.get(&owner).unwrap_or(&empty_marks);
        rewrite_body(body, locals, marks).map_err(|message| LowerError {
            message: format!("closure {}: {message}", closure.id.0),
            span: None,
        })?;
    }
    Ok(())
}

/// The source bindings of each body that a read or a capture can reach while
/// they are unassigned.
///
/// A read of a binding outside `must` marks it. A capture whose source is
/// outside `must` at the construction marks the source and the closure's
/// target local: the two share one storage, which must be able to hold
/// nothing. A read of a temporary or a parameter outside `must` is a lowering
/// bug, not a source behaviour, and is left for the validator to report.
fn possibly_unbound_bindings(
    mir: &Mir,
    captures: &CaptureInit,
) -> HashMap<BodyOwner, HashSet<LocalId>> {
    let mut unbound: HashMap<BodyOwner, HashSet<LocalId>> = HashMap::new();
    for body in mir.bodies() {
        let analysis = DefiniteAssignment::of(body, captures.entry_defs(body));
        let mut marked: Vec<(BodyOwner, LocalId)> = Vec::new();
        analysis.walk(body, |point, state| {
            let mut mark_read = |local: LocalId| {
                let is_binding = body
                    .local_decl(local)
                    .is_some_and(|decl| matches!(decl.kind, LocalKind::UserBinding(_)));
                if is_binding && !state.must.contains(&local) {
                    marked.push((body.owner, local));
                }
            };
            match point {
                Point::Statement { statement, .. } => {
                    non_capture_reads(statement, &mut mark_read);
                    if let Rvalue::Closure { id, captures: operands } = statement.value() {
                        let targets = mir.closure(*id).map(|closure| closure.captures.as_slice());
                        for (index, operand) in operands.iter().enumerate() {
                            let Some(source) = operand.local() else {
                                continue;
                            };
                            if state.must.contains(&source) {
                                continue;
                            }
                            marked.push((body.owner, source));
                            if let Some(target) = targets
                                .and_then(|closure_captures| closure_captures.get(index))
                                .and_then(|capture| capture.target_local)
                            {
                                marked.push((BodyOwner::Closure(*id), target));
                            }
                        }
                    }
                }
                Point::Terminator { terminator, .. } => terminator.read_locals(&mut mark_read),
            }
        });
        for (owner, local) in marked {
            unbound.entry(owner).or_default().insert(local);
        }
    }
    share_with_captures(mir, &mut unbound);
    unbound
}

/// Marks the closure's copy of every marked binding a closure captures.
///
/// A capture shares the binding's storage, so once the binding is stored as
/// something that can be empty, the closure reads that same storage and must
/// read it through a binding read too, even where the capture itself was
/// assigned at the construction. A closure's copy can be captured again by a
/// closure nested in it, so this runs to a fixpoint.
fn share_with_captures(mir: &Mir, unbound: &mut HashMap<BodyOwner, HashSet<LocalId>>) {
    loop {
        let mut added: Vec<(BodyOwner, LocalId)> = Vec::new();
        for body in mir.bodies() {
            let Some(marked) = unbound.get(&body.owner) else {
                continue;
            };
            for statement in body.blocks.iter().flat_map(|block| &block.statements) {
                let Rvalue::Closure { id, captures } = statement.value() else {
                    continue;
                };
                let Some(closure) = mir.closure(*id) else {
                    continue;
                };
                for (operand, capture) in captures.iter().zip(&closure.captures) {
                    if let Some(source) = operand.local()
                        && marked.contains(&source)
                        && let Some(target) = capture.target_local
                        && !unbound.get(&BodyOwner::Closure(*id)).is_some_and(|set| set.contains(&target))
                    {
                        added.push((BodyOwner::Closure(*id), target));
                    }
                }
            }
        }
        if added.is_empty() {
            return;
        }
        for (owner, local) in added {
            unbound.entry(owner).or_default().insert(local);
        }
    }
}

/// The handler that replaces each `(catch block, binding)` handler catching
/// into a possibly-unbound binding (see [`land_exception`]).
type Landings = HashMap<(BlockId, LocalId), ExceptionHandler>;

/// One body's mutable parts and the facts its rewrite needs.
struct BodyParts<'a> {
    /// The body's locals.
    locals: &'a mut Vec<LocalDecl>,
    /// The body's blocks.
    blocks: &'a mut Vec<BasicBlock>,
    /// The entry block.
    entry: BlockId,
    /// The locals assigned when the body starts (see [`CaptureInit::entry_defs`]).
    entry_defs: Vec<LocalId>,
    /// The closure's captures; empty for a function.
    captures: &'a [MirClosureCapture],
    /// The language the body was written in.
    language: Language,
    /// Whether the body is a module's top level.
    module_body: bool,
    /// The `None` type a binding check's unit destination has.
    none_ty: smelt_hir::TypeId,
}

impl BodyParts<'_> {
    /// What an unassigned read of `local` raises in this body.
    fn unbound_read(&self, local: LocalId) -> UnboundRead {
        match self.language {
            Language::TypeScript => UnboundRead::JsTemporalDeadZone,
            Language::Python
                if self.captures.iter().any(|capture| capture.target_local == Some(local)) =>
            {
                UnboundRead::PyFreeVariable
            }
            Language::Python if self.module_body => UnboundRead::PyGlobal,
            Language::Python => UnboundRead::PyLocal,
        }
    }

    /// Declares a fresh temporary of `local`'s type.
    fn temp_like(&mut self, local: LocalId) -> Result<LocalId, String> {
        let decl = self
            .locals
            .get(local.index())
            .ok_or_else(|| format!("local {} is out of range", local.0))?;
        let temp = LocalDecl {
            ty: decl.ty,
            kind: LocalKind::Temp,
            span: decl.span,
            init: LocalInit::Definite,
        };
        let id = LocalId(u32::try_from(self.locals.len()).map_err(|error| format!("too many locals: {error}"))?);
        self.locals.push(temp);
        Ok(id)
    }

    /// Declares a fresh `None` temporary for the answer of a check of `local`,
    /// at `local`'s source position.
    fn unit_temp(&mut self, local: LocalId) -> Result<LocalId, String> {
        let span = self
            .locals
            .get(local.index())
            .ok_or_else(|| format!("local {} is out of range", local.0))?
            .span;
        let id = LocalId(u32::try_from(self.locals.len()).map_err(|error| format!("too many locals: {error}"))?);
        self.locals.push(LocalDecl { ty: self.none_ty, kind: LocalKind::Temp, span, init: LocalInit::Definite });
        Ok(id)
    }

    /// Appends an empty block with `span`'s source position and returns it.
    fn push_block(&mut self, like: BlockId) -> Result<BlockId, String> {
        let span = self
            .blocks
            .get(like.index())
            .ok_or_else(|| format!("block {} is out of range", like.0))?
            .span;
        let id = BlockId(u32::try_from(self.blocks.len()).map_err(|error| format!("too many blocks: {error}"))?);
        self.blocks.push(BasicBlock { id, statements: Vec::new(), terminator: None, span });
        Ok(id)
    }

    /// The block at `id`.
    fn block_mut(&mut self, id: BlockId) -> Result<&mut BasicBlock, String> {
        self.blocks.get_mut(id.index()).ok_or_else(|| format!("block {} is out of range", id.0))
    }
}

/// Rewrites one body so that each binding in `unbound` is stored, written and
/// read the way [`LocalInit::MaybeUnbound`] requires.
fn rewrite_body(
    mut body: BodyParts<'_>,
    unbound: &HashSet<LocalId>,
    marks: &HandlerMarks,
) -> Result<(), String> {
    route_writes_through_assign(&mut body, unbound)?;
    let sites = read_sites(&body, unbound);
    let mut by_block: HashMap<BlockId, Vec<ReadSite>> = HashMap::new();
    for site in sites {
        by_block.entry(site.block).or_default().push(site);
    }
    let mut blocks: Vec<BlockId> = by_block.keys().copied().collect();
    blocks.sort_unstable_by_key(|block| block.0);
    for block in blocks {
        let Some(mut block_sites) = by_block.remove(&block) else {
            continue;
        };
        // From the last position back, so a split never moves a site still to
        // be rewritten.
        block_sites.sort_by_key(|site| std::cmp::Reverse(site.position));
        for site in block_sites {
            rewrite_site(&mut body, &site, marks)?;
        }
    }
    for local in unbound {
        if let Some(decl) = body.locals.get_mut(local.index()) {
            decl.init = LocalInit::MaybeUnbound;
        }
    }
    Ok(())
}

/// Makes every write to an `unbound` binding a whole-local
/// [`Statement::Assign`].
///
/// * `binding = value` spelled as a place assignment becomes an `Assign`.
/// * A call or await that returns into the binding returns into a fresh
///   temporary instead, and a landing block on its normal edge assigns it.
/// * A handler that catches into the binding catches into a fresh temporary,
///   and a landing block between the throw and the catch block assigns it. The
///   landing block is shared by every edge into the same handler.
fn route_writes_through_assign(body: &mut BodyParts<'_>, unbound: &HashSet<LocalId>) -> Result<(), String> {
    let mut landings: Landings = HashMap::new();
    let block_count = body.blocks.len();
    for block_index in 0..block_count {
        let block_id = BlockId(u32::try_from(block_index).map_err(|error| format!("too many blocks: {error}"))?);
        let statement_count = body.block_mut(block_id)?.statements.len();
        for statement_index in 0..statement_count {
            let statement = body
                .block_mut(block_id)?
                .statements
                .get_mut(statement_index)
                .ok_or_else(|| "statement index out of range".to_owned())?;
            match statement {
                Statement::AssignPlace { place: Place::Local(local), value } if unbound.contains(local) => {
                    let dest = *local;
                    let assigned = std::mem::replace(value, Rvalue::Use(Operand::Const(crate::Constant::None)));
                    *statement = Statement::Assign { dest, value: assigned };
                }
                Statement::DictEntryUpdate { current, .. } if unbound.contains(current) => {
                    return Err(format!(
                        "binding {} is bound by a dict-entry update and can be read unassigned",
                        current.0
                    ));
                }
                Statement::Assign { .. } | Statement::AssignPlace { .. } | Statement::DictEntryUpdate { .. } => {}
            }
            let yield_handler = match body.block_mut(block_id)?.statements.get(statement_index).map(Statement::value) {
                Some(Rvalue::GeneratorYield { unwind: Some(handler), .. }) => Some(*handler),
                _ => None,
            };
            if let Some(handler) = yield_handler
                && let Some(landed) = land_exception(body, &mut landings, handler, unbound)?
                && let Some(Rvalue::GeneratorYield { unwind, .. }) = body
                    .block_mut(block_id)?
                    .statements
                    .get_mut(statement_index)
                    .map(Statement::value_mut)
            {
                *unwind = Some(landed);
            }
        }
        let terminator = body.block_mut(block_id)?.terminator.clone();
        let Some(Terminator::Call { dest, target, unwind, .. } | Terminator::Await { dest, target, unwind, .. }) =
            terminator
        else {
            continue;
        };
        let landed_unwind = match unwind {
            Some(handler) => land_exception(body, &mut landings, handler, unbound)?,
            None => None,
        };
        let (new_dest, new_target) = if unbound.contains(&dest) {
            let temp = body.temp_like(dest)?;
            let landing = body.push_block(block_id)?;
            let landing_block = body.block_mut(landing)?;
            landing_block.statements.push(Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Move(Place::Local(temp))),
            });
            landing_block.terminator = Some(Terminator::Goto(target));
            (Some(temp), Some(landing))
        } else {
            (None, None)
        };
        if let Some(
            Terminator::Call { dest: dest_slot, target: target_slot, unwind: unwind_slot, .. }
            | Terminator::Await { dest: dest_slot, target: target_slot, unwind: unwind_slot, .. },
        ) = &mut body.block_mut(block_id)?.terminator
        {
            if let Some(temp) = new_dest {
                *dest_slot = temp;
            }
            if let Some(landing) = new_target {
                *target_slot = landing;
            }
            if let Some(landed) = landed_unwind {
                *unwind_slot = Some(landed);
            }
        }
    }
    Ok(())
}

/// The handler to use instead of `handler` when it catches into an `unbound`
/// binding: one that catches into a temporary and lands in a block assigning
/// the binding. `None` when `handler` needs no change.
fn land_exception(
    body: &mut BodyParts<'_>,
    landings: &mut Landings,
    handler: ExceptionHandler,
    unbound: &HashSet<LocalId>,
) -> Result<Option<ExceptionHandler>, String> {
    let Some(local) = handler.exception_local.filter(|local| unbound.contains(local)) else {
        return Ok(None);
    };
    if let Some(landed) = landings.get(&(handler.catch_block, local)) {
        return Ok(Some(*landed));
    }
    let temp = body.temp_like(local)?;
    let landing = body.push_block(handler.catch_block)?;
    let landing_block = body.block_mut(landing)?;
    landing_block.statements.push(Statement::Assign {
        dest: local,
        value: Rvalue::Use(Operand::Move(Place::Local(temp))),
    });
    landing_block.terminator = Some(Terminator::Goto(handler.catch_block));
    let landed = ExceptionHandler { catch_block: landing, exception_local: Some(temp) };
    landings.insert((handler.catch_block, local), landed);
    Ok(Some(landed))
}

/// One statement or terminator that reads `unbound` bindings that are not
/// definitely assigned there.
#[derive(Debug, Clone)]
struct ReadSite {
    /// The block.
    block: BlockId,
    /// The statement's index, or the statement count for the terminator.
    position: usize,
    /// Each binding the site reads unproven, in id order.
    unproven: Vec<LocalId>,
}

/// Every point that reads an `unbound` binding the analysis cannot prove
/// assigned there. A read it can prove needs nothing: it is an ordinary read.
fn read_sites(body: &BodyParts<'_>, unbound: &HashSet<LocalId>) -> Vec<ReadSite> {
    let blocks: &[BasicBlock] = body.blocks;
    let analysis = DefiniteAssignment::of_blocks(blocks, body.entry, body.entry_defs.iter().copied());
    let mut sites = Vec::new();
    analysis.walk_blocks(blocks, |point, state| {
        let mut read: Vec<LocalId> = Vec::new();
        let (block, position) = match point {
            Point::Statement { block, index, statement } => {
                non_capture_reads(statement, |local| read.push(local));
                (block, index)
            }
            Point::Terminator { block, terminator } => {
                terminator.read_locals(|local| read.push(local));
                let count = blocks.get(block.index()).map_or(0, |terminated| terminated.statements.len());
                (block, count)
            }
        };
        read.retain(|local| unbound.contains(local) && !state.must.contains(local));
        read.sort_unstable_by_key(|local| local.0);
        read.dedup();
        if !read.is_empty() {
            sites.push(ReadSite { block, position, unproven: read });
        }
    });
    sites
}

/// Puts a [`Rvalue::BindingCheck`] before one read site for each binding it
/// reads unproven. Each check is a call terminator whose unwind edge is the
/// handler lowering recorded for the site, so the block is split there: the
/// statements before the site stay, each check continues into the next block,
/// and the last one holds the site and the rest of the original block.
fn rewrite_site(body: &mut BodyParts<'_>, site: &ReadSite, marks: &HandlerMarks) -> Result<(), String> {
    let block = body.block_mut(site.block)?;
    let is_terminator = site.position >= block.statements.len();
    let handler = match (&block.terminator, is_terminator) {
        (Some(Terminator::Call { unwind, .. } | Terminator::Await { unwind, .. }), true) => {
            unwind.or_else(|| marks.at(site.block, site.position))
        }
        _ => marks.at(site.block, site.position),
    };
    let rest: Vec<Statement> = block.statements.split_off(site.position.min(block.statements.len()));
    let terminator = block.terminator.take();
    let mut current = site.block;
    for local in &site.unproven {
        let next = body.push_block(site.block)?;
        let checked = body.unit_temp(*local)?;
        let unbound = body.unbound_read(*local);
        body.block_mut(current)?.terminator = Some(Terminator::Call {
            callee: Callee::Op(
                Box::new(Rvalue::BindingCheck { binding: Operand::Copy(Place::Local(*local)), unbound }),
                None,
            ),
            args: Vec::new(),
            dest: checked,
            target: next,
            unwind: handler,
        });
        current = next;
    }
    let tail = body.block_mut(current)?;
    tail.statements.extend(rest);
    tail.terminator = terminator;
    Ok(())
}

#[cfg(test)]
mod tests;
