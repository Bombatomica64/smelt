//! Definite assignment over one body's control-flow graph.
//!
//! One forward dataflow, shared by the pass that decides which source bindings
//! can be read unassigned (`lower::passes::unbound_bindings`) and by the
//! validator that enforces its result (`validate::assignment`), so the two can
//! never disagree about what "assigned here" means.
//!
//! For every reachable block it computes, on entry, two sets of locals:
//!
//! * `must`: assigned on EVERY path into the block (the meet is intersection);
//! * `may`: assigned on SOME path into the block (the meet is union).
//!
//! A read of a local in `must` is definitely assigned; one in `may` but not in
//! `must` is possibly unassigned; one in neither is definitely unassigned
//! ([`ReadState`]).
//!
//! # Edges
//!
//! Facts meet per edge ([`Terminator::edges`]): a call's destination is
//! assigned on its normal edge only, a handler's exception local on its unwind
//! edge only. A `yield` (`Rvalue::GeneratorYield`) is a second kind of edge, out
//! of the middle of a block: the generator can be resumed with `throw` (into the
//! yield's catch block, assigning its exception local) or with `return` (into
//! the enclosing `finally`, whose region the emitter renders from
//! `cleanup.block` up to `cleanup.after`). Both are edges here, so blocks only a
//! generator command reaches are analysed like any other.
//!
//! # A completed read leaves its local assigned
//!
//! After a statement reads a local, that local is in `must` and `may` for the
//! rest of the path. For a `Rvalue::BindingCheck` that is the semantics: the
//! check raised if the binding was unassigned, so the code after it only runs
//! when it was not. (The check is a call terminator, and its local is assigned
//! on the normal edge only.) For every other read it is merely a way to report
//! one violation per path instead of one per read. A closure's capture operands
//! are NOT reads: capturing a binding shares its storage, and the binding may
//! well be assigned only later (a recursive closure captures itself).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    BasicBlock, BlockId, ClosureId, LocalId, Mir, MirBody, Operand, Place, Read, Rvalue, Statement,
    Terminator,
};

/// What is known assigned at one program point.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Assigned {
    /// Locals assigned on every path that reaches the point.
    pub must: HashSet<LocalId>,
    /// Locals assigned on at least one path that reaches the point.
    pub may: HashSet<LocalId>,
}

/// How a read of one local relates to the paths that reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    /// Every path into the read assigns the local first.
    Assigned,
    /// Some path does and some path does not.
    MaybeUnassigned,
    /// No path does: the read always finds the local unassigned.
    Unassigned,
}

impl Assigned {
    /// The facts on entry: exactly `defs` are assigned.
    fn entry(defs: impl IntoIterator<Item = LocalId>) -> Self {
        let must: HashSet<LocalId> = defs.into_iter().collect();
        Self {
            may: must.clone(),
            must,
        }
    }

    /// How a read of `local` at this point relates to its assignments.
    #[must_use]
    pub fn state_of(&self, local: LocalId) -> ReadState {
        if self.must.contains(&local) {
            ReadState::Assigned
        } else if self.may.contains(&local) {
            ReadState::MaybeUnassigned
        } else {
            ReadState::Unassigned
        }
    }

    /// Records that `local` is assigned from this point on.
    pub(crate) fn insert(&mut self, local: LocalId) {
        self.must.insert(local);
        self.may.insert(local);
    }

    /// Meets the facts of one more incoming edge into `self`; answers whether
    /// `self` changed.
    fn meet(&mut self, incoming: &Self) -> bool {
        let must_before = self.must.len();
        let may_before = self.may.len();
        self.must.retain(|local| incoming.must.contains(local));
        self.may.extend(incoming.may.iter().copied());
        self.must.len() != must_before || self.may.len() != may_before
    }
}

