//! Escape analysis for list-typed MIR locals.
//!
//! # Why this exists
//!
//! `Type::List` lowers to `SmeltList<T>`, which is `{ id: usize, values:
//! Rc<RefCell<Vec<T>>> }`. That shape costs **two** heap allocations per list
//! — the `RcBox` (strong/weak counts, borrow flag, `Vec` header) plus the
//! element buffer — where a plain `Vec<T>` costs one. Callgrind on the
//! es-toolkit benchmark corpus puts `malloc`/`free` at roughly a third of the
//! run time in the array-heavy cases, so halving the allocation count for
//! lists that provably do not need a shared buffer is worth pricing.
//!
//! This module does not change codegen. It **measures the population**: how
//! many list-typed locals could, in principle, be lowered to a plain `Vec<T>`
//! (a hypothetical "tiered" representation), and how many could not. Any
//! decision about actually building that tiering should be made from these
//! numbers, not from intuition.
//!
//! # The three classes
//!
//! Every list-typed local in a body ([`MirFunction`] or [`MirClosure`]) is put
//! in exactly one class:
//!
//! * [`ListLocalClass::Escaping`] — a handle on this list's buffer can be
//!   observed from outside the local's own frame, so the buffer must stay
//!   shared.
//! * [`ListLocalClass::Aliased`] — not escaping, but more than one local in the
//!   same frame names the same buffer, so a write through one name must be
//!   visible through the other. A plain `Vec<T>` cannot express that without
//!   the codegen also proving one of the names dead.
//! * [`ListLocalClass::LocalImmutable`] / [`ListLocalClass::LocalMutated`] — neither. Exactly one local names the buffer
//!   for its whole lifetime and nothing outside the frame can see it. This is
//!   the tierable population, reported split by whether the list is ever
//!   mutated in place.
//!
//! # Soundness direction
//!
//! **When the analysis cannot prove a list is `Local`, it says `Escaping`.**
//! A wrong `Local` would later produce generated Rust where a mutation is
//! silently lost — far worse than a missed optimization — so every unmodelled
//! construct falls to `Escaping`.
//!
//! Concretely, the analysis is a whitelist on two axes, and *anything* outside
//! the whitelist escapes:
//!
//! 1. **Definitions.** A list local's group is `Escaping` unless every
//!    definition of every member mints a fresh, unshared buffer (see
//!    [`rvalue_mints_fresh_list`]) or is a recorded alias edge.
//! 2. **Uses.** Every read of a list local is assigned an [`OperandRole`] by
//!    [`operand_roles`], which matches on the `Rvalue` variant. The match ends
//!    in a wildcard arm that gives every operand [`OperandRole::Escapes`], so a
//!    newly added `Rvalue` variant degrades to "escaping" rather than to a
//!    silently wrong `Local`.
//!
//! ## Escaping because it genuinely escapes
//!
//! These constructs really do publish the buffer, and no amount of extra
//! analysis precision would change the verdict:
//!
//! * returned ([`Terminator::Return`]) or thrown ([`Terminator::Throw`]);
//! * passed as a call argument ([`Terminator::Call`], [`Rvalue::ClosureCall`],
//!   [`Rvalue::ClosureCallSpread`], [`Rvalue::ExternalClassInstance`],
//!   [`Rvalue::HostConstruct`], [`Rvalue::AsyncOp`], `await`);
//! * the **receiver** of [`Rvalue::ListCallback`] (`map`/`filter`/`forEach`/…)
//!   and [`Rvalue::ListReduce`] — JavaScript passes the array itself to the
//!   callback (`cb(item, index, array)`), and codegen emits that argument, so
//!   the callback can retain a handle. A comparator (`sort`) and
//!   `Array.from({length}, cb)` do not receive the array and stay read-only;
//! * captured by a closure ([`Rvalue::Closure`] captures);
//! * stored into a container — a list/set/dict/tuple/struct literal, a
//!   `push`/`unshift`/`insert`/`fill`/`with` element position, `DictSet`'s
//!   value, `SetAdd`'s item, or any [`Statement::AssignPlace`] whose place is a
//!   field or index projection;
//! * erased to `SmeltUnknown` ([`Rvalue::UnknownCast`], [`Rvalue::BoxPrimitive`]);
//! * written to a module global ([`Rvalue::GlobalSet`]);
//! * transitively: unified with any of the above through an alias edge.
//!
//! ## Escaping only because the analysis cannot prove otherwise
//!
//! These are *conservative* verdicts. A more precise analysis could recover
//! some of them; they are reported under
//! [`EscapeReason::UnprovenDefinition`] / [`EscapeReason::UnmodelledUse`] so
//! the count of "genuinely escaping" is not inflated by them:
//!
//! * **parameters** — a caller-supplied list may be aliased anywhere; proving
//!   otherwise needs interprocedural analysis this pass does not do;
//! * **call and `await` results** — the callee may have kept a handle;
//! * **container reads** — a list read out of a field, index, `DictGet`,
//!   `ListPop`/`ListShift`/`ListNext`, or a `find`-shaped search is by
//!   construction a handle on a buffer that also lives inside the container;
//! * **`Phi` destinations** and copy destinations — these are recorded as alias
//!   edges, so the *group* survives, but a group whose members are merged by a
//!   phi from an unproven source escapes with the source;
//! * **closure capture targets** — the local inside a closure body that
//!   receives a capture is treated exactly like a parameter;
//! * **globals read back** ([`Rvalue::GlobalGet`]) and any other rvalue not in
//!   [`rvalue_mints_fresh_list`];
//! * **any operand of an `Rvalue` variant not enumerated in
//!   [`operand_roles`]** — the wildcard arm.
//!
//! # Mutation
//!
//! A `Local` list is reported as mutated when at least one of these reaches it
//! in a receiver position: [`Rvalue::ListPush`], [`Rvalue::ListExtend`],
//! [`Rvalue::ListInsert`], [`Rvalue::ListUnshift`], [`Rvalue::ListReverse`],
//! [`Rvalue::ListClear`], [`Rvalue::ListSort`], [`Rvalue::ListPop`],
//! [`Rvalue::ListShift`], [`Rvalue::ListRemove`], [`Rvalue::ListFill`],
//! [`Rvalue::ListCopyWithin`], [`Rvalue::ListSplice`] with `mutate: true`,
//! [`Rvalue::ListNext`] (it advances the iteration cursor), and
//! `AssignPlace { place: Place::Index { .. } }` (`arr[i] = v`).
//!
//! An immutable `Local` list is the cheapest possible win — it needs neither a
//! shared buffer nor interior mutability. A mutated `Local` list still only
//! needs a plain `Vec<T>` plus `&mut` access.

mod summary;

use std::collections::HashMap;

use smelt_hir::{Type, TypeId};

