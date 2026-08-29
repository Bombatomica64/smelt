//! Interprocedural escape summaries for the list-escape analysis.
//!
//! # Why this exists
//!
//! [`crate::list_escape`] classifies one body at a time. On its own that forces
//! it to assume every callee retains whatever it is handed, so a locally
//! allocated list handed to a helper that only reads it is reported as
//! `escaping` with reason `call-argument`, and every call *result* is reported
//! as an unproven definition. Both verdicts are artefacts of the per-body
//! scope, not of the program.
//!
//! This module computes, for every [`MirFunction`] in the crate, two facts the
//! per-body walk can then consult at a statically resolved call site:
//!
//! * [`FunctionSummary::param_escapes`] — per parameter, whether a handle on
//!   the argument's buffer can outlive the call (the callee returns it, throws
//!   it, stores it in a container, captures it, erases it, writes it to a
//!   global, or hands it to something that does any of those);
//! * [`FunctionSummary::returns_fresh_list`] — whether every value this body
//!   returns is a list buffer minted inside the body and not reachable from any
//!   parameter, so the caller receives a uniquely owned list.
//!
//! [`FunctionSummary::param_mutated`] rides along: a callee that writes through
//! a parameter in place does not make the argument escape, but the caller must
//! still count the argument as mutated or the immutable/mutated split would
//! under-report writes.
//!
//! Nothing here changes codegen. The summaries only make the *measurement* in
//! [`crate::list_escape`] less pessimistic; the module is reachable from the
//! `list-escape-report` CLI command and from nothing else.
//!
//! # Why starting optimistic is sound
//!
//! "May escape" is a least-fixpoint property: an escape is only ever *derived*
//! from a concrete escaping operation in some body, through a finite chain of
//! call edges. The lattice is ordered by "more escaping" — the bottom element
//! is `param_escapes = all false`, `returns_fresh_list = true` (no argument
//! escapes, every result is fresh), and every transfer function is monotone,
//! because adding an escape to a callee's summary can only add escapes to its
//! callers' summaries and can only turn `returns_fresh_list` from `true` to
//! `false`, never the reverse. Kleene iteration from the bottom element
//! therefore converges to the *least* fixpoint, which is exactly the set of
//! escapes with a finite derivation — that is, the set of escapes that can
//! actually happen.
//!
//! Starting optimistic is not an assumption about the program: during iteration
//! the analysis never *asserts* that nothing escapes, it only has not derived
//! an escape yet, and it is not allowed to stop before the fixpoint. The
//! alternative — starting pessimistic and removing escapes — would compute a
//! greatest fixpoint, which for recursion would happily keep an escape alive
//! that nothing witnesses, i.e. it would be sound but strictly less precise.
//!
//! Recursion and mutual recursion fall out of this: a self-edge or a cycle in
//! the call graph is just an equation whose solution the iteration reaches.
//! Termination is guaranteed because every summary field is a boolean that can
//! only move in one direction (`false` → `true` for the escape and mutation
//! flags, `true` → `false` for `returns_fresh_list`), the number of fields is
//! finite, and a body is only re-analyzed when a callee's summary actually
//! changed.
//!
//! Note that `returns_fresh_list` is a *must*-style property, but it is encoded
//! here as the negation of a may-property ("may return a non-fresh list"), so
//! it lives in the same lattice and moves in the same direction as the rest.
//! `f(n) { return n <= 0 ? [] : f(n - 1); }` stays `returns_fresh_list = true`
//! because every base case mints a fresh list; if any base case returned a
//! global, that base case would drive the flag to `false` and the recursion
//! would inherit it.
//!
//! # Unknown callees
//!
//! A summary may only be consulted when the analysis can name the exact body
//! codegen will invoke. Every other call shape is an **unknown callee**, and an
//! unknown callee makes every list it is handed escape and every list it hands
//! back an unproven definition. The complete list:
//!
//! * [`crate::Callee::Indirect`] — a call through a runtime function value;
//! * [`crate::Callee::Builtin`] — a builtin whose body is not in this MIR;
//! * [`crate::Callee::Static`] whose [`crate::FuncId`] is out of range;
//! * [`crate::Callee::Static`] naming a method of a class that participates in
//!   inheritance (see [`overridable_methods`]) — the receiver's runtime class
//!   may not be its static class;
//! * [`crate::Callee::Static`] naming an `async` or generator function — its
//!   parameters are stored into suspended state that outlives the call;
//! * a call whose argument count does not line up positionally with the
//!   callee's parameters, or whose callee packs a `...rest` parameter, so an
//!   argument cannot be matched to the parameter whose summary describes it;
//! * every callback-shaped [`crate::Rvalue`] — `ListCallback`, `ListReduce`,
//!   `ListSort`/`ListSorted` selectors, `ListFromLengthMap` — whose callback is
//!   a `SmeltErasedFunction` value, not a named body;
//! * [`crate::Rvalue::ClosureCall`] and [`crate::Rvalue::ClosureCallSpread`] —
//!   closure dispatch;
//! * [`crate::Rvalue::ExternalClassInstance`], [`crate::Rvalue::HostConstruct`],
//!   [`crate::Rvalue::AsyncOp`], [`crate::Rvalue::Await`] and
//!   [`crate::Terminator::Await`] — host and runtime boundaries with no MIR
//!   body;
//! * [`crate::Rvalue::OptionalMethod`] and [`crate::Rvalue::UnionMethod`] —
//!   dispatch on a receiver whose concrete body is not fixed;
//! * [`crate::Rvalue::UnknownCast`] and [`crate::Rvalue::BoxPrimitive`] — the
//!   erased `SmeltUnknown` boundary;
//! * any [`crate::Rvalue`] variant the role table does not enumerate, via its
//!   wildcard arm.
//!
//! Closure bodies get no summaries at all: every call that reaches one goes
//! through closure dispatch, which is already an unknown callee, so a summary
//! for a closure could never be consulted.