/// One program point of a reachable block, in execution order.
#[derive(Debug, Clone, Copy)]
pub enum Point<'a> {
    /// The statement at `index` of `block`.
    Statement {
        /// The block.
        block: BlockId,
        /// The statement's position in the block.
        index: usize,
        /// The statement.
        statement: &'a Statement,
    },
    /// The terminator of `block`.
    Terminator {
        /// The block.
        block: BlockId,
        /// The terminator.
        terminator: &'a Terminator,
    },
}

/// The definite-assignment facts of one body: the [`Assigned`] sets on entry
/// to every block, `None` for a block no edge reaches.
#[derive(Debug, Clone)]
pub struct DefiniteAssignment {
    /// Facts on entry, indexed by block position.
    entry: Vec<Option<Assigned>>,
}

impl DefiniteAssignment {
    /// Runs the forward fixpoint over `body`, with exactly `entry_defs`
    /// assigned when the entry block starts.
    #[must_use]
    pub fn of(body: MirBody<'_>, entry_defs: impl IntoIterator<Item = LocalId>) -> Self {
        Self::of_blocks(body.blocks, body.entry, entry_defs)
    }

    /// [`Self::of`] over a body's blocks and entry block.
    #[must_use]
    pub fn of_blocks(
        blocks: &[BasicBlock],
        entry_block: BlockId,
        entry_defs: impl IntoIterator<Item = LocalId>,
    ) -> Self {
        let mut entry: Vec<Option<Assigned>> = vec![None; blocks.len()];
        let Some(slot) = entry.get_mut(entry_block.index()) else {
            return Self { entry };
        };
        *slot = Some(Assigned::entry(entry_defs));
        let mut queue = VecDeque::from([entry_block]);
        let mut queued: HashSet<BlockId> = HashSet::from([entry_block]);
        while let Some(block_id) = queue.pop_front() {
            queued.remove(&block_id);
            let Some(block) = blocks.get(block_id.index()) else {
                continue;
            };
            let Some(Some(state)) = entry.get(block_id.index()).cloned() else {
                continue;
            };
            step_block(block, state, &mut |_, _| {}, &mut |target, incoming| {
                let Some(target_entry) = entry.get_mut(target.index()) else {
                    return;
                };
                let changed = if let Some(existing) = target_entry {
                    existing.meet(&incoming)
                } else {
                    *target_entry = Some(incoming);
                    true
                };
                if changed && queued.insert(target) {
                    queue.push_back(target);
                }
            });
        }
        Self { entry }
    }

    /// Whether any edge reaches `block` from the entry.
    #[must_use]
    pub fn is_reachable(&self, block: BlockId) -> bool {
        matches!(self.entry.get(block.index()), Some(Some(_)))
    }

    /// Visits every point of every reachable block, in block order and then
    /// execution order, with the facts that hold just BEFORE the point runs.
    pub fn walk<'a>(&self, body: MirBody<'a>, visit: impl FnMut(Point<'a>, &Assigned)) {
        self.walk_blocks(body.blocks, visit);
    }

    /// [`Self::walk`] over a body's blocks.
    pub fn walk_blocks<'a>(
        &self,
        blocks: &'a [BasicBlock],
        mut visit: impl FnMut(Point<'a>, &Assigned),
    ) {
        for block in blocks {
            let Some(Some(state)) = self.entry.get(block.id.index()).cloned() else {
                continue;
            };
            step_block(block, state, &mut visit, &mut |_, _| {});
        }
    }
}

/// One generator-command edge out of a `yield`: its target and the local it
/// assigns.
pub(crate) type YieldEdge = (BlockId, Option<LocalId>);

