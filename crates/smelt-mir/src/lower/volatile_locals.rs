//! Which locals can change while one expression is being evaluated.
//!
//! [`LoweringCtx::lower_expr`] answers a local read with the place itself, and
//! the statement that consumes the operand reads it later, after every sibling
//! operand has been lowered. For most locals nothing can happen in between. For
//! these it can, and JavaScript fixes the value where the read is evaluated, so
//! `pair(x, bump())` with a `bump` that writes `x` passes the old `x`:
//!
//! * a local a closure writes, at any nesting depth: any later call can run it;
//! * in a closure body, a capture of such a local, since a sibling closure holds
//!   the same cell;
//! * a local an expression writes in place: `pair(a, a++)`, where the write is a
//!   statement of an [`ExprKind::Block`] operand.
//!
//! Reads of these are snapshotted into a temporary at the point of evaluation.
//! The optimizer's [`TempCopyForwarding`](crate::opt::TempCopyForwarding) removes
//! every snapshot nothing can invalidate, so this costs nothing when the operands
//! around it are plain.
//!
//! A field or element read is the same lazy place, but any call can reach the
//! object through another name, so no set of locals answers it. Such a read is
//! snapshotted when a LATER sibling operand can run code: `pair(c.n, inc())`. A
//! field nothing in the crate can write cannot change, so its read stays a
//! place; a keyed write with a non-literal key (`o[k] = v`) or an
//! `Object.assign` can write any field of a receiver of the target's type (of
//! any receiver when the target is wholly erased).
//! A property read on a string-keyed record of erased values can itself run a
//! getter, so it counts as running code and is snapshotted ahead of a later
//! sibling that runs code or reads a module global.
//!
//! [`LoweringCtx::lower_expr`]: super::context::LoweringCtx

use std::collections::{HashMap, HashSet};

use smelt_hir::{Body, BodyId, ClosureExpr, ExprId, ExprKind, LocalId, Stmt, Type, TypeInterner};

/// The volatile locals of every HIR body; see the module docs.
#[derive(Debug, Default)]
pub(super) struct VolatileLocals {
    /// Volatile locals by owning body.
    by_body: HashMap<BodyId, HashSet<LocalId>>,
    /// Field and element reads a later sibling can invalidate, by owning body.
    early_reads: HashMap<BodyId, HashSet<ExprId>>,
}

impl VolatileLocals {
    /// Computes the volatile locals of every body of `krate`.
    pub(super) fn of(krate: &smelt_hir::Crate) -> Self {
        let bodies: HashMap<BodyId, &Body> = krate.bodies.iter().map(|body| (body.id, body)).collect();
        let written = closure_written_captures(&krate.bodies, &bodies);
        let mut by_body: HashMap<BodyId, HashSet<LocalId>> = HashMap::new();
        for body in &krate.bodies {
            let mut volatile = written_in_expression(body);
            for closure in closures_of(body) {
                let writes = written.get(&closure.body);
                volatile.extend(
                    closure
                        .captures
                        .iter()
                        .filter(|capture| capture.body_local.is_some_and(|local| writes.is_some_and(|set| set.contains(&local))))
                        .map(|capture| capture.source_local),
                );
            }
            by_body.insert(body.id, volatile);
        }
        spread_into_captures(&krate.bodies, &mut by_body);
        let assigned_fields = assigned_fields(krate);
        let early_reads =
            krate.bodies.iter().map(|body| (body.id, early_reads(body, krate, &assigned_fields))).collect();
        Self { by_body, early_reads }
    }

    /// Whether `local` of `body` is volatile.
    pub(super) fn contains(&self, body: BodyId, local: LocalId) -> bool {
        self.by_body.get(&body).is_some_and(|locals| locals.contains(&local))
    }

    /// Whether `expr` of `body` is a read a later sibling can invalidate.
    pub(super) fn is_early_read(&self, body: BodyId, expr: ExprId) -> bool {
        self.early_reads.get(&body).is_some_and(|reads| reads.contains(&expr))
    }
}

/// The direct child expressions of `kind`, in evaluation order.
fn children(kind: &ExprKind) -> Vec<ExprId> {
    let mut found = Vec::new();
    let mapped = kind.clone().try_map_child_exprs(&mut |child| {
        found.push(child);
        Ok::<_, std::convert::Infallible>(child)
    });
    drop(mapped);
    found
}