use std::collections::{HashSet, VecDeque};

use crate::{BasicBlock, Callee, FuncId, Mir, MirFunction, Terminator};

/// What a statically resolved call does to one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArgumentEffect {
    /// A handle on the argument's buffer can outlive the call.
    Escapes,
    /// The callee keeps no handle. `mutated` says whether it nonetheless
    /// writes through the argument in place.
    Confined {
        /// Whether the callee mutates the argument's buffer.
        mutated: bool,
    },
}

/// The interprocedural escape facts of one [`MirFunction`].
///
/// Only meaningful when [`FunctionSummary::resolvable`] is true; the summary of
/// an unresolvable body is never consulted and stays at its optimistic initial
/// value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FunctionSummary {
    /// Whether a call site may consult this summary at all. False for `async`
    /// and generator bodies and for methods of classes that participate in
    /// inheritance; see the module docs.
    pub(super) resolvable: bool,
    /// Whether this body packs a `...rest` parameter, which breaks the
    /// positional argument-to-parameter mapping.
    pub(super) packs_rest: bool,
    /// Per parameter, whether a handle on the argument may outlive the call.
    pub(super) param_escapes: Vec<bool>,
    /// Per parameter, whether the body writes through it in place.
    pub(super) param_mutated: Vec<bool>,
    /// Whether every returned value is a freshly minted list buffer that is not
    /// reachable from any parameter and is published nowhere else.
    pub(super) returns_fresh_list: bool,
}

impl FunctionSummary {
    /// The optimistic bottom element for `function`: nothing escapes, nothing
    /// is mutated, every result is fresh.
    fn optimistic(function: &MirFunction, resolvable: bool) -> Self {
        Self {
            resolvable,
            packs_rest: function.rest.is_some(),
            param_escapes: vec![false; function.params.len()],
            param_mutated: vec![false; function.params.len()],
            returns_fresh_list: true,
        }
    }

    /// The effect of this call on the argument at `index`, given that the call
    /// site passes `arg_count` arguments.
    ///
    /// Anything that breaks the positional mapping — a packed rest parameter,
    /// an argument count that does not match the parameter count, an index past
    /// the end — degrades to [`ArgumentEffect::Escapes`].
    pub(super) fn argument_effect(&self, index: usize, arg_count: usize) -> ArgumentEffect {
        if self.packs_rest || arg_count != self.param_escapes.len() {
            return ArgumentEffect::Escapes;
        }
        match (self.param_escapes.get(index), self.param_mutated.get(index)) {
            (Some(false), Some(mutated)) => ArgumentEffect::Confined { mutated: *mutated },
            _ => ArgumentEffect::Escapes,
        }
    }
}

/// Every function's summary, indexed by [`FuncId`].
#[derive(Debug, Clone)]
pub(super) struct CallSummaries {
    /// One entry per `mir.functions` slot, in the same order.
    summaries: Vec<FunctionSummary>,
    /// How many body summarizations the fixpoint performed, for cost reporting.
    analyses: usize,
}

impl CallSummaries {
    /// A table in which no function is resolvable.
    ///
    /// Used to reproduce the purely per-body verdict — every call is an unknown
    /// callee — which is what the tests compare the refined verdict against.
    #[cfg(test)]
    pub(super) fn none(mir: &Mir) -> Self {
        Self {
            summaries: mir
                .functions
                .iter()
                .map(|function| FunctionSummary::optimistic(function, false))
                .collect(),
            analyses: 0,
        }
    }