use self::summary::{ArgumentEffect, CallSummaries, FunctionSummary};
use crate::{
    Callee, ClosureId, FuncId, GlobalProjection, LocalId, LocalKind, Mir, MirClosure,
    MirFunction, Operand, Place,
    Rvalue, Statement, Terminator,
};

/// The class assigned to one list-typed local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ListLocalClass {
    /// A handle on the buffer can be observed outside this frame.
    Escaping,
    /// Confined to this frame, but more than one local names the buffer.
    Aliased,
    /// Confined to this frame and named by exactly one local, never mutated.
    LocalImmutable,
    /// Confined to this frame and named by exactly one local, mutated in place.
    LocalMutated,
}

impl ListLocalClass {
    /// A short stable label used in report tables.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Escaping => "escaping",
            Self::Aliased => "aliased",
            Self::LocalImmutable => "local-immutable",
            Self::LocalMutated => "local-mutated",
        }
    }
}

/// Why a list local was classified [`ListLocalClass::Escaping`].
///
/// The first four variants are genuine escapes. The last two are conservative:
/// the analysis could not prove confinement, so it assumed the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EscapeReason {
    /// Returned from, or thrown out of, the body.
    Returned,
    /// Passed as an argument to a call, an `await`, or a constructor.
    CallArgument,
    /// Captured by a closure environment.
    Captured,
    /// Stored into a container, a struct field, an index, or a global.
    StoredInContainer,
    /// Erased to `SmeltUnknown`.
    Erased,
    /// At least one definition in the group could not be proven to mint a
    /// fresh, unshared buffer (parameter, call result, container read, ...).
    UnprovenDefinition,
    /// Read by an `Rvalue` variant the role table does not model.
    UnmodelledUse,
}

impl EscapeReason {
    /// A short stable label used in report tables.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Returned => "returned",
            Self::CallArgument => "call-argument",
            Self::Captured => "closure-capture",
            Self::StoredInContainer => "stored-in-container",
            Self::Erased => "erased-to-unknown",
            Self::UnprovenDefinition => "unproven-definition",
            Self::UnmodelledUse => "unmodelled-use",
        }
    }

    /// Whether this reason is a real escape rather than a conservative guess.
    #[must_use]
    pub const fn is_genuine(self) -> bool {
        matches!(
            self,
            Self::Returned
                | Self::CallArgument
                | Self::Captured
                | Self::StoredInContainer
                | Self::Erased
        )
    }
}

/// The classification of one list-typed local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListLocalFact {
    /// The classified local.
    pub local: LocalId,
    /// The buffer group this local belongs to, named by its lowest-numbered
    /// member.
    ///
    /// Locals in one group name one buffer, so they always share a class.
    /// Counting *groups* rather than locals is what turns the report into a
    /// count of allocation sites: lowering routes a single source array through
    /// a chain of moved temporaries, and each of those is a separate `LocalId`
    /// for one `Rc<RefCell<Vec<T>>>`.
    pub group: LocalId,
    /// Source name when the local came from a user binding or named parameter.
    pub name: Option<String>,
    /// The class this local landed in.
    pub class: ListLocalClass,
    /// Why it escapes, when it does. `None` for `Aliased` and `Local` lists.
    pub reason: Option<EscapeReason>,
    /// Whether the list is mutated in place anywhere in the body.
    pub mutated: bool,
}

/// Which MIR body a set of facts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKey {
    /// A top-level or member function.
    Function(FuncId),
    /// A closure body.
    Closure(ClosureId),
}

/// The list-escape facts for one MIR body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyListEscape {
    /// Which body these facts describe.
    pub key: BodyKey,
    /// A human-readable body name for reports.
    pub name: String,
    /// Whether this body is emitted as a native Rust test.
    ///
    /// Reports use this to separate the product population from the test
    /// population: a confined list inside a `#[test]` is not code any runtime
    /// optimization would speed up. Closure bodies are always `false` because
    /// [`MirClosure`] does not record which function it was lowered from.
    pub is_test: bool,
    /// One fact per list-typed local that is defined or read in this body.
    pub locals: Vec<ListLocalFact>,
}

impl BodyListEscape {
    /// Number of locals in this body that landed in `class`.
    #[must_use]
    pub fn count(&self, class: ListLocalClass) -> usize {
        self.locals.iter().filter(|fact| fact.class == class).count()
    }

    /// Number of locals in this body that are confined to the frame.
    #[must_use]
    pub fn local_count(&self) -> usize {
        self.count(ListLocalClass::LocalImmutable)
            .saturating_add(self.count(ListLocalClass::LocalMutated))
    }
}

/// Classify every list-typed local in every function and closure of `mir`.
///
/// Bodies with no list-typed locals are omitted. Results are ordered by body
/// (functions first, then closures) and by `LocalId` inside each body, so
/// reports diff cleanly between runs.
#[must_use]
pub fn analyze_list_escapes(mir: &Mir) -> Vec<BodyListEscape> {
    let summaries = CallSummaries::compute(mir);
    let mut out = Vec::new();
    for function in &mir.functions {
        let body = FunctionBody::from_function(function);
        let facts = analyze_body(mir, &summaries, &body);
        if !facts.is_empty() {
            out.push(BodyListEscape {
                key: BodyKey::Function(function.id),
                is_test: function.is_test,
                name: mir
                    .symbols
                    .get(function.name)
                    .unwrap_or("<unknown>")
                    .to_owned(),
                locals: facts,
            });
        }
    }
    for closure in &mir.closures {
        let body = FunctionBody::from_closure(closure);
        let facts = analyze_body(mir, &summaries, &body);
        if !facts.is_empty() {
            out.push(BodyListEscape {
                key: BodyKey::Closure(closure.id),
                is_test: false,
                name: format!("<closure #{}>", closure.id.0),
                locals: facts,
            });
        }
    }
    out
}

/// The parts of a function or closure body the analysis needs.
///
/// Functions and closures carry the same locals/blocks shape but are distinct
/// types; this view lets one walk serve both. `entry_locals` holds the locals
/// that arrive already-defined from outside the frame (parameters, and for a
/// closure the capture targets), which the analysis treats as unproven
/// definitions.
struct FunctionBody<'mir> {
    /// Local declarations, indexed by [`LocalId`].
    locals: &'mir [crate::LocalDecl],
    /// The body's basic blocks.
    blocks: &'mir [crate::BasicBlock],
    /// Locals bound before the first block runs.
    entry_locals: Vec<LocalId>,
}

impl<'mir> FunctionBody<'mir> {
    /// View a [`MirFunction`]: its parameters are the externally-defined locals.
    fn from_function(function: &'mir MirFunction) -> Self {
        Self {
            locals: &function.locals,
            blocks: &function.blocks,
            entry_locals: function.params.clone(),
        }
    }

