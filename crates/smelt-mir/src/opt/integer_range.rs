//! Integer range analysis: retype provably integral `number` locals.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), the integer-range
//! reasoning of `src/sema/check/aot/integers.rs` ("the compiler works out the
//! range of a loop counter or an index by itself, and keeps it in a register as
//! an integer"). Bun proves ranges on its typed AST; Smelt proves them on MIR,
//! where every definition of a local is an explicit statement and every guard is
//! an explicit `switch` on a comparison.
//!
//! # What is retyped
//!
//! A TypeScript `number` local is an `f64`. A hand-written Rust port would make
//! a loop counter or an index a `usize` (or an `i64` when it can go negative),
//! so `arr[i]` indexes directly instead of round-tripping through `f64`. This
//! pass does that when it can PROVE the change is unobservable:
//!
//! * **Behavioral requirement.** Every definition of the local is an integer
//!   constant, a copy of another proven local, `a + b` / `a - b` of proven locals
//!   and integer constants, or a `.length` read; and interval analysis over the
//!   control-flow graph (with branch refinement on `<`, `<=`, `>`, `>=`, `==`,
//!   `!=` guards against proven locals, constants and lengths) bounds every
//!   definition to `[-2^53, 2^53]`, where `f64` addition and subtraction of
//!   integers are exact. Nothing else produces a value: no multiplication (the
//!   `-0` producer), no division, no unary minus, no call result, no parameter,
//!   no closure capture. Such a value is never `NaN`, `±Infinity`, `-0` or a
//!   fraction, so integer arithmetic agrees with `f64` arithmetic on it, and
//!   JavaScript prints it exactly as Rust prints the integer. A shape that is
//!   not provable keeps its `f64`; the pass never inserts a runtime check.
//! * **Type-preservation requirement.** The retyped local becomes
//!   [`Type::FixedInt`]: `usize` when every value of it and of every proven
//!   local it is compared or combined with is non-negative (an index counter,
//!   so `arr[i]` and `arr.len()` need no conversion at all), `i64` otherwise (a
//!   counter that steps down to `-1`). Wherever the local meets a `number`
//!   context — an argument, a return, `console.log`, string concatenation, a
//!   template literal, arithmetic with an unproven `number` — the pass inserts an
//!   explicit, exact `ToFloat` conversion temporary, so no consumer outside the
//!   integer-closed forms ever sees the integer type.
//!
//! The pass only runs on JavaScript-sourced bodies: a Python `float` prints
//! `1.0` where an integer prints `1`, so the "prints identically" half of the
//! behavioral requirement does not hold there.

use std::collections::{HashMap, HashSet};

use smelt_hir::{BinOp, IntWidth, PrimitiveCastOp, Type, TypeId, TypeInterner, UnaryOp};

use super::Pass;
use crate::{
    AbsentSpelling, BasicBlock, BlockId, Constant, GlobalProjection, LocalDecl, LocalId,
    LocalKind, Mir, Operand, Place, Rvalue, Statement, Terminator,
};

/// Largest magnitude at which every integer is exactly an `f64`.
const EXACT_LIMIT: i128 = 1 << 53;

/// Upper bound assumed for any `.length`.
///
/// JavaScript caps an array's length at `2^32 - 1`; a string or a Rust
/// collection that large cannot be built in practice. The proof only needs the
/// bound to keep derived values inside [`EXACT_LIMIT`], which `2^32` leaves a
/// `2^21`-fold margin for.
const MAX_LENGTH: i128 = (1 << 32) - 1;

/// Rounds after which a block's entry state is widened instead of joined.
const WIDEN_AFTER: usize = 3;

/// Descending (narrowing) rounds run after the widened fixpoint.
const NARROW_ROUNDS: usize = 3;

/// Hard cap on ascending rounds; widening converges long before it.
const MAX_ROUNDS: usize = 64;

/// MIR pass that retypes provably integral `number` locals to Rust integers.
///
/// See the module documentation for the behavioral and type-preservation
/// requirements it enforces.
#[derive(Debug, Default)]
pub struct IntegerRange;

impl Pass for IntegerRange {
    /// Returns the stable identifier of this pass.
    fn name(&self) -> &'static str {
        "integer-range"
    }

    /// Retypes proven locals in every JavaScript function and closure body.
    fn run(&self, mir: &mut Mir) -> bool {
        let Mir {
            functions,
            closures,
            types,
            ..
        } = mir;
        let mut changed = false;
        for function in functions.iter_mut() {
            if function.is_generator || function.absent != AbsentSpelling::Undefined {
                continue;
            }
            let excluded = function.params.iter().copied().collect();
            changed |= retype_body(
                &mut function.locals,
                &mut function.blocks,
                function.entry,
                excluded,
                types,
            );
        }
        for closure in closures.iter_mut() {
            if closure.is_generator || closure.absent != AbsentSpelling::Undefined {
                continue;
            }
            let mut excluded: HashSet<LocalId> = closure.params.iter().copied().collect();
            excluded.extend(closure.captures.iter().filter_map(|capture| capture.target_local));
            changed |= retype_body(
                &mut closure.locals,
                &mut closure.blocks,
                closure.entry,
                excluded,
                types,
            );
        }
        changed
    }
}

/// A closed integer interval; `None` on a side means unbounded on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    /// Smallest value, or `None` for negative infinity.
    lo: Option<i128>,
    /// Largest value, or `None` for positive infinity.
    hi: Option<i128>,
}

impl Interval {
    /// The interval holding exactly `value`.
    fn point(value: i128) -> Self {
        Self::new(Some(value), Some(value))
    }

    /// Builds an interval, dropping any bound beyond [`EXACT_LIMIT`].
    ///
    /// A finite bound is therefore always exact in `f64`, which is what lets
    /// refinement trust it.
    fn new(lo: Option<i128>, hi: Option<i128>) -> Self {
        let clamp = |bound: Option<i128>| bound.filter(|value| value.abs() <= EXACT_LIMIT);
        Self {
            lo: clamp(lo),
            hi: clamp(hi),
        }
    }

    /// The smallest interval containing both.
    fn join(self, other: Self) -> Self {
        Self {
            lo: self.lo.zip(other.lo).map(|(a, b)| a.min(b)),
            hi: self.hi.zip(other.hi).map(|(a, b)| a.max(b)),
        }
    }

    /// `self` widened by `next`, with thresholds.
    ///
    /// A bound that moved jumps to the nearest threshold beyond it (the
    /// constants of the body and the length bound, each with small offsets), or
    /// to infinity past the last one. Jumping to a threshold rather than straight
    /// to infinity keeps a counter that an OUTER loop bounds bounded at an inner
    /// loop head, which no descending round could recover afterwards: the inner
    /// loop's own cycle sustains whatever bound it was widened to. `thresholds`
    /// is sorted ascending.
    fn widen(self, next: Self, thresholds: &[i128]) -> Self {
        let lo = match (self.lo, next.lo) {
            (Some(old), Some(new)) if new >= old => Some(old),
            (Some(_), Some(new)) => thresholds.iter().rev().copied().find(|t| *t <= new),
            _ => None,
        };
        let hi = match (self.hi, next.hi) {
            (Some(old), Some(new)) if new <= old => Some(old),
            (Some(_), Some(new)) => thresholds.iter().copied().find(|t| *t >= new),
            _ => None,
        };
        Self { lo, hi }
    }

    /// Interval of `a + b`.
    fn add(self, other: Self) -> Self {
        Self::new(
            self.lo.zip(other.lo).map(|(a, b)| a + b),
            self.hi.zip(other.hi).map(|(a, b)| a + b),
        )
    }