    /// The summary of `func`, or `None` when the call must be treated as an
    /// unknown callee.
    pub(super) fn resolved(&self, func: FuncId) -> Option<&FunctionSummary> {
        self.summaries
            .get(usize::try_from(func.0).ok()?)
            .filter(|summary| summary.resolvable)
    }

    /// How many body summarizations the fixpoint performed.
    #[cfg(test)]
    pub(super) const fn analyses(&self) -> usize {
        self.analyses
    }

    /// Compute the least fixpoint of the escape summaries over `mir`'s call
    /// graph.
    ///
    /// Cost: one pass to build the reverse call graph, then a worklist of body
    /// summarizations. Each body is re-summarized only when a callee's summary
    /// actually changed, and each summary can change at most
    /// `2 * params + 1` times (every field is a boolean that moves once), so
    /// the number of body walks is bounded by
    /// `O(sum over callees of callers(callee) * fields(callee))` rather than by
    /// the quadratic `bodies * bodies` a round-robin loop would cost.
    pub(super) fn compute(mir: &Mir) -> Self {
        let overridable = overridable_methods(mir);
        let mut table = Self {
            summaries: mir
                .functions
                .iter()
                .enumerate()
                .map(|(index, function)| {
                    let resolvable = !function.is_async
                        && !function.is_generator
                        && !overridable.contains(&index);
                    FunctionSummary::optimistic(function, resolvable)
                })
                .collect(),
            analyses: 0,
        };

        let callers = reverse_call_graph(mir);
        // Seed the worklist with every resolvable body. An unresolvable body's
        // summary is never read, so summarizing it would be wasted work.
        let mut queued: Vec<bool> = table
            .summaries
            .iter()
            .map(|summary| summary.resolvable)
            .collect();
        let mut worklist: VecDeque<usize> = (0..mir.functions.len())
            .filter(|index| queued.get(*index).copied().unwrap_or(false))
            .collect();

        while let Some(index) = worklist.pop_front() {
            if let Some(slot) = queued.get_mut(index) {
                *slot = false;
            }
            let Some(function) = mir.functions.get(index) else {
                continue;
            };
            table.analyses = table.analyses.saturating_add(1);
            let next = super::summarize_function(mir, &table, function);
            let changed = table
                .summaries
                .get(index)
                .is_some_and(|current| *current != next);
            if !changed {
                continue;
            }
            if let Some(slot) = table.summaries.get_mut(index) {
                *slot = next;
            }
            for caller in callers.get(index).into_iter().flatten() {
                if let Some(slot) = queued.get_mut(*caller)
                    && !*slot
                    && table
                        .summaries
                        .get(*caller)
                        .is_some_and(|summary| summary.resolvable)
                {
                    *slot = true;
                    worklist.push_back(*caller);
                }
            }
        }
        table
    }
}

/// For each function index, the indices of the functions that call it
/// statically.
///
/// Built in one pass over every block of every function and closure, so it
/// costs `O(call edges)` rather than a lookup per query.
fn reverse_call_graph(mir: &Mir) -> Vec<Vec<usize>> {
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); mir.functions.len()];
    for (index, function) in mir.functions.iter().enumerate() {
        for callee in static_callees(&function.blocks) {
            if let Some(slot) = callers.get_mut(callee)
                && !slot.contains(&index)
            {
                slot.push(index);
            }
        }
    }
    callers
}

/// The indices of every statically named callee reachable from `blocks`.
fn static_callees(blocks: &[BasicBlock]) -> Vec<usize> {
    blocks
        .iter()
        .filter_map(|block| match block.terminator.as_ref() {
            Some(Terminator::Call {
                callee: Callee::Static(func),
                ..
            }) => usize::try_from(func.0).ok(),
            _ => None,
        })
        .collect()
}

/// Function indices whose body may not be the one a `Callee::Static` naming
/// them actually runs.
///
/// MIR resolves a method call against the receiver's *static* type. When a
/// class hierarchy is involved the runtime class may override the method, so
/// the summary of the statically named body would not describe the code that
/// runs. Rather than model overriding, every method of every class that either
/// has a base or is used as a base is excluded from summary-based refinement.
fn overridable_methods(mir: &Mir) -> HashSet<usize> {
    let bases: HashSet<_> = mir.classes.iter().filter_map(|class| class.base).collect();
    let mut out = HashSet::new();
    for class in &mir.classes {
        if class.base.is_none() && !bases.contains(&class.name) {
            continue;
        }
        for method in class.methods.iter().chain(class.static_methods.iter()) {
            if let Ok(index) = usize::try_from(method.0) {
                out.insert(index);
            }
        }
    }
    out
}