    /// View a [`MirClosure`]: parameters *and* capture targets arrive from
    /// outside, so both count as externally defined.
    fn from_closure(closure: &'mir MirClosure) -> Self {
        let mut entry_locals = closure.params.clone();
        entry_locals.extend(closure.captures.iter().filter_map(|capture| capture.target_local));
        Self {
            locals: &closure.locals,
            blocks: &closure.blocks,
            entry_locals,
        }
    }

    /// The interned type of a local, if the index is in range.
    fn local_ty(&self, local: LocalId) -> Option<TypeId> {
        self.locals
            .get(usize::try_from(local.0).unwrap_or(usize::MAX))
            .map(|decl| decl.ty)
    }

    /// The source spelling of a local, when it has one.
    fn local_name(&self, mir: &Mir, local: LocalId) -> Option<String> {
        let decl = self
            .locals
            .get(usize::try_from(local.0).unwrap_or(usize::MAX))?;
        let symbol = match decl.kind {
            LocalKind::UserBinding(symbol) | LocalKind::Param { symbol: Some(symbol) } => symbol,
            LocalKind::Param { symbol: None } | LocalKind::Temp => return None,
        };
        mir.symbols.get(symbol).map(ToOwned::to_owned)
    }
}

/// What one operand read does to the list it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandRole {
    /// Read for its value; no handle survives the operation.
    Read,
    /// Receiver read; the buffer may be written through in place, but no handle
    /// leaves the frame.
    Receiver {
        /// Whether the operation writes to the receiver's buffer.
        mutating: bool,
    },
    /// The rvalue's result names the *same* buffer as this operand.
    ResultAlias {
        /// Whether the operation also writes to that buffer.
        mutating: bool,
    },
    /// A handle on this operand's buffer becomes reachable from outside.
    Escapes(EscapeReason),
}

/// A [`LocalId`] index as a slice index, saturating rather than wrapping.
fn index_of(local: u32) -> usize {
    usize::try_from(local).unwrap_or(usize::MAX)
}

/// Per-body union-find over list locals plus the escape verdict for each group.
struct Groups {
    /// Union-find parent pointers, indexed by `LocalId.0`.
    parent: Vec<u32>,
    /// Escape reason for each root, when the group escapes.
    escaped: HashMap<u32, EscapeReason>,
    /// Escape reason for each root, ignoring the two body-boundary sources that
    /// [`summary`] must not count: the conservative seeding of parameters and
    /// closure capture targets, and a plain [`Terminator::Return`].
    ///
    /// A caller cares about those two separately — "the callee returns this
    /// parameter" is what makes an argument escape, while "the callee returns a
    /// buffer it minted itself" is what makes a call result fresh — so they are
    /// tracked in [`Groups::entry`] and [`Groups::returned`] instead of being
    /// folded into one verdict here.
    escaped_external: HashMap<u32, EscapeReason>,
    /// Roots that contain a parameter or a closure capture target.
    entry: HashMap<u32, bool>,
    /// Roots that reach a [`Terminator::Return`] operand.
    returned: HashMap<u32, bool>,
    /// Whether each root's buffer is written in place anywhere.
    mutated: HashMap<u32, bool>,
    /// Roots whose buffer is named by more than one *concurrently live* local.
    ///
    /// A group property, not a per-local one: every local in a group names the
    /// same buffer, so they must all agree on the verdict. Keeping it per-local
    /// would let two names for one buffer report different classes and would
    /// make a per-buffer count ill-defined.
    aliased: HashMap<u32, bool>,
}

impl Groups {
    /// One singleton group per local slot.
    fn new(len: usize) -> Self {
        Self {
            parent: (0..u32::try_from(len).unwrap_or(u32::MAX)).collect(),
            escaped: HashMap::new(),
            escaped_external: HashMap::new(),
            entry: HashMap::new(),
            returned: HashMap::new(),
            mutated: HashMap::new(),
            aliased: HashMap::new(),
        }
    }

    /// Union-find root of `local`, with path halving.
    fn find(&mut self, local: u32) -> u32 {
        let mut current = local;
        while let Some(&parent) = self.parent.get(index_of(current)) {
            if parent == current {
                return current;
            }
            let grand = self.parent.get(index_of(parent)).copied().unwrap_or(parent);
            if let Some(slot) = self.parent.get_mut(index_of(current)) {
                *slot = grand;
            }
            current = grand;
        }
        current
    }

    /// Merge the groups of two locals that name the same buffer.
    ///
    /// `concurrent` says whether both names can be live at the same time. It is
    /// false for an [`Operand::Move`], which [`crate::opt`]'s move-on-last-use
    /// pass only emits when the source local is dead immediately afterwards —
    /// so the buffer is *transferred*, not shared, and a plain `Vec<T>` would
    /// express it fine. HIR lowering emits `Copy` everywhere, so every `Move`
    /// in optimized MIR carries that pass's liveness guarantee. Without this
    /// distinction the measurement would be worthless: lowering routes even
    /// `const xs = [1, 2, 3]` through a temporary, and every list in the corpus
    /// would report as `Aliased`.
    fn union(&mut self, left: u32, right: u32, concurrent: bool) {
        // The lower id always wins so the representative is deterministic and
        // the reported group name does not depend on visit order.
        let (found_left, found_right) = (self.find(left), self.find(right));
        let (left_root, right_root) = (
            found_left.min(found_right),
            found_left.max(found_right),
        );
        if left_root == right_root {
            if concurrent {
                self.aliased.insert(left_root, true);
            }
            return;
        }
        if let Some(slot) = self.parent.get_mut(index_of(right_root)) {
            *slot = left_root;
        }
        // Carry the merged group's verdicts onto the surviving root.
        if let Some(reason) = self.escaped.remove(&right_root) {
            self.escaped.entry(left_root).or_insert(reason);
        }
        if let Some(reason) = self.escaped_external.remove(&right_root) {
            self.escaped_external.entry(left_root).or_insert(reason);
        }
        if self.entry.remove(&right_root) == Some(true) {
            self.entry.insert(left_root, true);
        }
        if self.returned.remove(&right_root) == Some(true) {
            self.returned.insert(left_root, true);
        }
        if self.mutated.remove(&right_root) == Some(true) {
            self.mutated.insert(left_root, true);
        }
        if self.aliased.remove(&right_root) == Some(true) || concurrent {
            self.aliased.insert(left_root, true);
        }
    }

    /// Record that `local`'s whole group escapes. The first reason recorded
    /// wins, so a genuine escape seen before a conservative one is what the
    /// report shows.
    fn escape(&mut self, local: u32, reason: EscapeReason) {
        let root = self.find(local);
        Self::record(&mut self.escaped, root, reason);
        Self::record(&mut self.escaped_external, root, reason);
    }