/// The expression at `id` of `body`.
fn expr_kind(body: &Body, id: ExprId) -> Option<&ExprKind> {
    body.exprs.get(usize::try_from(id.0).ok()?).map(|expr| &expr.kind)
}

/// Whether `kind` itself runs source code, suspends, or stores in place.
const fn runs_code_or_stores(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Call { .. }
            | ExprKind::ClosureCall { .. }
            | ExprKind::ClosureCallSpread { .. }
            | ExprKind::ClosureCallRest { .. }
            | ExprKind::Method { .. }
            | ExprKind::Construct { .. }
            | ExprKind::ConstructLike { .. }
            | ExprKind::New { .. }
            | ExprKind::Block(_)
            | ExprKind::Await(_)
            | ExprKind::AsyncOp { .. }
            | ExprKind::GeneratorYield { .. }
            | ExprKind::GlobalSet { .. }
            | ExprKind::ListClear { .. }
            | ExprKind::ListExtend { .. }
            | ExprKind::ListInsert { .. }
            | ExprKind::ListPop { .. }
            | ExprKind::ListPush { .. }
            | ExprKind::ListRemove { .. }
            | ExprKind::ListReverse { .. }
            | ExprKind::ListShift { .. }
            | ExprKind::ListSort { .. }
            | ExprKind::ListUnshift { .. }
            | ExprKind::ListSplice { .. }
            | ExprKind::ListFill { .. }
            | ExprKind::ListCopyWithin { .. }
            | ExprKind::DictSet { .. }
            | ExprKind::DictAssign { .. }
            | ExprKind::CallableObjectAssign { .. }
    )
}

/// Whether evaluating `id` can run source code or store, given what `memo` knows:
/// it does so itself, a child does, or it is handed a function it can call.
fn can_run_code(body: &Body, types: &TypeInterner, id: ExprId, memo: &mut HashMap<ExprId, bool>) -> bool {
    if let Some(known) = memo.get(&id) {
        return *known;
    }
    let Some(kind) = expr_kind(body, id) else {
        return false;
    };
    let takes_function = |child: &ExprId| {
        usize::try_from(child.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .is_some_and(|expr| matches!(types.get(expr.ty), Some(Type::Function(_))))
    };
    let kids = children(kind);
    let answer = runs_code_or_stores(kind)
        || read_may_run_accessor(body, types, kind)
        || (!matches!(kind, ExprKind::ListLit(_) | ExprKind::DictLit(_) | ExprKind::TupleLit(_))
            && kids.iter().any(takes_function))
        || kids.into_iter().any(|child| can_run_code(body, types, child, memo));
    memo.insert(id, answer);
    answer
}

/// Whether `kind` is a property read that can run a getter: a field or element
/// read on a string-keyed record of erased values, where an object literal
/// stores its `get` and the read calls it. A read on a wholly erased value is
/// left lazy, as it was: snapshotting it would add an erased temporary to
/// every such read for the rare erased object that carries a getter.
fn read_may_run_accessor(body: &Body, types: &TypeInterner, kind: &ExprKind) -> bool {
    let (ExprKind::Field { receiver, .. } | ExprKind::Index { receiver, .. }) = kind else {
        return false;
    };
    let Some(receiver_expr) = usize::try_from(receiver.0).ok().and_then(|index| body.exprs.get(index)) else {
        return false;
    };
    match types.get(receiver_expr.ty) {
        Some(Type::Dict(key, value)) => {
            matches!(types.get(*key), Some(Type::String | Type::PyStr)) && matches!(types.get(*value), Some(Type::Unknown))
        }
        _ => false,
    }
}

/// Whether evaluating `id` reads a module global, itself or in a child.
///
/// `memo` holds the answer for every expression already asked, so asking it of
/// each expression of a body visits every expression once instead of once per
/// enclosing expression.
fn reads_global(body: &Body, id: ExprId, memo: &mut HashMap<ExprId, bool>) -> bool {
    if let Some(known) = memo.get(&id) {
        return *known;
    }
    let answer = expr_kind(body, id).is_some_and(|kind| {
        matches!(kind, ExprKind::GlobalGet { .. })
            || children(kind).into_iter().any(|child| reads_global(body, child, memo))
    });
    memo.insert(id, answer);
    answer
}

/// Which fields the statements of the crate can write.
#[derive(Debug, Default)]
struct FieldWrites {
    /// Fields written by name: `o.n = v`, or `o["n"] = v` with a literal key.
    names: HashSet<String>,
    /// Receiver types a write whose field is not known statically
    /// (`o[key] = v`, `Object.assign(o, src)`) can reach: any field of a value
    /// of such a type can change.
    any_field_of: HashSet<smelt_hir::TypeId>,
    /// Such a write into a wholly erased value, which can be any object.
    any_field_of_anything: bool,
}

impl FieldWrites {
    /// Whether the field named `name` of a receiver of type `receiver` can be
    /// written.
    fn may_write(&self, types: &TypeInterner, receiver: smelt_hir::TypeId, name: &str) -> bool {
        self.any_field_of_anything
            || self.names.contains(name)
            || self.any_field_of.contains(&receiver)
            || matches!(types.get(receiver), Some(Type::Unknown)) && !self.any_field_of.is_empty()
    }

    /// Records a write of unknown field into a receiver of type `receiver`.
    fn any_field(&mut self, types: &TypeInterner, receiver: smelt_hir::TypeId) {
        if matches!(types.get(receiver), Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))) {
            self.any_field_of_anything = true;
        } else {
            self.any_field_of.insert(receiver);
        }
    }
}