    /// Interval of `a - b`.
    fn sub(self, other: Self) -> Self {
        Self::new(
            self.lo.zip(other.hi).map(|(a, b)| a - b),
            self.hi.zip(other.lo).map(|(a, b)| a - b),
        )
    }

    /// Whether both bounds are finite (and hence exact).
    const fn is_bounded(self) -> bool {
        self.lo.is_some() && self.hi.is_some()
    }

    /// Whether the interval holds no value.
    fn is_empty(self) -> bool {
        self.lo.zip(self.hi).is_some_and(|(lo, hi)| lo > hi)
    }

    /// Raises the lower bound to at least `bound`.
    fn at_least(self, bound: Option<i128>) -> Self {
        let lo = match (self.lo, bound) {
            (Some(current), Some(raised)) => Some(current.max(raised)),
            (None, raised) => raised,
            (current, None) => current,
        };
        Self { lo, hi: self.hi }
    }

    /// Lowers the upper bound to at most `bound`.
    fn at_most(self, bound: Option<i128>) -> Self {
        let hi = match (self.hi, bound) {
            (Some(current), Some(lowered)) => Some(current.min(lowered)),
            (None, lowered) => lowered,
            (current, None) => current,
        };
        Self { lo: self.lo, hi }
    }
}

/// Abstract value of every candidate local at one program point.
///
/// `None` for the whole state means the point is unreachable; `None` for one
/// candidate means it holds no value yet on any path reaching the point.
type State = Option<Vec<Option<Interval>>>;

/// Each candidate's definition sites.
type Definitions = HashMap<LocalId, Vec<Definition>>;

/// `(compared, base, offset)`: a compared local that equals `base + offset`.
type Offset = (LocalId, LocalId, i128);

/// A set of locals.
type LocalSet = HashSet<LocalId>;

/// Joins two candidate values.
fn join_value(left: Option<Interval>, right: Option<Interval>) -> Option<Interval> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.join(right)),
        (value, None) | (None, value) => value,
    }
}

/// Joins two states.
fn join_state(left: &State, right: &State) -> State {
    match (left, right) {
        (Some(left), Some(right)) => Some(
            left.iter()
                .zip(right)
                .map(|(left, right)| join_value(*left, *right))
                .collect(),
        ),
        (Some(state), None) | (None, Some(state)) => Some(state.clone()),
        (None, None) => None,
    }
}

/// Widens `old` by `next`, candidate by candidate.
fn widen_state(old: &State, next: &State, thresholds: &[i128]) -> State {
    match (old, next) {
        (Some(old), Some(next)) => Some(
            old.iter()
                .zip(next)
                .map(|(old, next)| match (old, next) {
                    (Some(old), Some(next)) => Some(old.widen(*next, thresholds)),
                    (value, None) | (None, value) => *value,
                })
                .collect(),
        ),
        (_, next) => next.clone(),
    }
}

/// The candidates of one body and their positions in a [`State`].
struct Candidates {
    /// Candidate locals in state order.
    locals: Vec<LocalId>,
    /// Position of each candidate in a state vector.
    index: HashMap<LocalId, usize>,
}

impl Candidates {
    /// Builds the index for `locals`.
    fn new(locals: Vec<LocalId>) -> Self {
        let index = locals
            .iter()
            .enumerate()
            .map(|(position, local)| (*local, position))
            .collect();
        Self { locals, index }
    }

    /// Whether `local` is a candidate.
    fn contains(&self, local: LocalId) -> bool {
        self.index.contains_key(&local)
    }
}

/// An integer constant usable in an integer-closed operation, if `constant` is one.
///
/// Excludes `-0`, fractions, non-finite values and anything beyond
/// [`EXACT_LIMIT`].
fn integral_constant(constant: &Constant) -> Option<i128> {
    match constant {
        Constant::Float(value)
            if value.is_finite()
                && value.fract() == 0.0
                && !(*value == 0.0 && value.is_sign_negative())
                && value.abs() <= 9_007_199_254_740_992.0 =>
        {
            // In range and integral, so the conversion is exact.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "value is integral and within 2^53, so it converts exactly"
            )]
            Some(*value as i128)
        }
        _ => None,
    }
}

/// The local an operand reads directly, if any.
const fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        _ => None,
    }
}

/// Whether `operand` is a candidate local or an integral constant.
fn is_integer_operand(operand: &Operand, candidates: &Candidates) -> bool {
    match operand {
        Operand::Const(constant) => integral_constant(constant).is_some(),
        other => operand_local(other).is_some_and(|local| candidates.contains(local)),
    }
}

/// Whether `op` is a comparison whose integer and `f64` meanings agree.
const fn is_comparison(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Lt
            | BinOp::Lte
            | BinOp::Gt
            | BinOp::Gte
            | BinOp::Eq
            | BinOp::NotEq
            | BinOp::StrictEq
            | BinOp::StrictNotEq
            | BinOp::JsStrictEq
            | BinOp::JsStrictNotEq
    )
}

/// Whether `value` is one of the integer-producing definition forms.
fn is_integer_definition(value: &Rvalue, candidates: &Candidates) -> bool {
    match value {
        Rvalue::Use(operand) => is_integer_operand(operand, candidates),
        Rvalue::Binary {
            op: BinOp::Add | BinOp::Sub,
            lhs,
            rhs,
        } => is_integer_operand(lhs, candidates) && is_integer_operand(rhs, candidates),
        Rvalue::Len(_) => true,
        _ => false,
    }
}

/// One definition site of a local inside a body.
#[derive(Debug, Clone, Copy)]
struct Definition {
    /// Block holding the definition.
    block: usize,
    /// Statement index inside the block.
    statement: usize,
}

/// Retypes the proven locals of one body. Returns whether anything changed.
fn retype_body(
    locals: &mut Vec<LocalDecl>,
    blocks: &mut [BasicBlock],
    entry: BlockId,
    mut excluded: HashSet<LocalId>,
    types: &mut TypeInterner,
) -> bool {
    let Some(float) = types.all().iter().position(|ty| *ty == Type::Float) else {
        return false;
    };
    let float = TypeId(u32::try_from(float).unwrap_or(u32::MAX));
    if blocks.iter().any(|block| !block.phis.is_empty()) {
        return false;
    }
    collect_escaping_locals(blocks, &mut excluded);
    let initial: Vec<LocalId> = locals
        .iter()
        .enumerate()
        .filter(|(_, decl)| decl.ty == float && !matches!(decl.kind, LocalKind::Param { .. }))
        .filter_map(|(index, _)| u32::try_from(index).ok().map(LocalId))
        .filter(|local| !excluded.contains(local))
        .collect();
    if initial.is_empty() {
        return false;
    }
    let (candidates, definitions) = definition_closed_candidates(blocks, initial);
    if candidates.locals.is_empty() {
        return false;
    }
    let user_bindings: Vec<bool> = candidates
        .locals
        .iter()
        .map(|local| {
            locals
                .get(local.0 as usize)
                .is_some_and(|decl| matches!(decl.kind, LocalKind::UserBinding(_)))
        })
        .collect();
    let analysis = Analysis {
        blocks,
        candidates: &candidates,
        entry: entry.0 as usize,
        user_bindings: &user_bindings,
        thresholds: widening_thresholds(blocks),
    };
    let entry_states = analysis.solve();
    let hulls = analysis.definition_hulls(&entry_states, &definitions);
    let accepted = accepted_locals(blocks, &candidates, &definitions, &hulls);
    if accepted.is_empty() {
        return false;
    }
    let (indexed, reassigned) = useful_locals(blocks, locals, types, &definitions);
    let stepped = stepped_locals(blocks, &definitions);
    let widths = choose_widths(
        blocks,
        &candidates,
        &accepted,
        &hulls,
        [&indexed, &reassigned, &stepped],
    );
    if widths.is_empty() {
        return false;
    }
    // The rewrite turns integral `Float` constants into `Constant::Int`, whose
    // operand type the emitter looks up as `Type::Int`.
    types.intern(Type::Int);
    let retyped: Vec<(LocalId, TypeId)> = widths
        .iter()
        .map(|(local, width)| (*local, types.intern(Type::FixedInt(*width))))
        .collect();
    let mut rewriter = Rewriter {
        locals,
        widths: &widths,
        float,
        types,
    };
    for block in blocks.iter_mut() {
        rewriter.rewrite_block(block);
    }
    for (local, ty) in retyped {
        if let Some(decl) = locals.get_mut(local.0 as usize) {
            decl.ty = ty;
        }
    }
    true
}