    /// Insert `reason` for `root` under the "a genuine escape may replace a
    /// conservative one, never the reverse" precedence rule.
    fn record(map: &mut HashMap<u32, EscapeReason>, root: u32, reason: EscapeReason) {
        match map.get(&root) {
            // A genuine escape is more informative than a conservative guess,
            // so it may replace one; the reverse never happens.
            Some(existing) if existing.is_genuine() || !reason.is_genuine() => {}
            _ => {
                map.insert(root, reason);
            }
        }
    }

    /// Record that `local` arrives already bound from outside the frame.
    ///
    /// This is an escape for the per-body verdict but *not* for the summary: a
    /// callee that merely reads its parameter does not make the caller's
    /// argument escape.
    fn seed_entry(&mut self, local: u32) {
        let root = self.find(local);
        self.entry.insert(root, true);
        Self::record(&mut self.escaped, root, EscapeReason::UnprovenDefinition);
    }

    /// Record that `local`'s group is handed back to the caller by a
    /// [`Terminator::Return`].
    ///
    /// An escape for the per-body verdict, and for a *parameter* it is also an
    /// escape in the summary — but a body that returns a buffer it minted
    /// itself is precisely what makes the caller's result uniquely owned, so
    /// this is kept out of [`Groups::escaped_external`].
    fn escape_via_return(&mut self, local: u32) {
        let root = self.find(local);
        self.returned.insert(root, true);
        Self::record(&mut self.escaped, root, EscapeReason::Returned);
    }

    /// Record that `local`'s buffer is written in place.
    fn mutate(&mut self, local: u32) {
        let root = self.find(local);
        self.mutated.insert(root, true);
    }
}

/// Classify the list-typed locals of one body, refining statically resolved
/// calls with `summaries`.
fn analyze_body(
    mir: &Mir,
    summaries: &CallSummaries,
    body: &FunctionBody<'_>,
) -> Vec<ListLocalFact> {
    let mut walk = BodyWalk::new(mir, summaries, body);
    walk.run();
    walk.finish()
}

/// Derive the interprocedural summary of one function body under the summaries
/// currently known for its callees.
///
/// One walk serves both jobs: the group state it leaves behind records the
/// parameter-seeded and returned groups separately from the genuine escapes
/// (see [`Groups::escaped_external`]), so the per-body verdict and the summary
/// can be read off the same run.
fn summarize_function(
    mir: &Mir,
    summaries: &CallSummaries,
    function: &MirFunction,
) -> FunctionSummary {
    let body = FunctionBody::from_function(function);
    let mut walk = BodyWalk::new(mir, summaries, &body);
    walk.run();
    walk.summarize(function)
}

/// The mutable state of one body's walk.
///
/// Bundling the group state, the "seen anywhere" marks, and the body view into
/// one receiver keeps every rule below a method on the same value, so the
/// def/use rules read as a single pass instead of a chain of six-argument
/// helpers threading the same four references.
struct BodyWalk<'a> {
    /// Interners, used to decide which locals are list-typed.
    mir: &'a Mir,
    /// The body being walked.
    body: &'a FunctionBody<'a>,
    /// Escape summaries for statically resolvable callees.
    summaries: &'a CallSummaries,
    /// Union-find groups plus their escape/mutation verdicts.
    groups: Groups,
    /// Locals that appear anywhere in the body. Slots that are only declared
    /// are dead, and counting them would inflate the population with lists the
    /// program never allocates.
    touched: Vec<bool>,
}

impl<'a> BodyWalk<'a> {
    /// Start a walk with every list local in its own singleton group.
    fn new(mir: &'a Mir, summaries: &'a CallSummaries, body: &'a FunctionBody<'a>) -> Self {
        Self {
            mir,
            body,
            summaries,
            groups: Groups::new(body.locals.len()),
            touched: vec![false; body.locals.len()],
        }
    }

    /// Whether a local holds a `Type::List`.
    fn is_list(&self, local: LocalId) -> bool {
        self.body
            .local_ty(local)
            .is_some_and(|ty| matches!(self.mir.types.get(ty), Some(Type::List(_))))
    }

    /// Record that a list local appears somewhere in the body.
    fn touch(&mut self, local: LocalId) {
        if let Some(slot) = self
            .touched
            .get_mut(usize::try_from(local.0).unwrap_or(usize::MAX))
        {
            *slot = true;
        }
    }

    /// Apply every def/use rule in the body.
    fn run(&mut self) {
        // A parameter or a closure capture target arrives already bound to a
        // buffer the caller may still hold. That is the conservative half of
        // the verdict: proving otherwise needs interprocedural information this
        // pass does not have.
        for param in self.body.entry_locals.clone() {
            if self.is_list(param) {
                self.touch(param);
                self.groups.seed_entry(param.0);
            }
        }

        for block in self.body.blocks {
            for phi in &block.phis {
                self.visit_phi(phi);
            }
            for statement in &block.statements {
                self.visit_statement(statement);
            }
            self.visit_terminator(block.terminator.as_ref());
        }
    }

    /// A phi merges its incoming names onto one destination buffer.
    fn visit_phi(&mut self, phi: &crate::Phi) {
        if !self.is_list(phi.dest) {
            return;
        }
        self.touch(phi.dest);
        for (_, incoming) in &phi.incoming {
            match operand_local(incoming) {
                // Both arms of the merge name the buffer the destination will
                // name, so they share one group.
                Some(source) if self.is_list(source) => {
                    self.touch(source);
                    self.groups
                        .union(phi.dest.0, source.0, matches!(incoming, Operand::Copy(_)));
                }
                // A constant or a projected read cannot be proven fresh.
                _ => self
                    .groups
                    .escape(phi.dest.0, EscapeReason::UnprovenDefinition),
            }
        }
    }