/// Visits each point of a block with the facts before it.
type PointVisitor<'v, 'a> = dyn FnMut(Point<'a>, &Assigned) + 'v;

/// Receives the facts each outgoing edge of a block carries.
type EdgeSink<'v> = dyn FnMut(BlockId, Assigned) + 'v;

/// The generator-command edges out of `statement`: none unless it is a
/// `yield`, else its catch block (assigning the exception local) and its
/// enclosing `finally` region (entry and continuation), each with the local
/// it assigns.
pub(crate) fn yield_edges(statement: &Statement) -> Vec<YieldEdge> {
    let Rvalue::GeneratorYield {
        unwind, cleanup, ..
    } = statement.value()
    else {
        return Vec::new();
    };
    let mut edges = Vec::new();
    if let Some(handler) = unwind {
        edges.push((handler.catch_block, handler.exception_local));
    }
    if let Some(finally) = cleanup {
        edges.push((finally.block, None));
        edges.push((finally.after, None));
    }
    edges
}

/// Runs one block from `state`: reports each point with the facts before it,
/// then hands every outgoing edge its facts.
fn step_block<'a>(
    block: &'a BasicBlock,
    mut state: Assigned,
    visit: &mut PointVisitor<'_, 'a>,
    edge: &mut EdgeSink<'_>,
) {
    for (index, statement) in block.statements.iter().enumerate() {
        visit(
            Point::Statement {
                block: block.id,
                index,
                statement,
            },
            &state,
        );
        let mut after_reads = state.clone();
        non_capture_reads(statement, |local| after_reads.insert(local));
        for (target, defines) in yield_edges(statement) {
            let mut incoming = after_reads.clone();
            if let Some(local) = defines {
                incoming.insert(local);
            }
            edge(target, incoming);
        }
        state = after_reads;
        if let Some(local) = statement.defs() {
            state.insert(local);
        }
    }
    let Some(terminator) = &block.terminator else {
        return;
    };
    visit(
        Point::Terminator {
            block: block.id,
            terminator,
        },
        &state,
    );
    let mut after_reads = state.clone();
    terminator.read_locals(|local| after_reads.insert(local));
    match terminator {
        // The unwind edge leaves before the call completes: its reads may be
        // the very thing that raised (a checked binding read), so only the
        // normal edge counts them.
        Terminator::Call {
            dest,
            target,
            unwind,
            ..
        }
        | Terminator::Await {
            dest,
            target,
            unwind,
            ..
        } => {
            if let Some(handler) = unwind {
                let mut incoming = state.clone();
                if let Some(local) = handler.exception_local {
                    incoming.insert(local);
                }
                edge(handler.catch_block, incoming);
            }
            after_reads.insert(*dest);
            edge(*target, after_reads);
        }
        Terminator::Goto(_)
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_)
        | Terminator::Throw(_) => {
            for successor in terminator.edges() {
                let mut incoming = after_reads.clone();
                if let Some(local) = successor.defines {
                    incoming.insert(local);
                }
                edge(successor.target, incoming);
            }
        }
    }
}

/// Visits every local `statement` reads, except a closure's capture operands,
/// which share a binding's storage rather than read its value.
pub fn non_capture_reads(statement: &Statement, mut visit: impl FnMut(LocalId)) {
    let captures = matches!(statement.value(), Rvalue::Closure { .. });
    match statement {
        Statement::Assign { value, .. } => {
            if !captures {
                value.read_locals(&mut visit);
            }
        }
        Statement::AssignPlace { place, value } => {
            match place {
                Place::Local(_) => {}
                Place::Field { base, .. } => visit(*base),
                Place::Index { base, index, .. } => {
                    visit(*base);
                    index.read_locals(&mut visit);
                }
                Place::Global { projection, .. } => {
                    if let crate::GlobalProjection::Index { index, .. } = projection {
                        index.read_locals(&mut visit);
                    }
                }
            }
            if !captures {
                value.read_locals(&mut visit);
            }
        }
        Statement::DictEntryUpdate { .. } => {
            statement.reads(|read| match read {
                Read::Base(local) => visit(local),
                Read::Operand(operand) => operand.read_locals(&mut visit),
            });
        }
    }
}