/// Candidates whose integer representation is what a hand-written port would use.
///
/// A group of connected proven locals is retyped only when one of them is a
/// list index (`xs[i]` then indexes with the integer directly) or when it is a
/// counter: some member is reassigned and some member is stepped with `+`/`-`
/// (carried around a loop as an integer). A constant that is only printed,
/// fed to `number` arithmetic, or merely reassigned another constant would
/// gain conversions and nothing else, so it keeps its `number` type.
///
/// Returns the list-indexed locals and the reassigned locals.
fn useful_locals(
    blocks: &[BasicBlock],
    locals: &[LocalDecl],
    types: &TypeInterner,
    definitions: &Definitions,
) -> (LocalSet, LocalSet) {
    let reassigned: HashSet<LocalId> = definitions
        .iter()
        .filter(|(_, sites)| sites.len() > 1)
        .map(|(local, _)| *local)
        .collect();
    let mut useful: HashSet<LocalId> = HashSet::new();
    let is_list = |base: LocalId| {
        locals
            .get(base.0 as usize)
            .is_some_and(|decl| matches!(types.get(decl.ty), Some(Type::List(_))))
    };
    let record_place = |place: &Place, useful: &mut HashSet<LocalId>| {
        if let Place::Index { base, index, .. } = place
            && is_list(*base)
            && let Some(local) = operand_local(index)
        {
            useful.insert(local);
        }
    };
    for block in blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } | Statement::DictEntryUpdate { value, .. } => {
                    value.for_each_operand(|operand| {
                        if let Operand::Copy(place) | Operand::Move(place) = operand {
                            record_place(place, &mut useful);
                        }
                    });
                }
                Statement::AssignPlace { place, value } => {
                    record_place(place, &mut useful);
                    value.for_each_operand(|operand| {
                        if let Operand::Copy(place) | Operand::Move(place) = operand {
                            record_place(place, &mut useful);
                        }
                    });
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        let terminator_operands: Vec<&Operand> = match &block.terminator {
            Some(Terminator::Call { args, .. }) => args.iter().collect(),
            Some(
                Terminator::Await { future: operand, .. }
                | Terminator::Switch { cond: operand, .. }
                | Terminator::Match {
                    scrutinee: operand, ..
                }
                | Terminator::Return(operand)
                | Terminator::Throw(operand),
            ) => vec![operand],
            _ => Vec::new(),
        };
        for operand in terminator_operands {
            if let Operand::Copy(place) | Operand::Move(place) = operand {
                record_place(place, &mut useful);
            }
        }
    }
    (useful, reassigned)
}

/// Candidates defined at least once as `a + b` or `a - b`.
fn stepped_locals(
    blocks: &[BasicBlock],
    definitions: &Definitions,
) -> HashSet<LocalId> {
    definitions
        .iter()
        .filter(|(_, sites)| {
            sites.iter().any(|site| {
                matches!(
                    &blocks[site.block].statements[site.statement],
                    Statement::Assign {
                        value: Rvalue::Binary { .. },
                        ..
                    } | Statement::AssignPlace {
                        value: Rvalue::Binary { .. },
                        ..
                    }
                )
            })
        })
        .map(|(local, _)| *local)
        .collect()
}

/// The widening thresholds of a body: `0`, the length bound and every integral
/// constant it mentions, each with offsets up to two either way.
fn widening_thresholds(blocks: &[BasicBlock]) -> Vec<i128> {
    let mut bases = vec![0, MAX_LENGTH];
    let mut record = |operand: &Operand| {
        if let Operand::Const(constant) = operand
            && let Some(value) = integral_constant(constant)
        {
            bases.push(value);
        }
    };
    for block in blocks {
        for statement in &block.statements {
            if let Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } = statement {
                value.for_each_operand(&mut record);
            }
        }
    }
    let mut thresholds: Vec<i128> = bases
        .into_iter()
        .flat_map(|base| (-2..=2).map(move |offset| base + offset))
        .filter(|value| value.abs() <= EXACT_LIMIT)
        .collect();
    thresholds.sort_unstable();
    thresholds.dedup();
    thresholds
}

/// Adds every local a closure captures, or a non-local definition binds, to `excluded`.
///
/// A captured local can be written by the closure where this analysis cannot
/// see it; a call result, an awaited value, a caught exception and a fused
/// dictionary entry are produced by code outside the integer-closed forms.
fn collect_escaping_locals(blocks: &[BasicBlock], excluded: &mut HashSet<LocalId>) {
    for block in blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                    if let Rvalue::Closure { captures, .. } = value {
                        excluded.extend(captures.iter().filter_map(operand_local));
                    }
                }
                Statement::DictEntryUpdate { current, value, .. } => {
                    excluded.insert(*current);
                    if let Rvalue::Closure { captures, .. } = value {
                        excluded.extend(captures.iter().filter_map(operand_local));
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        if let Some(Terminator::Call { dest, unwind, .. } | Terminator::Await { dest, unwind, .. }) =
            &block.terminator
        {
            excluded.insert(*dest);
            if let Some(local) = unwind.and_then(|handler| handler.exception_local) {
                excluded.insert(local);
            }
        }
    }
}

/// Narrows `initial` to locals whose every definition is an integer form over candidates.
///
/// Returns the surviving candidates together with each one's definition sites.
fn definition_closed_candidates(
    blocks: &[BasicBlock],
    initial: Vec<LocalId>,
) -> (Candidates, Definitions) {
    let mut current = initial;
    loop {
        let candidates = Candidates::new(current.clone());
        let mut definitions: Definitions = HashMap::new();
        let mut rejected = HashSet::new();
        for (block_index, block) in blocks.iter().enumerate() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                let (dest, value) = match statement {
                    Statement::Assign { dest, value }
                    | Statement::AssignPlace {
                        place: Place::Local(dest),
                        value,
                    } => (*dest, value),
                    _ => continue,
                };
                if !candidates.contains(dest) {
                    continue;
                }
                if is_integer_definition(value, &candidates) {
                    definitions.entry(dest).or_default().push(Definition {
                        block: block_index,
                        statement: statement_index,
                    });
                } else {
                    rejected.insert(dest);
                }
            }
        }
        if rejected.is_empty() {
            return (candidates, definitions);
        }
        current.retain(|local| !rejected.contains(local));
    }
}

/// The interval analysis of one body.
struct Analysis<'body> {
    /// The body's blocks.
    blocks: &'body [BasicBlock],
    /// Locals whose values are tracked.
    candidates: &'body Candidates,
    /// Entry block index.
    entry: usize,
    /// Per candidate: whether it is a user binding (implicitly `0` on entry).
    user_bindings: &'body [bool],
    /// Widening thresholds, sorted ascending; see [`Interval::widen`].
    thresholds: Vec<i128>,
}