    /// Apply the def/use rules of one statement.
    fn visit_statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Assign { dest, value } => self.visit_rvalue(value, Some(*dest)),
            // Assigning through a bare local is an ordinary definition.
            Statement::AssignPlace {
                place: Place::Local(dest),
                value,
            } => self.visit_rvalue(value, Some(*dest)),
            // `obj.f = v` / `arr[i] = v` publishes the assigned value into a
            // container, so the rvalue's result escapes instead of aliasing a
            // destination. The base is a receiver: `arr[i] = v` writes through
            // it but does not leak a handle on it.
            Statement::AssignPlace {
                place: Place::Field { base, .. },
                value,
            } => {
                self.mutate_receiver(*base);
                self.visit_rvalue(value, None);
            }
            Statement::AssignPlace {
                place: Place::Index { base, index, .. },
                value,
            } => {
                self.mutate_receiver(*base);
                // A list used as an index is not a shape the backend models;
                // stay conservative rather than guess.
                self.escape_operand(index, EscapeReason::UnmodelledUse);
                self.visit_rvalue(value, None);
            }
            // A write into a module-level mutable global publishes the value
            // into a `thread_local!` cell that outlives every function body, so
            // the assigned value escapes unconditionally. There is no base
            // LOCAL to mark as a mutated receiver -- the receiver is the cell.
            Statement::AssignPlace {
                place: Place::Global { projection, .. },
                value,
            } => {
                if let GlobalProjection::Index { index, .. } = projection {
                    self.escape_operand(index, EscapeReason::UnmodelledUse);
                }
                self.visit_rvalue(value, None);
            }
            // The fused entry update reaches into a dict through `base`, binds
            // the entry to `current` (a handle on a value that also lives
            // inside the container), and stores `value` back into it.
            Statement::DictEntryUpdate {
                base,
                index,
                default,
                current,
                value,
            } => {
                self.mutate_receiver(*base);
                if self.is_list(*current) {
                    self.touch(*current);
                    self.groups
                        .escape(current.0, EscapeReason::UnprovenDefinition);
                }
                self.escape_operand(index, EscapeReason::StoredInContainer);
                self.escape_operand(default, EscapeReason::StoredInContainer);
                self.visit_rvalue(value, None);
            }
            Statement::StorageLive(_) | Statement::StorageDead(_) => {}
        }
    }

    /// Apply the rules of one terminator.
    fn visit_terminator(&mut self, terminator: Option<&Terminator>) {
        let Some(found) = terminator else { return };
        match found {
            // A call result may be a handle the callee kept; an awaited value
            // comes from a future built elsewhere. Neither is provably fresh.
            Terminator::Call {
                callee, args, dest, ..
            } => self.visit_call(callee, args, *dest),
            Terminator::Await { future, dest, .. } => {
                self.escape_operand(future, EscapeReason::CallArgument);
                self.define_from_outside(*dest);
            }
            // A return hands the buffer to the caller. It is kept apart from
            // the other escapes because a body that returns a buffer it minted
            // itself is what makes the *caller's* result uniquely owned.
            Terminator::Return(operand) => self.return_operand(operand),
            // A throw publishes the value onto the unwind path, where no
            // summary describes what happens to it.
            Terminator::Throw(operand) => {
                self.escape_operand(operand, EscapeReason::Returned);
            }
            // Branch conditions only test a value; no handle survives.
            Terminator::Switch { cond: operand, .. }
            | Terminator::Match {
                scrutinee: operand, ..
            } => {
                if let Some(local) = operand_local(operand)
                    && self.is_list(local)
                {
                    self.touch(local);
                }
            }
            Terminator::Goto(_) | Terminator::Unreachable => {}
        }
    }

    /// Apply the rules of a [`Terminator::Call`].
    ///
    /// When the callee is a body this analysis has a usable summary for, each
    /// argument is refined by that summary: a parameter the callee provably
    /// does not let escape leaves the argument confined (merely mutated, if the
    /// callee writes through it), and a callee that provably returns a
    /// uniquely owned fresh list makes the destination a fresh definition
    /// rather than an unproven one.
    ///
    /// Every other callee shape — an indirect call through a runtime function
    /// value, a builtin, an out-of-range or overridable or `async`/generator
    /// target, or a mismatched argument list — is an **unknown callee**: every
    /// argument escapes and the destination is unproven. See the
    /// [`summary`] module docs for the full list.
    fn visit_call(&mut self, callee: &Callee, args: &[Operand], dest: LocalId) {
        let resolved = match callee {
            Callee::Static(func) => self.summaries.resolved(*func),
            // The callee value itself is handed to the dispatcher, and nothing
            // names the body that will run.
            Callee::Indirect(operand) => {
                self.escape_operand(operand, EscapeReason::CallArgument);
                None
            }
            Callee::Builtin(_) => None,
        };
        let Some(summary) = resolved else {
            for arg in args {
                self.escape_operand(arg, EscapeReason::CallArgument);
            }
            self.define_from_outside(dest);
            return;
        };
        let effects = args
            .iter()
            .enumerate()
            .map(|(index, _)| summary.argument_effect(index, args.len()))
            .collect::<Vec<_>>();
        let returns_fresh = summary.returns_fresh_list;
        for (arg, effect) in args.iter().zip(effects) {
            match effect {
                ArgumentEffect::Escapes => {
                    self.escape_operand(arg, EscapeReason::CallArgument);
                }
                ArgumentEffect::Confined { mutated } => self.read_operand(arg, mutated),
            }
        }
        if returns_fresh {
            // The callee minted this buffer and published it nowhere else, so
            // the caller is its only owner: a fresh definition, exactly like a
            // literal or a `slice()`.
            if self.is_list(dest) {
                self.touch(dest);
            }
        } else {
            self.define_from_outside(dest);
        }
    }

    /// Note an argument the callee provably does not retain.
    ///
    /// Mirrors [`BodyWalk::escape_operand`]'s place handling: a projected read
    /// only touches the base, because the list being passed is not a local.
    fn read_operand(&mut self, operand: &Operand, mutated: bool) {
        match operand_place(operand) {
            Some(&Place::Local(named)) if self.is_list(named) => {
                self.touch(named);
                if mutated {
                    self.groups.mutate(named.0);
                }
            }
            Some(&Place::Field { base, .. } | &Place::Index { base, .. })
                if self.is_list(base) =>
            {
                self.touch(base);
            }
            _ => {}
        }
    }

    /// Note the operand a [`Terminator::Return`] hands back to the caller.
    fn return_operand(&mut self, operand: &Operand) {
        match operand_place(operand) {
            Some(&Place::Local(named)) if self.is_list(named) => {
                self.touch(named);
                self.groups.escape_via_return(named.0);
            }
            // Returning `obj.items` hands back a buffer that also lives inside
            // the container, which no summary can call uniquely owned.
            Some(&Place::Field { base, .. } | &Place::Index { base, .. })
                if self.is_list(base) =>
            {
                self.touch(base);
            }
            _ => {}
        }
    }

    /// Note a destination bound to a value the analysis cannot prove fresh.
    fn define_from_outside(&mut self, dest: LocalId) {
        if self.is_list(dest) {
            self.touch(dest);
            self.groups
                .escape(dest.0, EscapeReason::UnprovenDefinition);
        }
    }

    /// Note an in-place write through a container base.
    fn mutate_receiver(&mut self, base: LocalId) {
        if self.is_list(base) {
            self.touch(base);
            self.groups.mutate(base.0);
        }
    }

    /// Mark every list local an operand names as escaping for `reason`.
    fn escape_operand(&mut self, operand: &Operand, reason: EscapeReason) {
        match operand_place(operand) {
            Some(&Place::Local(named)) if self.is_list(named) => {
                self.touch(named);
                self.groups.escape(named.0, reason);
            }
            // A projected read pulls a value *out* of the base; the base itself
            // is only read.
            Some(&Place::Field { base, .. } | &Place::Index { base, .. })
                if self.is_list(base) =>
            {
                self.touch(base);
            }
            _ => {}
        }
    }

    /// Apply the role table of `value`.
    ///
    /// `dest` is the local the result is bound to, when there is one. `None`
    /// means the result is stored somewhere the analysis treats as outside the
    /// frame, which turns [`OperandRole::ResultAlias`] into an escape.
    fn visit_rvalue(&mut self, value: &Rvalue, dest: Option<LocalId>) {
        let roles = operand_roles(value);
        // `(local, concurrent)`: `concurrent` is false for a move, which
        // transfers the buffer instead of creating a second live name.
        let mut alias_sources: Vec<(LocalId, bool)> = Vec::new();
        let mut reads: Vec<(LocalId, OperandRole)> = Vec::new();
        let mut bases: Vec<LocalId> = Vec::new();
        value.for_each_operand(|operand| {
            // Any operand the role table did not name is unmodelled, so it
            // escapes. This is what makes a newly added `Rvalue` variant (or a
            // newly added operand on an existing one) fail safe.
            let role = roles
                .iter()
                .find(|(candidate, _)| std::ptr::eq(*candidate, operand))
                .map_or(
                    OperandRole::Escapes(EscapeReason::UnmodelledUse),
                    |(_, role)| *role,
                );
            match operand_place(operand) {
                Some(Place::Local(local)) => {
                    if let OperandRole::ResultAlias { .. } = role {
                        alias_sources.push((*local, matches!(operand, Operand::Copy(_))));
                    }
                    reads.push((*local, role));
                }
                // A read through a projection also touches the base local, and
                // reaching into a container through a list base is a plain read.
                Some(Place::Field { base, .. } | Place::Index { base, .. }) => bases.push(*base),
                // A global-rooted place is only ever an assignment target, so
                // it names no local to record as a read.
                Some(Place::Global { .. }) => {}
                None => {}
            }
        });

        for base in bases {
            if self.is_list(base) {
                self.touch(base);
            }
        }
        for (local, role) in reads {
            if !self.is_list(local) {
                continue;
            }
            self.touch(local);
            match role {
                OperandRole::Read => {}
                OperandRole::Receiver { mutating } | OperandRole::ResultAlias { mutating } => {
                    if mutating {
                        self.groups.mutate(local.0);
                    }
                }
                OperandRole::Escapes(reason) => self.groups.escape(local.0, reason),
            }
        }
        alias_sources.retain(|(local, _)| self.is_list(*local));

        let Some(target) = dest else {
            // The result is stored into a container, so anything the result
            // would have aliased is published with it.
            for (source, _) in alias_sources {
                self.groups
                    .escape(source.0, EscapeReason::StoredInContainer);
            }
            return;
        };
        if !self.is_list(target) {
            // A non-list destination cannot carry the buffer forward.
            return;
        }
        self.touch(target);
        if alias_sources.is_empty() {
            if !rvalue_mints_fresh_list(value) {
                self.groups
                    .escape(target.0, EscapeReason::UnprovenDefinition);
            }
            return;
        }
        for (source, concurrent) in alias_sources {
            self.groups.union(target.0, source.0, concurrent);
        }
    }

    /// Collect one fact per list-typed local that the body actually uses.
    fn finish(mut self) -> Vec<ListLocalFact> {
        let mut facts = Vec::new();
        for index in 0..self.body.locals.len() {
            let local = LocalId(u32::try_from(index).unwrap_or(u32::MAX));
            if !self.is_list(local) || !self.touched.get(index).copied().unwrap_or(false) {
                continue;
            }
            let root = self.groups.find(local.0);
            let reason = self.groups.escaped.get(&root).copied();
            let mutated = self.groups.mutated.get(&root).copied().unwrap_or(false);
            let aliased = self.groups.aliased.get(&root).copied().unwrap_or(false);
            let class = match (reason, aliased, mutated) {
                (Some(_), _, _) => ListLocalClass::Escaping,
                (None, true, _) => ListLocalClass::Aliased,
                (None, false, true) => ListLocalClass::LocalMutated,
                (None, false, false) => ListLocalClass::LocalImmutable,
            };
            facts.push(ListLocalFact {
                local,
                group: LocalId(root),
                name: self.body.local_name(self.mir, local),
                class,
                reason,
                mutated,
            });
        }
        facts
    }

    /// Read the interprocedural summary off a finished walk.
    ///
    /// A parameter escapes when its group carries any escape that is not the
    /// conservative entry seeding — including a plain `Return`, since returning
    /// a parameter really does hand the caller's buffer back out. A body
    /// returns a fresh list when every `Return` operand's group was minted
    /// inside this body (no entry local reached it) and is published nowhere
    /// else.
    fn summarize(mut self, function: &MirFunction) -> FunctionSummary {
        let mut param_escapes = Vec::with_capacity(function.params.len());
        let mut param_mutated = Vec::with_capacity(function.params.len());
        for param in &function.params {
            if !self.is_list(*param) {
                // A non-list parameter cannot carry a list buffer, and a
                // non-list argument never consults this entry. Reporting it as
                // escaping keeps the summary conservative if it ever is.
                param_escapes.push(true);
                param_mutated.push(false);
                continue;
            }
            let root = self.groups.find(param.0);
            param_escapes.push(
                self.groups.escaped_external.contains_key(&root)
                    || self.groups.returned.get(&root).copied().unwrap_or(false),
            );
            param_mutated.push(self.groups.mutated.get(&root).copied().unwrap_or(false));
        }

        let returns_fresh_list = self.returns_fresh_list(function);
        FunctionSummary {
            resolvable: true,
            packs_rest: function.rest.is_some(),
            param_escapes,
            param_mutated,
            returns_fresh_list,
        }
    }

    /// Whether every `Return` in this body hands back a uniquely owned buffer.
    ///
    /// False unless the declared return type is a list — a call whose
    /// destination is not list-typed never consults this — and false as soon as
    /// one return operand is anything but a list local whose group is free of
    /// entry locals and of every escape other than the return itself.
    fn returns_fresh_list(&mut self, function: &MirFunction) -> bool {
        if !matches!(
            self.mir.types.get(function.return_ty),
            Some(Type::List(_))
        ) {
            return false;
        }
        let returned: Vec<Operand> = function
            .blocks
            .iter()
            .filter_map(|block| match block.terminator.as_ref() {
                Some(Terminator::Return(operand)) => Some(operand.clone()),
                _ => None,
            })
            .collect();
        returned.iter().all(|operand| match operand_local(operand) {
            Some(local) if self.is_list(local) => {
                let root = self.groups.find(local.0);
                !self.groups.escaped_external.contains_key(&root)
                    && !self.groups.entry.get(&root).copied().unwrap_or(false)
            }
            // A constant, or a buffer read out through a projection: not
            // provably minted here.
            _ => false,
        })
    }
}