/// For every closure capture, whether the captured binding is assigned
/// whenever the closure's body can run.
///
/// A capture is *initialized* when its source local is definitely assigned at
/// the closure's construction, or when the construction provably cannot run
/// before the source is assigned: every read of the constructed closure value
/// happens where the source is already assigned, or is the assignment of that
/// value to the source itself. That second rule is what a recursive closure
/// needs (`const walk = (n) => walk(n - 1)` captures `walk` while building the
/// value `walk` is then assigned), and it is the only way a closure value can
/// be invoked: through a read of the local holding it.
///
/// The facts of a closure body depend on the body that constructs it, so the
/// bodies are analysed in construction order: every function first, then the
/// closures by id. Lowering numbers a closure before any closure nested in it,
/// so a constructing body is always analysed before the closures it builds; a
/// capture whose constructing body comes later is conservatively uninitialized.
/// A closure that is never constructed, or whose construction no edge reaches,
/// never runs, so its captures are vacuously initialized.
#[derive(Debug, Clone, Default)]
pub struct CaptureInit {
    /// Per closure id, per capture index, whether the capture is initialized.
    initialized: HashMap<ClosureId, Vec<bool>>,
}

impl CaptureInit {
    /// Computes the facts of every capture in `mir`.
    #[must_use]
    pub fn of(mir: &Mir) -> Self {
        let sites = mir.closure_sites();
        let mut facts = Self::default();
        let mut analysed: HashSet<crate::BodyOwner> = HashSet::new();
        for body in mir.bodies() {
            if let crate::BodyOwner::Closure(id) = body.owner
                && !facts.initialized.contains_key(&id)
            {
                // Not recorded: no reachable construction, unless the body that
                // constructs it has not been analysed yet.
                let constructed_later = match sites.get(id.index()) {
                    Some(crate::SiteLookup::One(site)) => !analysed.contains(&site.owner),
                    Some(crate::SiteLookup::Many) => true,
                    Some(crate::SiteLookup::Unconstructed) | None => false,
                };
                facts
                    .initialized
                    .insert(id, vec![!constructed_later; body.captures.len()]);
            }
            let analysis = DefiniteAssignment::of(body, facts.entry_defs(body));
            facts.record_sites(body, &analysis);
            analysed.insert(body.owner);
        }
        facts
    }