impl Analysis<'_> {
    /// The state on entry to the body.
    ///
    /// MIR treats a user binding as defined on entry (it is declared with its
    /// type's default, `0` for a number), so its entry value is `[0, 0]`; a
    /// temporary holds nothing until its definition.
    fn initial_state(&self) -> Vec<Option<Interval>> {
        self.user_bindings
            .iter()
            .map(|is_binding| is_binding.then(|| Interval::point(0)))
            .collect()
    }

    /// The state that assumes nothing: every candidate may hold any integer.
    fn top_state(&self) -> Vec<Option<Interval>> {
        vec![
            Some(Interval {
                lo: None,
                hi: None
            });
            self.candidates.locals.len()
        ]
    }

    /// Blocks that an exception handler enters.
    ///
    /// Their entry state is forced to [`Self::top_state`]: a throw can leave its
    /// block between any two statements, so no single edge state describes it.
    fn handler_blocks(&self) -> HashSet<usize> {
        self.blocks
            .iter()
            .filter_map(|block| match &block.terminator {
                Some(
                    Terminator::Call {
                        unwind: Some(handler),
                        ..
                    }
                    | Terminator::Await {
                        unwind: Some(handler),
                        ..
                    },
                ) => Some(handler.catch_block.0 as usize),
                _ => None,
            })
            .collect()
    }

    /// Reverse post-order of the blocks reachable from the entry.
    fn reverse_post_order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        let mut visited = vec![false; self.blocks.len()];
        let mut stack = vec![(self.entry, false)];
        while let Some((block, expanded)) = stack.pop() {
            if expanded {
                order.push(block);
                continue;
            }
            if visited.get(block).copied().unwrap_or(true) {
                continue;
            }
            visited[block] = true;
            stack.push((block, true));
            if let Some(terminator) = &self.blocks[block].terminator {
                for successor in terminator.successors().into_iter().rev() {
                    let successor = successor.0 as usize;
                    if visited.get(successor) == Some(&false) {
                        stack.push((successor, false));
                    }
                }
            }
        }
        order.reverse();
        order
    }

    /// Computes every block's entry state: widened ascending iteration to a
    /// post-fixpoint, then a few descending rounds to recover loop bounds.
    fn solve(&self) -> Vec<State> {
        let order = self.reverse_post_order();
        let handlers = self.handler_blocks();
        let mut entry_states: Vec<State> = vec![None; self.blocks.len()];
        for round in 0..MAX_ROUNDS {
            let mut changed = false;
            for &block in &order {
                let incoming = self.incoming_state(block, &entry_states, &handlers);
                let next = if round >= WIDEN_AFTER {
                    widen_state(&entry_states[block], &incoming, &self.thresholds)
                } else {
                    join_state(&entry_states[block], &incoming)
                };
                if next != entry_states[block] {
                    entry_states[block] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            if round + 1 == MAX_ROUNDS {
                // Did not converge: assume nothing anywhere.
                return vec![Some(self.top_state()); self.blocks.len()];
            }
        }
        for _ in 0..NARROW_ROUNDS {
            for &block in &order {
                entry_states[block] = self.incoming_state(block, &entry_states, &handlers);
            }
        }
        // A block no edge reaches is still emitted, and could be entered in a
        // way MIR does not spell as an edge; assume nothing about it, so a
        // definition there is never trusted to be bounded.
        let reached: HashSet<usize> = order.iter().copied().collect();
        for (block, state) in entry_states.iter_mut().enumerate() {
            if !reached.contains(&block) {
                *state = Some(self.top_state());
            }
        }
        entry_states
    }

    /// Join of every edge state flowing into `block` under `entry_states`.
    fn incoming_state(
        &self,
        block: usize,
        entry_states: &[State],
        handlers: &HashSet<usize>,
    ) -> State {
        if handlers.contains(&block) {
            return Some(self.top_state());
        }
        let mut incoming = if block == self.entry {
            Some(self.initial_state())
        } else {
            None
        };
        for (predecessor, state) in entry_states.iter().enumerate() {
            if state.is_none() {
                continue;
            }
            for (successor, edge) in self.edge_states(predecessor, state) {
                if successor == block {
                    incoming = join_state(&incoming, &edge);
                }
            }
        }
        incoming
    }

    /// The state after `block`'s statements, starting from `state`.
    fn exit_state(&self, block: usize, state: &State) -> State {
        let mut state = state.clone()?;
        for statement in &self.blocks[block].statements {
            self.apply(statement, &mut state);
        }
        Some(state)
    }

    /// Applies one statement's effect on candidate values.
    fn apply(&self, statement: &Statement, state: &mut [Option<Interval>]) {
        let (dest, value) = match statement {
            Statement::Assign { dest, value }
            | Statement::AssignPlace {
                place: Place::Local(dest),
                value,
            } => (*dest, value),
            _ => return,
        };
        if let Some(&position) = self.candidates.index.get(&dest) {
            state[position] = self.evaluate(value, state);
        }
    }

    /// Interval of an integer definition form under `state`.
    fn evaluate(&self, value: &Rvalue, state: &[Option<Interval>]) -> Option<Interval> {
        match value {
            Rvalue::Use(operand) => self.operand_interval(operand, state),
            Rvalue::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => Some(
                self.operand_interval(lhs, state)?
                    .add(self.operand_interval(rhs, state)?),
            ),
            Rvalue::Binary {
                op: BinOp::Sub,
                lhs,
                rhs,
            } => Some(
                self.operand_interval(lhs, state)?
                    .sub(self.operand_interval(rhs, state)?),
            ),
            Rvalue::Len(_) => Some(Interval::new(Some(0), Some(MAX_LENGTH))),
            _ => Some(Interval {
                lo: None,
                hi: None,
            }),
        }
    }

    /// Interval of a candidate local or integral constant; `None` when the
    /// local holds nothing yet. Anything else is unbounded.
    fn operand_interval(&self, operand: &Operand, state: &[Option<Interval>]) -> Option<Interval> {
        if let Operand::Const(constant) = operand {
            return Some(integral_constant(constant).map_or(
                Interval {
                    lo: None,
                    hi: None,
                },
                Interval::point,
            ));
        }
        match operand_local(operand).and_then(|local| self.candidates.index.get(&local)) {
            Some(&position) => state[position],
            None => Some(Interval {
                lo: None,
                hi: None,
            }),
        }
    }

    /// The state along each outgoing edge of `block`.
    ///
    /// A `switch` on a comparison of candidates and constants refines both
    /// edges; an edge whose refinement is empty is unreachable.
    fn edge_states(&self, block: usize, entry_state: &State) -> Vec<(usize, State)> {
        let exit = self.exit_state(block, entry_state);
        let Some(terminator) = &self.blocks[block].terminator else {
            return Vec::new();
        };
        if let Terminator::Switch {
            cond,
            then_block,
            else_block,
        } = terminator
            && let Some(guard) = self.guard(block, cond)
        {
            return vec![
                (then_block.0 as usize, self.refine(&exit, &guard, true)),
                (else_block.0 as usize, self.refine(&exit, &guard, false)),
            ];
        }
        terminator
            .successors()
            .into_iter()
            .map(|successor| (successor.0 as usize, exit.clone()))
            .collect()
    }

    /// The comparison a `switch` condition tests, normalized so `true` means `lhs op rhs`.
    ///
    /// The condition must be defined in the same block by a comparison (through
    /// any number of `!`), and neither compared operand may be redefined between
    /// that comparison and the `switch`.
    fn guard(&self, block: usize, cond: &Operand) -> Option<Guard> {
        let statements = &self.blocks[block].statements;
        let mut target = operand_local(cond)?;
        let mut negated = false;
        let mut cursor = statements.len();
        loop {
            let (position, value) = statements[..cursor]
                .iter()
                .enumerate()
                .rev()
                .find_map(|(position, statement)| match statement {
                    Statement::Assign { dest, value } if *dest == target => {
                        Some((position, value))
                    }
                    Statement::AssignPlace {
                        place: Place::Local(dest),
                        value,
                    } if *dest == target => Some((position, value)),
                    _ => None,
                })?;
            match value {
                Rvalue::Unary {
                    op: UnaryOp::Not,
                    operand,
                } => {
                    target = operand_local(operand)?;
                    negated = !negated;
                    cursor = position;
                }
                Rvalue::Binary { op, lhs, rhs }
                    if is_comparison(*op)
                        && is_integer_operand(lhs, self.candidates)
                        && is_integer_operand(rhs, self.candidates) =>
                {
                    let compared: Vec<LocalId> =
                        [lhs, rhs].into_iter().filter_map(operand_local).collect();
                    let redefined = statements[position + 1..].iter().any(|statement| {
                        matches!(statement,
                            Statement::Assign { dest, .. }
                            | Statement::AssignPlace { place: Place::Local(dest), .. }
                                if compared.contains(dest))
                    });
                    if redefined {
                        return None;
                    }
                    let op = if negated { negate(*op) } else { *op };
                    let offsets = compared
                        .iter()
                        .filter_map(|local| self.offset_of(&statements[..position], *local))
                        .filter(|(_, base, _)| {
                            !statements.iter().skip(position).any(|statement| {
                                matches!(statement,
                                    Statement::Assign { dest, .. }
                                    | Statement::AssignPlace { place: Place::Local(dest), .. }
                                        if dest == base)
                            })
                        })
                        .collect();
                    return Some(Guard {
                        op,
                        lhs: lhs.clone(),
                        rhs: rhs.clone(),
                        offsets,
                    });
                }
                _ => return None,
            }
        }
    }

    /// `(compared, base, offset)` when `compared` was last defined in
    /// `statements` as `base + offset` (or `base - c`) for a candidate `base`
    /// that the rest of `statements` does not redefine.
    fn offset_of(&self, statements: &[Statement], compared: LocalId) -> Option<Offset> {
        let (position, value) = statements.iter().enumerate().rev().find_map(
            |(position, statement)| match statement {
                Statement::Assign { dest, value }
                | Statement::AssignPlace {
                    place: Place::Local(dest),
                    value,
                } if *dest == compared => Some((position, value)),
                _ => None,
            },
        )?;
        let (base, offset) = match value {
            Rvalue::Binary {
                op: BinOp::Add,
                lhs,
                rhs: Operand::Const(constant),
            }
            | Rvalue::Binary {
                op: BinOp::Add,
                lhs: Operand::Const(constant),
                rhs: lhs,
            } => (operand_local(lhs)?, integral_constant(constant)?),
            Rvalue::Binary {
                op: BinOp::Sub,
                lhs,
                rhs: Operand::Const(constant),
            } => (operand_local(lhs)?, -integral_constant(constant)?),
            _ => return None,
        };
        if !self.candidates.contains(base) {
            return None;
        }
        let redefined = statements[position + 1..].iter().any(|statement| {
            matches!(statement,
                Statement::Assign { dest, .. }
                | Statement::AssignPlace { place: Place::Local(dest), .. } if *dest == base)
        });
        (!redefined).then_some((compared, base, offset))
    }

    /// `state` restricted to the values where the guard's comparison is `sense`.
    ///
    /// A compared temporary that is `base + offset` also bounds `base`, so
    /// `i + 1 < xs.length` bounds `i` itself.
    fn refine(&self, state: &State, guard: &Guard, sense: bool) -> State {
        let mut state = state.clone()?;
        let (lhs, rhs) = (&guard.lhs, &guard.rhs);
        let op = if sense { guard.op } else { negate(guard.op) };
        let (Some(left), Some(right)) = (
            self.operand_interval(lhs, &state),
            self.operand_interval(rhs, &state),
        ) else {
            return Some(state);
        };
        let one = Some(1);
        let minus = |bound: Option<i128>| bound.zip(one).map(|(a, b)| a - b);
        let plus = |bound: Option<i128>| bound.zip(one).map(|(a, b)| a + b);
        let (left, right) = match op {
            BinOp::Lt => (left.at_most(minus(right.hi)), right.at_least(plus(left.lo))),
            BinOp::Lte => (left.at_most(right.hi), right.at_least(left.lo)),
            BinOp::Gt => (left.at_least(plus(right.lo)), right.at_most(minus(left.hi))),
            BinOp::Gte => (left.at_least(right.lo), right.at_most(left.hi)),
            BinOp::Eq | BinOp::StrictEq | BinOp::JsStrictEq => {
                let both = left.at_least(right.lo).at_most(right.hi);
                (both, both)
            }
            BinOp::NotEq | BinOp::StrictNotEq | BinOp::JsStrictNotEq => {
                (exclude_point(left, right), exclude_point(right, left))
            }
            _ => (left, right),
        };
        if left.is_empty() || right.is_empty() {
            return None;
        }
        for (operand, refined) in [(lhs, left), (rhs, right)] {
            if let Some(&position) =
                operand_local(operand).and_then(|local| self.candidates.index.get(&local))
            {
                state[position] = Some(refined);
            }
        }
        for (compared, base, offset) in &guard.offsets {
            let (Some(&compared), Some(&base)) = (
                self.candidates.index.get(compared),
                self.candidates.index.get(base),
            ) else {
                continue;
            };
            let (Some(compared), Some(current)) = (state[compared], state[base]) else {
                continue;
            };
            let shifted = |bound: Option<i128>| bound.map(|value| value - offset);
            let refined = current
                .at_least(shifted(compared.lo))
                .at_most(shifted(compared.hi));
            if refined.is_empty() {
                return None;
            }
            state[base] = Some(refined);
        }
        Some(state)
    }

    /// Per candidate: the join of the intervals of all its reachable definitions.
    ///
    /// A user binding also includes its implicit entry value `0`.
    fn definition_hulls(
        &self,
        entry_states: &[State],
        definitions: &Definitions,
    ) -> HashMap<LocalId, Interval> {
        let mut hulls = HashMap::new();
        for (position, local) in self.candidates.locals.iter().enumerate() {
            let mut hull = self.user_bindings[position].then(|| Interval::point(0));
            for definition in definitions.get(local).into_iter().flatten() {
                let Some(mut state) = entry_states[definition.block].clone() else {
                    continue;
                };
                let statements = &self.blocks[definition.block].statements;
                for statement in &statements[..definition.statement] {
                    self.apply(statement, &mut state);
                }
                let value = match &statements[definition.statement] {
                    Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                        self.evaluate(value, &state)
                    }
                    _ => None,
                };
                hull = join_value(hull, value);
            }
            if let Some(hull) = hull {
                hulls.insert(*local, hull);
            }
        }
        hulls
    }
}