/// The place an operand reads, if it reads one.
const fn operand_place(operand: &Operand) -> Option<&Place> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => Some(place),
        Operand::Const(_) => None,
    }
}

/// The local an operand names directly, ignoring projected reads.
const fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand_place(operand) {
        Some(Place::Local(local)) => Some(*local),
        Some(Place::Field { .. } | Place::Index { .. } | Place::Global { .. }) | None => None,
    }
}

/// Whether this rvalue's result is a brand-new list buffer nothing else names.
///
/// Deliberately a short whitelist. Leaving a genuinely fresh producer out only
/// under-counts the tierable population; putting a non-fresh one in would be
/// unsound, so only operations whose runtime helper allocates a new `Vec` are
/// listed. In particular the JavaScript in-place mutators (`sort`, `reverse`,
/// `fill`, `copyWithin`) return *the receiver* and so are handled as
/// [`OperandRole::ResultAlias`] instead of appearing here.
#[must_use]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "the wildcard is the soundness guarantee: an `Rvalue` variant this \
              whitelist has not vetted must report `false` (not provably fresh), so \
              a variant added later degrades to a conservative `Escaping` verdict \
              rather than to a silently wrong `Local` one"
)]
pub const fn rvalue_mints_fresh_list(value: &Rvalue) -> bool {
    match value {
        // Literal construction and the explicit copying operations.
        Rvalue::List(_)
        | Rvalue::ListCopy { .. }
        | Rvalue::ListSlice { .. }
        | Rvalue::ListConcat { .. }
        | Rvalue::ListWith { .. }
        | Rvalue::ListFlat { .. }
        | Rvalue::ListFromLength { .. }
        | Rvalue::ListRepeat { .. }
        | Rvalue::ListFromLengthMap { .. }
        // `splice` returns the removed elements as a new array; `toSpliced`
        // (`mutate: false`) returns a new array outright. Both are fresh.
        | Rvalue::ListSplice { .. }
        // Non-mutating ES2023 variants: `toSorted`, `toReversed`.
        | Rvalue::ListSorted { .. }
        | Rvalue::ListReversed { .. }
        | Rvalue::ListEnumerate { .. }
        | Rvalue::ListZip { .. }
        | Rvalue::ListRange { .. }
        | Rvalue::ListProjection { .. }
        | Rvalue::TupleToList { .. }
        | Rvalue::TupleSlice { .. }
        | Rvalue::DictProjection { .. }
        | Rvalue::SetProjection { .. }
        // String and regex splits allocate their result vector.
        | Rvalue::StringSplit { .. }
        | Rvalue::StringChars { .. }
        | Rvalue::RegexSplit { .. } => true,
        // Only the list-producing callbacks; `find`/`some`/`forEach` return an
        // element or a scalar, and a returned element is a handle on a value
        // that also lives inside the receiver.
        Rvalue::ListCallback { op, .. } => matches!(
            op,
            smelt_hir::ListCallbackOp::Map
                | smelt_hir::ListCallbackOp::Filter
                | smelt_hir::ListCallbackOp::FlatMap
        ),
        _ => false,
    }
}