/// Every field a statement or `Object.assign` of the crate can write.
fn assigned_fields(krate: &smelt_hir::Crate) -> FieldWrites {
    let mut writes = FieldWrites::default();
    let ty_of = |body: &Body, id: ExprId| usize::try_from(id.0).ok().and_then(|index| body.exprs.get(index)).map(|expr| expr.ty);
    for body in &krate.bodies {
        for stmt in &body.stmts {
            let Stmt::Assign { target, .. } = stmt else { continue };
            match expr_kind(body, *target) {
                Some(ExprKind::Field { field, .. }) => {
                    if let Some(name) = krate.symbols.get(*field) {
                        writes.names.insert(name.to_owned());
                    }
                }
                Some(ExprKind::Index { receiver, index }) if receiver_has_fields(body, &krate.types, *receiver) => {
                    match expr_kind(body, *index) {
                        Some(ExprKind::Literal(smelt_hir::Literal::String(name))) => {
                            writes.names.insert(name.clone());
                        }
                        _ => {
                            if let Some(ty) = ty_of(body, *receiver) {
                                writes.any_field(&krate.types, ty);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for expr in &body.exprs {
            if let ExprKind::DictAssign { target, .. } = expr.kind
                && let Some(ty) = ty_of(body, target)
            {
                writes.any_field(&krate.types, ty);
            }
        }
    }
    writes
}

/// Whether a keyed write into `receiver` can land on a named field: anything
/// but a list, tuple or typed array, whose keys are element indices.
fn receiver_has_fields(body: &Body, types: &TypeInterner, receiver: ExprId) -> bool {
    usize::try_from(receiver.0)
        .ok()
        .and_then(|index| body.exprs.get(index))
        .is_none_or(|expr| !matches!(types.get(expr.ty), Some(Type::List(_) | Type::Tuple(_))))
}

/// The field and element reads of `body` followed by a sibling that can run code.
fn early_reads(body: &Body, krate: &smelt_hir::Crate, assigned_fields: &FieldWrites) -> HashSet<ExprId> {
    let types = &krate.types;
    let mut memo = HashMap::new();
    let mut global_memo = HashMap::new();
    let mut reads = HashSet::new();
    for expr in &body.exprs {
        let siblings = children(&expr.kind);
        let mut later_runs_code = false;
        let mut later_reads_global = false;
        for &child in siblings.iter().rev() {
            // A read that can run a getter happens where it is evaluated: ahead
            // of a later sibling that runs code, or that reads a module global
            // (a global read is taken at its own point, so a lazy read placed
            // after it would run the getter too late).
            if (later_runs_code || later_reads_global)
                && expr_kind(body, child).is_some_and(|kind| read_may_run_accessor(body, types, kind))
            {
                reads.insert(child);
            }
            later_reads_global |= reads_global(body, child, &mut global_memo);
            let is_lazy_read = match expr_kind(body, child) {
                Some(ExprKind::Field { receiver, field }) => {
                    let receiver_ty = usize::try_from(receiver.0).ok().and_then(|index| body.exprs.get(index)).map(|receiver_expr| receiver_expr.ty);
                    krate.symbols.get(*field).is_some_and(|name| {
                        receiver_ty.is_some_and(|ty| assigned_fields.may_write(types, ty, name))
                    })
                }
                Some(ExprKind::Index { .. }) => true,
                _ => false,
            };
            // A callable read stays a place: the call it feeds decides the ABI and
            // the throw analysis from it, as for every function-typed local.
            let is_callable = usize::try_from(child.0)
                .ok()
                .and_then(|index| body.exprs.get(index))
                .is_some_and(|read| matches!(types.get(read.ty), Some(Type::Function(_))));
            if later_runs_code && is_lazy_read && !is_callable {
                reads.insert(child);
            }
            later_runs_code |= can_run_code(body, types, child, &mut memo);
        }
    }
    reads
}

/// Every closure expression of `body`.
fn closures_of(body: &Body) -> impl Iterator<Item = &ClosureExpr> {
    body.exprs.iter().filter_map(|expr| match &expr.kind {
        ExprKind::Closure(closure) => Some(closure),
        _ => None,
    })
}

/// The local a statement of `body` assigns as a whole, if it does.
fn assigned_local(body: &Body, stmt: &Stmt) -> Option<LocalId> {
    let Stmt::Assign { target, .. } = stmt else {
        return None;
    };
    let index = usize::try_from(target.0).ok()?;
    match body.exprs.get(index)?.kind {
        ExprKind::Local(local) => Some(local),
        _ => None,
    }
}

/// Every local `body` assigns as a whole, anywhere.
fn assigned_locals(body: &Body) -> HashSet<LocalId> {
    body.stmts.iter().filter_map(|stmt| assigned_local(body, stmt)).collect()
}

/// The locals assigned by a block that is an operand of another expression.
///
/// A block with no parent expression is a whole statement, `x = 1;`, with no
/// sibling to be read around.
fn written_in_expression(body: &Body) -> HashSet<LocalId> {
    let operands: HashSet<ExprId> = body.exprs.iter().flat_map(|expr| children(&expr.kind)).collect();
    operands
        .into_iter()
        .filter_map(|id| match expr_kind(body, id)? {
            ExprKind::Block(block) => body.blocks.get(usize::try_from(block.0).ok()?),
            _ => None,
        })
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| body.stmts.get(usize::try_from(stmt.0).ok()?))
        .filter_map(|stmt| assigned_local(body, stmt))
        .collect()
}

/// Per closure body, the capture locals a call of it can write: assigned there,
/// or written by a closure it creates. Runs to a fixpoint, since a write can sit
/// any number of closures deep.
fn closure_written_captures(
    all: &[Body],
    bodies: &HashMap<BodyId, &Body>,
) -> HashMap<BodyId, HashSet<LocalId>> {
    let mut written: HashMap<BodyId, HashSet<LocalId>> =
        all.iter().map(|body| (body.id, assigned_locals(body))).collect();
    loop {
        let mut changed = false;
        for body in all {
            for closure in closures_of(body) {
                let Some(inner) = bodies.get(&closure.body).and_then(|inner| written.get(&inner.id)) else {
                    continue;
                };
                let reached: Vec<LocalId> = closure
                    .captures
                    .iter()
                    .filter(|capture| capture.body_local.is_some_and(|local| inner.contains(&local)))
                    .map(|capture| capture.source_local)
                    .collect();
                if let Some(outer) = written.get_mut(&body.id) {
                    for local in reached {
                        changed |= outer.insert(local);
                    }
                }
            }
        }
        if !changed {
            return written;
        }
    }
}

/// Marks a closure's capture volatile when the local it captures is. Runs to a
/// fixpoint because a capture can be captured again.
fn spread_into_captures(all: &[Body], by_body: &mut HashMap<BodyId, HashSet<LocalId>>) {
    loop {
        let mut changed = false;
        for body in all {
            for closure in closures_of(body) {
                let reached: Vec<LocalId> = closure
                    .captures
                    .iter()
                    .filter(|capture| by_body.get(&body.id).is_some_and(|set| set.contains(&capture.source_local)))
                    .filter_map(|capture| capture.body_local)
                    .collect();
                let inner = by_body.entry(closure.body).or_default();
                for local in reached {
                    changed |= inner.insert(local);
                }
            }
        }
        if !changed {
            return;
        }
    }
}