/// A `switch` condition resolved to the comparison it tests.
struct Guard {
    /// The comparison, already negated for any `!` between it and the switch.
    op: BinOp,
    /// Left compared operand.
    lhs: Operand,
    /// Right compared operand.
    rhs: Operand,
    /// `(compared, base, offset)`: a compared local that equals `base + offset`.
    offsets: Vec<Offset>,
}

/// Shrinks `value` by one at a bound equal to the single value `other` holds.
fn exclude_point(value: Interval, other: Interval) -> Interval {
    let (Some(lo), Some(hi)) = (other.lo, other.hi) else {
        return value;
    };
    if lo != hi {
        return value;
    }
    let mut value = value;
    if value.lo == Some(lo) {
        value.lo = Some(lo + 1);
    }
    if value.hi == Some(lo) {
        value.hi = Some(lo - 1);
    }
    value
}

/// The comparison that holds exactly when `op` does not (operands are never `NaN`).
const fn negate(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gte,
        BinOp::Lte => BinOp::Gt,
        BinOp::Gt => BinOp::Lte,
        BinOp::Gte => BinOp::Lt,
        BinOp::Eq => BinOp::NotEq,
        BinOp::NotEq => BinOp::Eq,
        BinOp::StrictEq => BinOp::StrictNotEq,
        BinOp::StrictNotEq => BinOp::StrictEq,
        BinOp::JsStrictEq => BinOp::JsStrictNotEq,
        BinOp::JsStrictNotEq => BinOp::JsStrictEq,
        other => other,
    }
}