    /// Whether capture `index` of closure `id` is initialized.
    #[must_use]
    pub fn is_initialized(&self, id: ClosureId, index: usize) -> bool {
        self.initialized
            .get(&id)
            .and_then(|captures| captures.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// The locals assigned when `body` starts: its parameters and, in a
    /// closure, the target of every initialized capture.
    #[must_use]
    pub fn entry_defs(&self, body: MirBody<'_>) -> Vec<LocalId> {
        let mut defs = body.params.to_vec();
        if let crate::BodyOwner::Closure(id) = body.owner {
            defs.extend(
                body.captures
                    .iter()
                    .enumerate()
                    .filter_map(|(index, capture)| {
                        capture
                            .target_local
                            .filter(|_| self.is_initialized(id, index))
                    }),
            );
        }
        defs
    }

    /// Records the facts of every closure `body` constructs.
    fn record_sites(&mut self, body: MirBody<'_>, analysis: &DefiniteAssignment) {
        // A construction whose captures are all assigned needs nothing more.
        let mut pending: Vec<PendingSite> = Vec::new();
        analysis.walk(body, |point, state| {
            let Point::Statement { statement, .. } = point else {
                return;
            };
            let (Statement::Assign {
                dest,
                value: Rvalue::Closure { id, captures },
            }
            | Statement::AssignPlace {
                place: Place::Local(dest),
                value: Rvalue::Closure { id, captures },
            }) = statement
            else {
                return;
            };
            let sources: Vec<Option<LocalId>> = captures.iter().map(Operand::local).collect();
            let assigned: Vec<bool> = sources
                .iter()
                .map(|source| source.is_some_and(|local| state.must.contains(&local)))
                .collect();
            pending.push(PendingSite {
                id: *id,
                value: *dest,
                sources,
                assigned,
            });
        });
        for site in &mut pending {
            for (index, assigned) in site.assigned.iter_mut().enumerate() {
                if *assigned {
                    continue;
                }
                let Some(Some(source)) = site.sources.get(index).copied() else {
                    continue;
                };
                *assigned = value_unobserved_until_assigned(body, analysis, site.value, source);
            }
            // A closure analysed before this construction was seen kept the
            // conservative verdict it was analysed with; the two are merged so
            // every later query agrees with what that analysis assumed.
            let verdicts = std::mem::take(&mut site.assigned);
            match self.initialized.get_mut(&site.id) {
                Some(existing) => {
                    for (kept, verdict) in existing.iter_mut().zip(verdicts) {
                        *kept = *kept && verdict;
                    }
                }
                None => {
                    self.initialized.insert(site.id, verdicts);
                }
            }
        }
    }
}

/// One closure construction while its facts are computed.
struct PendingSite {
    /// The closure.
    id: ClosureId,
    /// The local the constructed value is assigned to.
    value: LocalId,
    /// Each capture's source local.
    sources: Vec<Option<LocalId>>,
    /// Each capture's verdict so far: assigned at the construction.
    assigned: Vec<bool>,
}

/// Whether the closure value held in `value` cannot be observed (and so not
/// invoked) anywhere `source` is still unassigned.
///
/// The value may be copied into other locals first (`x = f; source = x`), so
/// the rule follows its *aliases*: `value` and every local a plain copy of an
/// alias assigns. Every read of an alias, capture operands included, must then
/// happen where `source` is assigned, or be a plain copy into another alias or
/// into `source` itself: copying the value is not calling it, and the copy is
/// held to the same rule. A read of any other shape (a call, an argument, a
/// field store) is where the closure could run or escape.
///
/// When `value` IS `source` (the construction assigns the binding directly),
/// every read of the binding comes after its assignment by definition.
fn value_unobserved_until_assigned(
    body: MirBody<'_>,
    analysis: &DefiniteAssignment,
    value: LocalId,
    source: LocalId,
) -> bool {
    if value == source {
        return true;
    }
    let aliases = copies_of(body, value, source);
    let mut observed_unassigned = false;
    analysis.walk(body, |point, state| {
        if observed_unassigned || state.must.contains(&source) {
            return;
        }
        match point {
            Point::Statement { statement, .. } => {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Use(operand),
                } = statement
                    && (*dest == source || aliases.contains(dest))
                    && operand
                        .local()
                        .is_some_and(|local| aliases.contains(&local))
                {
                    return;
                }
                statement.read_locals(|local| observed_unassigned |= aliases.contains(&local));
            }
            Point::Terminator { terminator, .. } => {
                terminator.read_locals(|local| observed_unassigned |= aliases.contains(&local));
            }
        }
    });
    !observed_unassigned
        && aliases
            .iter()
            .all(|alias| analysis_reads_only_reachable(body, analysis, *alias))
}

/// `value` and every local a chain of plain copies (`x = copy value`) from it
/// assigns, anywhere in the body, except `source`: once the value reaches the
/// binding the closure captured, the binding is assigned, which is the point.
fn copies_of(body: MirBody<'_>, value: LocalId, source: LocalId) -> HashSet<LocalId> {
    let mut aliases = HashSet::from([value]);
    loop {
        let before = aliases.len();
        for statement in body.blocks.iter().flat_map(|block| &block.statements) {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(operand),
            } = statement
                && *dest != source
                && operand
                    .local()
                    .is_some_and(|local| aliases.contains(&local))
            {
                aliases.insert(*dest);
            }
        }
        if aliases.len() == before {
            return aliases;
        }
    }
}

/// Whether every read of `value` is in a block the analysis reached: a read in
/// a block it did not reach was not checked by the walk, so the proof only
/// holds when there is none.
fn analysis_reads_only_reachable(
    body: MirBody<'_>,
    analysis: &DefiniteAssignment,
    value: LocalId,
) -> bool {
    body.blocks.iter().all(|block| {
        analysis.is_reachable(block.id)
            || (!block
                .statements
                .iter()
                .any(|statement| statement.reads_local(value))
                && !block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator.reads_local(value)))
    })
}