/// The role of each operand this rvalue reads, for operands the analysis models.
///
/// Returned as `(&Operand, role)` pairs so [`apply_rvalue`] can match them
/// against [`Rvalue::for_each_operand`] by pointer identity — that keeps the
/// canonical operand walk as the single enumeration point while still letting
/// positions differ in role. **Any operand not named here gets
/// [`OperandRole::Escapes`]**, including every operand of every variant that
/// falls into the wildcard arm, so the table fails safe.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "the wildcard is the soundness guarantee: an unlisted `Rvalue` variant \
              yields no roles, so every operand it reads falls to \
              `OperandRole::Escapes`. A variant added later fails safe instead of \
              silently producing a wrong `Local` verdict"
)]
fn operand_roles(value: &Rvalue) -> Vec<(&Operand, OperandRole)> {
    const READ: OperandRole = OperandRole::Read;
    const RECV: OperandRole = OperandRole::Receiver { mutating: false };
    const RECV_MUT: OperandRole = OperandRole::Receiver { mutating: true };
    const ALIAS: OperandRole = OperandRole::ResultAlias { mutating: false };
    const ALIAS_MUT: OperandRole = OperandRole::ResultAlias { mutating: true };
    const STORED: OperandRole = OperandRole::Escapes(EscapeReason::StoredInContainer);
    const CALLED: OperandRole = OperandRole::Escapes(EscapeReason::CallArgument);
    const ERASED: OperandRole = OperandRole::Escapes(EscapeReason::Erased);

    match value {
        // Plain propagation: the destination names the source's buffer.
        Rvalue::Use(operand) => vec![(operand, ALIAS)],
        // Both arms may become the result.
        Rvalue::Conditional {
            cond,
            then_operand,
            else_operand,
        } => vec![(cond, READ), (then_operand, ALIAS), (else_operand, ALIAS)],
        Rvalue::OptionalCoalesce { optional, fallback } => {
            vec![(optional, ALIAS), (fallback, ALIAS)]
        }

        // In-place mutators that return the receiver in JavaScript.
        Rvalue::ListReverse { list } => vec![(list, ALIAS_MUT)],
        Rvalue::ListSort {
            list,
            comparator,
            key,
            ..
        } => {
            let mut roles = vec![(list, ALIAS_MUT)];
            roles.extend(
                comparator
                    .iter()
                    .chain(key.iter())
                    .map(|callback| (callback, CALLED)),
            );
            roles
        }
        Rvalue::ListFill { list, value: item, .. } => {
            vec![(list, ALIAS_MUT), (item, STORED)]
        }
        Rvalue::ListCopyWithin { list, .. } => vec![(list, ALIAS_MUT)],

        // In-place mutators whose result is a scalar or a removed element.
        Rvalue::ListPush { list, item } => vec![(list, RECV_MUT), (item, STORED)],
        Rvalue::ListInsert { list, index, item } => {
            vec![(list, RECV_MUT), (index, READ), (item, STORED)]
        }
        Rvalue::ListUnshift { list, items } => {
            let mut roles = vec![(list, RECV_MUT)];
            roles.extend(items.iter().map(|item| (item, STORED)));
            roles
        }
        // `extend` copies `other`'s elements out; `other` itself keeps its
        // buffer, so it is an ordinary read.
        Rvalue::ListExtend { list, other } => vec![(list, RECV_MUT), (other, READ)],
        Rvalue::ListClear { list }
        | Rvalue::ListPop { list }
        | Rvalue::ListShift { list }
        // `next` advances the iteration cursor, which is a write.
        | Rvalue::ListNext { list } => vec![(list, RECV_MUT)],
        Rvalue::ListRemove { list, item } => vec![(list, RECV_MUT), (item, READ)],
        Rvalue::ListSplice {
            list,
            start,
            delete_count,
            items,
            mutate,
        } => {
            let mut roles = vec![
                (
                    list,
                    OperandRole::Receiver {
                        mutating: *mutate,
                    },
                ),
                (start, READ),
            ];
            roles.extend(delete_count.iter().map(|count| (count, READ)));
            roles.extend(items.iter().map(|item| (&item.value, STORED)));
            roles
        }

        // Read-only list operations: the receiver is inspected and the result is
        // fresh or scalar.
        Rvalue::Len(list)
        | Rvalue::ListCopy { list }
        | Rvalue::ListSum { list }
        | Rvalue::ListToSet { list }
        | Rvalue::ListPairsToDict { list }
        | Rvalue::ListToTuple { list }
        | Rvalue::ListEnumerate { list }
        | Rvalue::ListRandomChoice { list }
        | Rvalue::ListBoolFold { list, .. }
        | Rvalue::ListProjection { list, .. }
        | Rvalue::ListReversed { list } => vec![(list, RECV)],
        Rvalue::ListSlice { list, start, end } => {
            let mut roles = vec![(list, RECV)];
            roles.extend(start.iter().map(|bound| (bound, READ)));
            roles.extend(end.iter().map(|bound| (bound, READ)));
            roles
        }
        Rvalue::ListFlat { list, depth } => {
            let mut roles = vec![(list, RECV)];
            roles.extend(depth.iter().map(|level| (level, READ)));
            roles
        }
        Rvalue::ListConcat { left, right } | Rvalue::ListZip { left, right } => {
            vec![(left, RECV), (right, RECV)]
        }
        Rvalue::ListContains { list, item } | Rvalue::ListCount { list, item } => {
            vec![(list, RECV), (item, READ)]
        }
        Rvalue::ListIndex { list, item } => vec![(list, RECV), (item, READ)],
        Rvalue::ListSearch {
            list,
            item,
            from_index,
            ..
        } => {
            let mut roles = vec![(list, RECV), (item, READ)];
            roles.extend(from_index.iter().map(|index| (index, READ)));
            roles
        }
        // The callback is a closure value, not a list; if a list ever appears
        // there it is being handed to something that can keep it.
        // JavaScript hands the receiver to the callback as its third argument
        // (`cb(item, index, array)`), and codegen emits exactly that
        // (`(smelt_callback)(item, index as i64, &smelt_array)`), so the
        // callback can retain a handle on the array. The receiver therefore
        // escapes here even though `map`/`filter` themselves only read it.
        Rvalue::ListCallback { list, callback, .. } => vec![(list, CALLED), (callback, CALLED)],
        Rvalue::ListSorted { list, key, .. } => {
            let mut roles = vec![(list, RECV)];
            roles.extend(key.iter().map(|selector| (selector, CALLED)));
            roles
        }
        // The accumulator seed flows into the reduction's result and through
        // the callback, so it is published even though the receiver is not.
        // `reduce` likewise passes the array as the reducer's fourth argument.
        Rvalue::ListReduce {
            list,
            initial,
            callback,
        } => {
            let mut roles = vec![(list, CALLED), (callback, CALLED)];
            roles.extend(initial.iter().map(|seed| (seed, CALLED)));
            roles
        }
        Rvalue::ListFromLengthMap { length, callback } => {
            vec![(length, READ), (callback, CALLED)]
        }
        Rvalue::ListFromLength { length } => vec![(length, READ)],
        // The repeated value ends up inside the produced list.
        Rvalue::ListRepeat { value: item, count } => vec![(item, STORED), (count, READ)],
        Rvalue::ListRange { start, end, step } => {
            vec![(start, READ), (end, READ), (step, READ)]
        }
        // `with` copies, but the replacement value lands inside the copy.
        Rvalue::ListWith {
            list,
            index,
            value: replacement,
        } => vec![(list, RECV), (index, READ), (replacement, STORED)],
        Rvalue::StringJoin { items, separator } => vec![(items, RECV), (separator, READ)],

        // Scalar-producing inspections of arbitrary values.
        Rvalue::Binary { lhs, rhs, .. } => vec![(lhs, READ), (rhs, READ)],
        Rvalue::Unary { operand, .. }
        | Rvalue::TypeofValue { value: operand }
        | Rvalue::UnknownIs { value: operand, .. }
        | Rvalue::InstanceOf { value: operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::JsonStringify { value: operand }
        | Rvalue::StructuredClone { operand } => vec![(operand, READ)],
        Rvalue::NumericExtrema { args, spread, .. } => {
            let mut roles: Vec<(&Operand, OperandRole)> =
                args.iter().map(|arg| (arg, READ)).collect();
            roles.extend(spread.iter().map(|list| (list, RECV)));
            roles
        }

        // Genuine escapes, named explicitly so the reported reason distinguishes
        // "really escapes" from "could not prove otherwise". The verdict would
        // be the same without these arms; only the reason label would blur.
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            items.iter().map(|item| (item, STORED)).collect()
        }
        Rvalue::Dict(entries) => entries
            .iter()
            .flat_map(|(key, entry)| [(key, STORED), (entry, STORED)])
            .collect(),
        Rvalue::Struct { fields, .. } => {
            fields.iter().map(|(_, field)| (field, STORED)).collect()
        }
        Rvalue::DictSet {
            dict,
            key: entry_key,
            value: entry_value,
        } => vec![(dict, RECV), (entry_key, STORED), (entry_value, STORED)],
        Rvalue::SetAdd { set, item } => vec![(set, RECV), (item, STORED)],
        Rvalue::GlobalSet { value: stored, .. } => vec![(stored, STORED)],
        Rvalue::Closure { captures, .. } => captures
            .iter()
            .map(|capture| (capture, OperandRole::Escapes(EscapeReason::Captured)))
            .collect(),
        Rvalue::ClosureCall { callee, args } => {
            let mut roles = vec![(callee, CALLED)];
            roles.extend(args.iter().map(|arg| (arg, CALLED)));
            roles
        }
        Rvalue::ClosureCallSpread { callee, args } => vec![(callee, CALLED), (args, CALLED)],
        Rvalue::ExternalClassInstance { args, .. }
        | Rvalue::HostConstruct { args, .. }
        | Rvalue::AsyncOp { args, .. } => args.iter().map(|arg| (arg, CALLED)).collect(),
        Rvalue::OptionalMethod { receiver, args, .. }
        | Rvalue::UnionMethod { receiver, args, .. } => {
            let mut roles = vec![(receiver, CALLED)];
            roles.extend(args.iter().map(|arg| (arg, CALLED)));
            roles
        }
        Rvalue::Await(operand) => vec![(operand, CALLED)],
        Rvalue::UnknownCast { value: operand, .. } | Rvalue::BoxPrimitive { value: operand } => {
            vec![(operand, ERASED)]
        }

        // Everything else — remaining containers, host globals, dates, regexes,
        // and any variant added after this table was written — publishes its
        // operands. Falling into this arm is what keeps a new `Rvalue` variant
        // from silently producing a wrong `Local` verdict.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