/// Candidates whose every definition is bounded in the exact range and reads only accepted locals.
fn accepted_locals(
    blocks: &[BasicBlock],
    candidates: &Candidates,
    definitions: &Definitions,
    hulls: &HashMap<LocalId, Interval>,
) -> HashSet<LocalId> {
    let mut accepted: HashSet<LocalId> = candidates
        .locals
        .iter()
        .copied()
        .filter(|local| hulls.get(local).is_some_and(|hull| hull.is_bounded()))
        .collect();
    loop {
        let rejected: Vec<LocalId> = accepted
            .iter()
            .copied()
            .filter(|local| {
                definitions.get(local).into_iter().flatten().any(|definition| {
                    let mut reads_rejected = false;
                    if let Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } =
                        &blocks[definition.block].statements[definition.statement]
                        && !matches!(value, Rvalue::Len(_))
                    {
                        value.for_each_operand(|operand| {
                            if let Some(read) = operand_local(operand) {
                                reads_rejected |= !accepted.contains(&read);
                            }
                        });
                    }
                    reads_rejected
                })
            })
            .collect();
        if rejected.is_empty() {
            return accepted;
        }
        for local in rejected {
            accepted.remove(&local);
        }
    }
}

/// Union-find root of `local`.
fn find(parents: &mut HashMap<LocalId, LocalId>, local: LocalId) -> LocalId {
    let parent = *parents.entry(local).or_insert(local);
    if parent == local {
        return local;
    }
    let root = find(parents, parent);
    parents.insert(local, root);
    root
}

/// Merges the union-find groups of `left` and `right`.
fn union(parents: &mut HashMap<LocalId, LocalId>, left: LocalId, right: LocalId) {
    let (left, right) = (find(parents, left), find(parents, right));
    parents.insert(left, right);
}

/// Chooses each accepted local's Rust width.
///
/// Locals that are combined (`a + b`, a copy) or compared with each other share
/// one width so every integer-closed operation is between equal Rust types. The
/// group is `usize` when every value of every member is non-negative and no
/// member is compared with a negative constant, else `i64`. Groups that are
/// neither list-indexed nor counters (see [`useful_locals`]) are left out.
fn choose_widths(
    blocks: &[BasicBlock],
    candidates: &Candidates,
    accepted: &HashSet<LocalId>,
    hulls: &HashMap<LocalId, Interval>,
    [indexed, reassigned, stepped]: [&HashSet<LocalId>; 3],
) -> HashMap<LocalId, IntWidth> {
    let mut parents: HashMap<LocalId, LocalId> = HashMap::new();
    let mut signed_roots: HashSet<LocalId> = HashSet::new();
    let mut signed_members: Vec<LocalId> = accepted
        .iter()
        .copied()
        .filter(|local| hulls.get(local).is_some_and(|hull| hull.lo.is_none_or(|lo| lo < 0)))
        .collect();
    for block in blocks {
        for statement in &block.statements {
            let (dest, value) = match statement {
                Statement::Assign { dest, value }
                | Statement::AssignPlace {
                    place: Place::Local(dest),
                    value,
                } => (*dest, value),
                _ => continue,
            };
            if accepted.contains(&dest) {
                if !matches!(value, Rvalue::Len(_)) {
                    value.for_each_operand(|operand| {
                        if let Some(read) = operand_local(operand) {
                            union(&mut parents, dest, read);
                        }
                    });
                }
                continue;
            }
            if let Some((lhs, rhs)) = accepted_comparison(value, candidates, accepted) {
                let locals: Vec<LocalId> = [&lhs, &rhs]
                    .into_iter()
                    .filter_map(operand_local)
                    .collect();
                if let [left, right] = locals[..] {
                    union(&mut parents, left, right);
                }
                let negative_constant = [&lhs, &rhs].into_iter().any(|operand| {
                    matches!(operand, Operand::Const(constant)
                        if integral_constant(constant).is_some_and(|value| value < 0))
                });
                if negative_constant {
                    signed_members.extend(locals);
                }
            }
        }
    }
    for local in signed_members {
        let root = find(&mut parents, local);
        signed_roots.insert(root);
    }
    let mut roots_of = |set: &HashSet<LocalId>| -> HashSet<LocalId> {
        set.iter()
            .filter(|local| accepted.contains(local))
            .map(|local| find(&mut parents, *local))
            .collect()
    };
    let stepped_roots = roots_of(stepped);
    let mut useful_roots = roots_of(indexed);
    useful_roots.extend(
        roots_of(reassigned)
            .into_iter()
            .filter(|root| stepped_roots.contains(root)),
    );
    let members: Vec<(LocalId, LocalId)> = accepted
        .iter()
        .map(|local| (*local, find(&mut parents, *local)))
        .collect();
    members
        .into_iter()
        .filter(|(_, root)| useful_roots.contains(root))
        .map(|(local, root)| {
            let width = if signed_roots.contains(&root) {
                IntWidth::I64
            } else {
                IntWidth::Usize
            };
            (local, width)
        })
        .collect()
}

/// The operands of a comparison that stays integer-closed after retyping.
///
/// Both sides must be accepted locals or integral constants, and at least one
/// side an accepted local.
fn accepted_comparison(
    value: &Rvalue,
    candidates: &Candidates,
    accepted: &HashSet<LocalId>,
) -> Option<(Operand, Operand)> {
    let Rvalue::Binary { op, lhs, rhs } = value else {
        return None;
    };
    if !is_comparison(*op) || !is_integer_operand(lhs, candidates) || !is_integer_operand(rhs, candidates)
    {
        return None;
    }
    let side_ok =
        |operand: &Operand| operand_local(operand).is_none_or(|local| accepted.contains(&local));
    let any_local = [lhs, rhs]
        .into_iter()
        .any(|operand| operand_local(operand).is_some());
    (side_ok(lhs) && side_ok(rhs) && any_local).then(|| (lhs.clone(), rhs.clone()))
}

/// Rewrites one body after the widths are chosen.
struct Rewriter<'body> {
    /// The body's locals; conversion temporaries are appended.
    locals: &'body mut Vec<LocalDecl>,
    /// Retyped locals and their widths.
    widths: &'body HashMap<LocalId, IntWidth>,
    /// The interned `Float` type.
    float: TypeId,
    /// The crate's types, to recognize list bases.
    types: &'body TypeInterner,
}

impl Rewriter<'_> {
    /// Whether `local` was retyped.
    fn is_retyped(&self, local: LocalId) -> bool {
        self.widths.contains_key(&local)
    }

    /// Whether `operand` is a retyped local or an integral constant.
    fn is_integer_side(&self, operand: &Operand) -> bool {
        match operand {
            Operand::Const(constant) => integral_constant(constant).is_some(),
            other => operand_local(other).is_some_and(|local| self.is_retyped(local)),
        }
    }

    /// Whether `value` compares retyped locals and integral constants only
    /// (at least one local), so it stays an integer comparison.
    fn is_integer_comparison(&self, value: &Rvalue) -> bool {
        matches!(value, Rvalue::Binary { op, lhs, rhs }
            if is_comparison(*op)
                && self.is_integer_side(lhs)
                && self.is_integer_side(rhs)
                && (operand_local(lhs).is_some() || operand_local(rhs).is_some()))
    }

    /// Rewrites every statement and the terminator of `block`.
    fn rewrite_block(&mut self, block: &mut BasicBlock) {
        let statements = std::mem::take(&mut block.statements);
        let mut rewritten = Vec::with_capacity(statements.len());
        for mut statement in statements {
            let mut prelude = Vec::new();
            self.rewrite_statement(&mut statement, &mut prelude);
            rewritten.extend(prelude);
            rewritten.push(statement);
        }
        if let Some(terminator) = &mut block.terminator {
            let mut prelude = Vec::new();
            self.rewrite_terminator(terminator, &mut prelude);
            rewritten.extend(prelude);
        }
        block.statements = rewritten;
    }

    /// Rewrites one statement, pushing any conversion it needs onto `prelude`.
    fn rewrite_statement(&mut self, statement: &mut Statement, prelude: &mut Vec<Statement>) {
        match statement {
            Statement::Assign { dest, value }
            | Statement::AssignPlace {
                place: Place::Local(dest),
                value,
            } if self.is_retyped(*dest) => {
                let width = self.widths[dest];
                if matches!(value, Rvalue::Len(_)) {
                    // The measured value is a container, but it may be an
                    // element read whose index needs the usual treatment.
                    self.rewrite_rvalue(value, prelude);
                } else {
                    Self::rewrite_integer_definition(value, width);
                }
            }
            Statement::Assign { value, .. } if self.is_integer_comparison(value) => {
                if let Rvalue::Binary { lhs, rhs, .. } = value {
                    integer_constant_operand(lhs);
                    integer_constant_operand(rhs);
                }
            }
            Statement::Assign { value, .. } => self.rewrite_rvalue(value, prelude),
            Statement::AssignPlace { place, value } => {
                self.rewrite_place(place, prelude);
                self.rewrite_rvalue(value, prelude);
            }
            Statement::DictEntryUpdate {
                index,
                default,
                value,
                ..
            } => {
                self.rewrite_number_operand(index, prelude);
                self.rewrite_number_operand(default, prelude);
                self.rewrite_rvalue(value, prelude);
            }
            Statement::StorageLive(_) | Statement::StorageDead(_) => {}
        }
    }

    /// Rewrites an integer definition: constants become integer constants, and
    /// on a `usize` a negative constant addend becomes the opposite operation.
    fn rewrite_integer_definition(value: &mut Rvalue, width: IntWidth) {
        match value {
            Rvalue::Use(operand) => integer_constant_operand(operand),
            Rvalue::Binary { op, lhs, rhs } => {
                if width == IntWidth::Usize
                    && let Some(magnitude) = negative_constant_magnitude(rhs)
                {
                    *op = if *op == BinOp::Add { BinOp::Sub } else { BinOp::Add };
                    *rhs = Operand::Const(Constant::Int(magnitude));
                } else if width == IntWidth::Usize
                    && *op == BinOp::Add
                    && let Some(magnitude) = negative_constant_magnitude(lhs)
                {
                    *op = BinOp::Sub;
                    *lhs = std::mem::replace(rhs, Operand::Const(Constant::Int(magnitude)));
                }
                integer_constant_operand(lhs);
                integer_constant_operand(rhs);
            }
            _ => {}
        }
    }

    /// Rewrites every operand of a non-integer rvalue.
    fn rewrite_rvalue(&mut self, value: &mut Rvalue, prelude: &mut Vec<Statement>) {
        value.for_each_operand_mut(|operand| self.rewrite_number_operand(operand, prelude));
    }

    /// Rewrites one operand in a `number` context.
    ///
    /// A retyped local is replaced by a fresh `Float` temporary holding its
    /// exact conversion; an element read keeps a retyped index on a list.
    fn rewrite_number_operand(&mut self, operand: &mut Operand, prelude: &mut Vec<Statement>) {
        match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                if self.is_retyped(*local) =>
            {
                let converted = self.converted(*local, prelude);
                *operand = Operand::Move(Place::Local(converted));
            }
            Operand::Copy(place) | Operand::Move(place) => self.rewrite_place(place, prelude),
            Operand::Const(_) => {}
        }
    }

    /// Rewrites the index operand inside a place.
    ///
    /// A list element index is an integer context (the emitter turns a
    /// `FixedInt` index straight into a `usize`); any other indexed container
    /// (a dictionary keyed by `number`, a global) gets the `f64` it expects.
    fn rewrite_place(&mut self, place: &mut Place, prelude: &mut Vec<Statement>) {
        match place {
            Place::Index { base, index, .. } => {
                let base_is_list = self.base_is_list(*base);
                match index.as_mut() {
                    Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                        if self.is_retyped(*local) && base_is_list => {}
                    other => self.rewrite_number_operand(other, prelude),
                }
            }
            Place::Global {
                projection: GlobalProjection::Index { index, .. },
                ..
            } => self.rewrite_number_operand(index, prelude),
            Place::Local(_) | Place::Field { .. } | Place::Global { .. } => {}
        }
    }

    /// Whether `base` is a list local.
    fn base_is_list(&self, base: LocalId) -> bool {
        self.locals
            .get(base.0 as usize)
            .is_some_and(|decl| matches!(self.types.get(decl.ty), Some(Type::List(_))))
    }

    /// Rewrites the operands a terminator reads in `number` contexts.
    fn rewrite_terminator(&mut self, terminator: &mut Terminator, prelude: &mut Vec<Statement>) {
        match terminator {
            Terminator::Call { args, callee, .. } => {
                if let crate::Callee::Indirect(operand) = callee {
                    self.rewrite_number_operand(operand, prelude);
                }
                for arg in args {
                    self.rewrite_number_operand(arg, prelude);
                }
            }
            Terminator::Await { future: operand, .. }
            | Terminator::Switch { cond: operand, .. }
            | Terminator::Match {
                scrutinee: operand, ..
            }
            | Terminator::Return(operand)
            | Terminator::Throw(operand) => self.rewrite_number_operand(operand, prelude),
            Terminator::Goto(_) | Terminator::Unreachable => {}
        }
    }

    /// A fresh `Float` temporary assigned the exact conversion of `local`.
    fn converted(&mut self, local: LocalId, prelude: &mut Vec<Statement>) -> LocalId {
        let span = self.locals[local.0 as usize].span;
        let temp = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(LocalDecl {
            ty: self.float,
            kind: LocalKind::Temp,
            span,
        });
        prelude.push(Statement::Assign {
            dest: temp,
            value: Rvalue::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand: Operand::Copy(Place::Local(local)),
            },
        });
        temp
    }
}

/// Replaces an integral `Float` constant operand by the equal `Int` constant.
fn integer_constant_operand(operand: &mut Operand) {
    if let Operand::Const(constant) = operand
        && let Some(value) = integral_constant(constant)
        && let Ok(value) = i64::try_from(value)
    {
        *constant = Constant::Int(value);
    }
}

/// The magnitude of a negative integral constant operand.
fn negative_constant_magnitude(operand: &Operand) -> Option<i64> {
    let Operand::Const(constant) = operand else {
        return None;
    };
    let value = match constant {
        Constant::Int(value) => i128::from(*value),
        other => integral_constant(other)?,
    };
    (value < 0).then(|| i64::try_from(-value).ok()).flatten()
}

#[cfg(test)]
mod tests {
    //! Behavioral and type-preservation checks for [`super::IntegerRange`].
    //!
    //! Each positive test pins the retyped local's width and the integer-closed
    //! forms it appears in; each negative test pins a shape the analysis must
    //! NOT prove, so it keeps its `f64`.

    use crate::{format_compact, lower_hir, opt, validate};
    use smelt_frontend_ts::{HirCtx, to_hir};
    use smelt_hir::FileId;

    /// Lowers `source` to optimized, validated MIR text.
    fn optimized(source: &str) -> String {
        let mut ctx = HirCtx::new();
        to_hir(source, FileId(0), &mut ctx)
            .unwrap_or_else(|error| panic!("HIR lowering failed: {error:?}"));
        let mut mir =
            lower_hir(&ctx.krate).unwrap_or_else(|error| panic!("MIR lowering failed: {error:?}"));
        opt::optimize(&mut mir);
        let errors = validate(&mir);
        assert!(errors.is_empty(), "invalid MIR: {errors:?}");
        format_compact(&mir)
    }

    /// The declared type of the user binding `name` in the MIR text of `function`.
    fn binding_type(output: &str, function: &str, name: &str) -> String {
        let header = format!("fn {function} ");
        let body = output
            .split("\n\n")
            .find(|chunk| chunk.trim_start().starts_with(&header))
            .unwrap_or_else(|| panic!("no function {function} in:\n{output}"));
        let needle = format!(" user {name}: ");
        let line = body
            .lines()
            .find(|line| line.contains(&needle))
            .unwrap_or_else(|| panic!("no binding {name} in {function}:\n{body}"));
        line.split(&needle).nth(1).unwrap_or_default().trim().to_owned()
    }

    #[test]
    fn index_counter_bounded_by_length_is_usize() {
        let output = optimized(
            "function total(xs: number[]): number {\n  let s = 0;\n  for (let i = 0; i < xs.length; i++) {\n    s += xs[i];\n  }\n  return s;\n}\nconsole.log(total([1, 2]));\n",
        );
        assert_eq!(binding_type(&output, "total", "i"), "usize", "{output}");
        assert_eq!(binding_type(&output, "total", "s"), "Float", "{output}");
        // The index is the integer itself, and the length is compared as one.
        assert!(output.contains("copy %0[copy %2]"), "{output}");
        assert!(output.contains("= copy %2 < move %3"), "{output}");
        assert!(output.contains("= move %2 + 1\n"), "{output}");
        assert!(!output.contains("primitive_cast_float copy %2"), "{output}");
    }

    #[test]
    fn counter_stepping_down_to_minus_one_is_i64() {
        let output = optimized(
            "function rev(xs: string[]): string {\n  let out = \"\";\n  for (let i = xs.length - 1; i >= 0; i--) {\n    out += xs[i];\n  }\n  return out;\n}\nconsole.log(rev([\"a\"]));\n",
        );
        assert_eq!(binding_type(&output, "rev", "i"), "i64", "{output}");
    }

    #[test]
    fn offset_guard_bounds_the_base_counter() {
        let output = optimized(
            "function pairs(xs: number[]): number {\n  let t = 0;\n  for (let i = 0; i + 1 < xs.length; i++) {\n    t += xs[i] * xs[i + 1];\n  }\n  return t;\n}\nconsole.log(pairs([1, 2, 3]));\n",
        );
        assert_eq!(binding_type(&output, "pairs", "i"), "usize", "{output}");
    }

    #[test]
    fn nested_counters_and_steps_greater_than_one() {
        let output = optimized(
            "function grid(rows: number[][]): number {\n  let acc = 0;\n  for (let r = 0; r < rows.length; r++) {\n    const row = rows[r];\n    for (let c = 0; c < row.length; c += 2) {\n      acc += row[c];\n    }\n  }\n  return acc;\n}\nconsole.log(grid([[1]]));\n",
        );
        assert_eq!(binding_type(&output, "grid", "r"), "usize", "{output}");
        assert_eq!(binding_type(&output, "grid", "c"), "usize", "{output}");
    }

    #[test]
    fn number_context_uses_get_an_exact_conversion() {
        let output = optimized("for (let i = 0; i < 3; i++) {\n  console.log(i, \"i=\" + i);\n}\n");
        assert_eq!(binding_type(&output, "main", "i"), "usize", "{output}");
        assert!(
            output.contains("primitive_cast_float copy %0"),
            "a number context reads the counter through ToFloat:\n{output}"
        );
    }

    #[test]
    fn unguarded_increment_stays_float() {
        let output = optimized(
            "function f(limit: number): number {\n  let i = 0;\n  while (true) {\n    i++;\n    if (i > limit) {\n      break;\n    }\n  }\n  return i;\n}\nconsole.log(f(3));\n",
        );
        assert_eq!(binding_type(&output, "f", "i"), "Float", "{output}");
    }

    #[test]
    fn multiplication_by_a_negative_stays_float() {
        let output = optimized(
            "function f(): number {\n  let sign = 1;\n  for (let k = 0; k < 4; k++) {\n    sign *= -1;\n  }\n  return sign;\n}\nconsole.log(f());\n",
        );
        assert_eq!(binding_type(&output, "f", "sign"), "Float", "{output}");
        assert_eq!(binding_type(&output, "f", "k"), "usize", "{output}");
    }

    /// `-1 * 0` is `-0` in JavaScript (`console.log` prints `-0`), which no
    /// integer can hold: a product of proven counters must not be proven.
    #[test]
    fn product_that_can_be_negative_zero_stays_float() {
        let output = optimized(
            "let z = 0;\nfor (let k = 0; k < 3; k++) {\n  const m = k - 1;\n  z = m * k;\n  console.log(z);\n}\n",
        );
        assert_eq!(binding_type(&output, "main", "z"), "Float", "{output}");
        assert_eq!(binding_type(&output, "main", "k"), "i64", "{output}");
    }

    #[test]
    fn division_stays_float() {
        let output = optimized(
            "function f(): number {\n  let h = 64;\n  while (h > 1) {\n    h = h / 2;\n  }\n  return h;\n}\nconsole.log(f());\n",
        );
        assert_eq!(binding_type(&output, "f", "h"), "Float", "{output}");
    }

    #[test]
    fn closure_captured_counter_stays_float() {
        let output = optimized(
            "function f(xs: number[]): number {\n  let seen = 0;\n  let i = 0;\n  while (i < xs.length) {\n    xs.forEach(() => {\n      seen = seen + i;\n    });\n    i++;\n  }\n  return seen;\n}\nconsole.log(f([1]));\n",
        );
        assert_eq!(binding_type(&output, "f", "i"), "Float", "{output}");
    }

    #[test]
    fn counter_from_an_arbitrary_number_stays_float() {
        let output = optimized(
            "function f(start: number, xs: number[]): number {\n  let i = start;\n  let acc = 0;\n  while (i < xs.length) {\n    acc += xs[i];\n    i++;\n  }\n  return acc;\n}\nconsole.log(f(0, [1]));\n",
        );
        assert_eq!(binding_type(&output, "f", "i"), "Float", "{output}");
    }

    #[test]
    fn guard_against_an_unproven_number_stays_float() {
        let output = optimized(
            "function f(n: number): number {\n  let i = 0;\n  while (i < n) {\n    i++;\n  }\n  return i;\n}\nconsole.log(f(2.5));\n",
        );
        assert_eq!(binding_type(&output, "f", "i"), "Float", "{output}");
    }

    #[test]
    fn reassigned_constant_without_a_step_keeps_its_number_type() {
        let output = optimized("let n = 1;\nn = 6;\nconsole.log(n);\n");
        assert_eq!(binding_type(&output, "main", "n"), "Float", "{output}");
    }

    #[test]
    fn constant_that_is_only_printed_keeps_its_number_type() {
        let output = optimized("let count = 42;\nconsole.log(count);\n");
        assert_eq!(binding_type(&output, "main", "count"), "Float", "{output}");
    }
}
