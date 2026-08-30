//! Emission of `throw` and of the `catch` binding that recovers its payload.
//!
//! Both the function-level terminator emitter (`control_flow.rs`) and the
//! reduced closure-body emitter (`closures.rs`) end a `Terminator::Throw` here so
//! the two paths cannot drift apart, and the `catch` side reads the payload back
//! through the same ABI. See `crate::thrown` for why the payload crosses the
//! error channel as a `SmeltUnknown`.

use super::*;
use crate::{stdlib, thrown};
use smelt_mir::ClosureId;

impl FunctionEmitter<'_> {
    /// Renders the `return Err(..)` statement for a `Terminator::Throw`.
    ///
    /// The thrown operand is erased to `SmeltUnknown` and handed to the
    /// payload-preserving `smelt_throw` adapter, so a `catch` can recover the
    /// value's class, `name`, `message` and custom fields instead of the
    /// `format!("{}", value)` text that previously replaced them (which rendered
    /// every erased `Error` as the literal `[object Object]`).
    ///
    /// The `Err` variant is annotated with the channel's error type so Rust never
    /// has to infer `E`: a throwing closure whose result is only stored, never
    /// called at a site that pins the error type, would otherwise leave `E`
    /// ambiguous (E0283).
    ///
    /// A program that never uses an erased value has no `SmeltUnknown` in its
    /// prelude, and therefore no payload adapter either; such a throw keeps the
    /// plain stringified `std::io::Error` form, which loses nothing because
    /// without `SmeltUnknown` there is no erased `catch` binding to observe a
    /// payload.
    ///
    /// The erasure goes through `erase_value_text` rather than `value_at_type`
    /// because `Type::Unknown` need not be interned in the type table even when
    /// the prelude defines `SmeltUnknown` (a program can reach `SmeltUnknown`
    /// through a runtime helper without ever naming an `unknown`-typed value), and
    /// requiring the id there would spuriously fail emission.
    pub(super) fn throw_terminator_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let payload = self.thrown_payload_text(operand)?;
        Ok(format!(
            "    return Err::<_, Box<dyn std::error::Error>>({payload});\n"
        ))
    }

    /// Renders the `Box<dyn std::error::Error>` an operand enters the error
    /// channel as.
    ///
    /// Shared by the `throw` terminator and `Promise.reject`, which are the same
    /// operation in JavaScript: both settle the error channel with an arbitrary
    /// value. Keeping one renderer means a rejection reason and a thrown value
    /// have identical fidelity — an erased program keeps the payload whole
    /// through `smelt_throw`, and a program with no erased values keeps the plain
    /// string `std::io::Error` form.
    pub(super) fn thrown_payload_text(&self, operand: &Operand) -> Result<String, EmitError> {
        if !stdlib::needs_unknown_type(self.mir) {
            return Ok(format!(
                "std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into()",
                self.operand_text(operand)?
            ));
        }
        let operand_ty = self.operand_ty(operand)?;
        let value_text = self.operand_text(operand)?;
        let erased = if matches!(self.mir.types.get(operand_ty), Some(Type::Unknown)) {
            value_text
        } else {
            self.erase_value_text(&value_text, operand_ty)?
        };
        Ok(thrown::throw_expr(&erased))
    }

    /// Renders the payload for a rejection with no reason (`Promise.reject()`),
    /// which JavaScript settles with `undefined`.
    pub(super) fn undefined_thrown_payload_text(&self) -> String {
        if stdlib::needs_unknown_type(self.mir) {
            return thrown::throw_expr("SmeltUnknown::Undefined");
        }
        "std::io::Error::new(std::io::ErrorKind::Other, \"undefined\").into()".to_owned()
    }

    /// Renders the `catch` binding for a caught `Box<dyn std::error::Error>`.
    ///
    /// `error_text` names the binding holding the caught error. An erased
    /// (`Unknown`-typed) catch parameter recovers the original thrown payload;
    /// a string-typed one keeps the error's `Display` text, which the payload
    /// ABI defines as the payload's `message` field when it has one.
    pub(super) fn caught_error_value_text(
        &self,
        exception_ty: TypeId,
        error_text: &str,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(exception_ty) {
            Some(Type::String) => Ok(format!("{error_text}.to_string()")),
            Some(Type::Unknown) => Ok(thrown::thrown_value_expr(error_text)),
            _ => self.default_value(exception_ty),
        }
    }

    /// Whether a local's assignment is folded into a `throw` expression.
    ///
    /// See [`folded_throw_payload_locals`] for what qualifies. Callers use this
    /// to suppress both the local's declaration and its assignment statement,
    /// because the value is rendered at the throw site instead.
    pub(super) fn is_folded_throw_payload(&self, local: LocalId) -> bool {
        self.folded_throw_payloads.contains(&local)
    }

    /// Renders a folded throw-payload local as the expression it was assigned.
    ///
    /// The local is written once, so its single assignment is unambiguous. The
    /// rvalue is rendered at the local's own declared type, exactly as the
    /// suppressed assignment statement would have rendered it, so folding
    /// changes where the expression appears and nothing about what it means.
    /// Nested folded locals resolve recursively because rendering an rvalue
    /// renders its operands through [`FunctionEmitter::operand_text`], which
    /// routes back here.
    pub(super) fn folded_throw_payload_text(
        &self,
        local: LocalId,
    ) -> Result<Option<String>, EmitError> {
        if !self.is_folded_throw_payload(local) {
            return Ok(None);
        }
        let ty = self.local_decl(local)?.ty;
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, value } = statement
                    && *dest == local
                {
                    return Ok(Some(self.rvalue_text_for_dest(value, ty)?));
                }
            }
        }
        Ok(None)
    }
}

/// Whether an rvalue is a *pure value construction* that may be folded into the
/// expression that consumes it.
///
/// Folding moves an rvalue from its own statement into its single consumer, so
/// it may only be applied to rvalues that (a) build a value out of already-read
/// operands, (b) have no side effect of their own, and (c) render the same text
/// whether or not they are being assigned to a named destination. The variants
/// listed here are the value constructors: a copy/move, the four collection
/// literals, a class instance, and the `unknown` erasure. Everything else —
/// calls, mutating list/set/map operations, generator steps, closure creation —
/// is deliberately excluded, because reordering it relative to its neighbours
/// or re-rendering it without a destination local is not obviously sound.
fn is_foldable_payload_rvalue(value: &Rvalue) -> bool {
    matches!(
        value,
        Rvalue::Use(_)
            | Rvalue::List(_)
            | Rvalue::Set(_)
            | Rvalue::Dict(_)
            | Rvalue::Tuple(_)
            | Rvalue::Struct { .. }
            | Rvalue::UnknownCast { .. }
    )
}

/// Count every read of every local in a function body.
///
/// Reads are counted through the canonical [`Rvalue::for_each_operand`] walk so
/// the tally cannot silently miss an rvalue variant, plus the phi incoming
/// operands, the place bases an assignment reads before writing, and the
/// terminator operands.
fn local_read_counts(function: &MirFunction) -> HashMap<LocalId, usize> {
    let mut counts: HashMap<LocalId, usize> = HashMap::new();
    fn count_place(counts: &mut HashMap<LocalId, usize>, place: &Place) {
        match place {
            Place::Local(local) | Place::Field { base: local, .. } => {
                *counts.entry(*local).or_default() += 1;
            }
            Place::Index { base, index } => {
                *counts.entry(*base).or_default() += 1;
                count_operand(counts, index);
            }
        }
    }
    fn count_operand(counts: &mut HashMap<LocalId, usize>, operand: &Operand) {
        if let Operand::Copy(place) | Operand::Move(place) = operand {
            count_place(counts, place);
        }
    }
    for block in &function.blocks {
        for phi in &block.phis {
            for (_, operand) in &phi.incoming {
                count_operand(&mut counts, operand);
            }
        }
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } => {
                    value.for_each_operand(|operand| count_operand(&mut counts, operand));
                }
                Statement::AssignPlace { place, value } => {
                    match place {
                        Place::Local(_) => {}
                        Place::Field { base, .. } => *counts.entry(*base).or_default() += 1,
                        Place::Index { base, index } => {
                            *counts.entry(*base).or_default() += 1;
                            count_operand(&mut counts, index);
                        }
                    }
                    value.for_each_operand(|operand| count_operand(&mut counts, operand));
                }
                // Same read tally as the `Index` place above, plus the seed.
                Statement::DictEntryUpdate {
                    base,
                    index,
                    default,
                    current: _,
                    value,
                } => {
                    let count = counts.entry(*base).or_default();
                    *count = count.saturating_add(1);
                    count_operand(&mut counts, index);
                    count_operand(&mut counts, default);
                    value.for_each_operand(|operand| count_operand(&mut counts, operand));
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        let Some(terminator) = &block.terminator else {
            continue;
        };
        match terminator {
            Terminator::Goto(_) | Terminator::Unreachable => {}
            Terminator::Call { callee, args, .. } => {
                if let Callee::Indirect(operand) = callee {
                    count_operand(&mut counts, operand);
                }
                for arg in args {
                    count_operand(&mut counts, arg);
                }
            }
            Terminator::Await { future, .. } => count_operand(&mut counts, future),
            Terminator::Switch { cond, .. } => count_operand(&mut counts, cond),
            Terminator::Match { scrutinee, .. } => count_operand(&mut counts, scrutinee),
            Terminator::Return(operand) | Terminator::Throw(operand) => {
                count_operand(&mut counts, operand);
            }
        }
    }
    counts
}

/// Locals captured by a closure that this function creates, directly or through
/// a nested closure.
///
/// `MirClosure` does not record its defining function, so ownership is
/// recovered from the `Rvalue::Closure` sites in the body: a function owns the
/// closures it constructs, and transitively those they construct in turn. The
/// walk is bounded by the visited set, so a closure table that refers to itself
/// cannot loop.
fn closure_captured_locals(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
    fn collect_closure_ids(blocks: &[BasicBlock], into: &mut Vec<ClosureId>) {
        for block in blocks {
            for statement in &block.statements {
                let (Statement::Assign { value, .. } | Statement::AssignPlace { value, .. }) =
                    statement
                else {
                    continue;
                };
                if let Rvalue::Closure { id, .. } = value {
                    into.push(*id);
                }
            }
        }
    }

    let mut pending = Vec::new();
    collect_closure_ids(&function.blocks, &mut pending);
    let mut visited = HashSet::new();
    let mut captured = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(closure) = mir
            .closures
            .iter()
            .find(|candidate| candidate.id == id)
        else {
            continue;
        };
        captured.extend(closure.captures.iter().map(|capture| capture.source_local));
        collect_closure_ids(&closure.blocks, &mut pending);
    }
    captured
}

/// Compiler temporaries that only exist to stage the payload of a `throw`.
///
/// A source `throw new Error(msg)` lowers to a record construction, an erasure
/// of that record to `SmeltUnknown`, and a `Terminator::Throw` reading the
/// erased temporary — MIR is three-address, so each step needs its own local.
/// Emitted verbatim that is five lines of Rust (two declarations, two
/// assignments and the `return Err(..)`) for one source-level statement, where
/// a team writing this Rust by hand would build the payload as one expression
/// at the throw site. Worse, the staged locals are bare `SmeltUnknown`
/// bindings that read as erasure in their own right even though they are only
/// the interior of the exception-payload boundary.
///
/// This returns the locals whose assignment can be folded into the throw
/// expression instead: the trailing run of statements in a block that ends in
/// `Terminator::Throw`, where each statement assigns a compiler temporary that
/// is written once, read once, and read only by a statement that is itself
/// being folded (or by the throw terminator). Because the folded statements are
/// a *contiguous suffix* of the block and every folded rvalue is a pure value
/// construction ([`is_foldable_payload_rvalue`]), moving them into the throw
/// expression cannot reorder them past anything observable.
///
/// The scope is deliberately the throw terminator and nothing else. The same
/// staging happens before ordinary `return`s and calls, and a general
/// single-use-temporary inliner would collapse those too, but it would also
/// rewrite essentially every generated function at once — including bindings
/// whose emitted text depends on having a named destination, and bindings whose
/// lifetime the borrow checker is currently relying on.
pub(super) fn folded_throw_payload_locals(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
    let mut folded = HashSet::new();
    if !function
        .blocks
        .iter()
        .any(|block| matches!(block.terminator, Some(Terminator::Throw(_))))
    {
        return folded;
    }
    let read_counts = local_read_counts(function);
    let mut assign_counts: HashMap<LocalId, usize> = HashMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, .. } => *assign_counts.entry(*dest).or_default() += 1,
                Statement::AssignPlace {
                    place: Place::Local(local),
                    ..
                } => *assign_counts.entry(*local).or_default() += 1,
                // The entry update defines the local it binds the entry to.
                Statement::DictEntryUpdate { current, .. } => {
                    let count = assign_counts.entry(*current).or_default();
                    *count = count.saturating_add(1);
                }
                _ => {}
            }
        }
        for phi in &block.phis {
            *assign_counts.entry(phi.dest).or_default() += 1;
        }
    }
    // A local captured by a closure is read through the closure environment
    // rather than through an operand, so the read tally above cannot see it.
    //
    // Only closures *this* function creates may be consulted: `source_local` is
    // a `LocalId`, which is meaningful only within its owning body, so scanning
    // the crate-wide closure table would let an unrelated function's capture of
    // its own local 12 mark this function's local 12 as captured. In a crate
    // with many closures that silently suppresses nearly every fold.
    let captured = closure_captured_locals(mir, function);
    let params = function.params.iter().copied().collect::<HashSet<_>>();

    for block in &function.blocks {
        let Some(Terminator::Throw(
            Operand::Copy(Place::Local(thrown)) | Operand::Move(Place::Local(thrown)),
        )) = &block.terminator
        else {
            continue;
        };
        // Locals the throw expression will read once the suffix is folded in.
        let mut wanted = HashSet::from([*thrown]);
        for statement in block.statements.iter().rev() {
            let Statement::Assign { dest, value } = statement else {
                break;
            };
            if !wanted.remove(dest)
                || !is_foldable_payload_rvalue(value)
                || params.contains(dest)
                || captured.contains(dest)
                || !matches!(
                    function
                        .locals
                        .get(id_index(dest.0, "local index").unwrap_or(usize::MAX)),
                    Some(LocalDecl {
                        kind: LocalKind::Temp,
                        ..
                    })
                )
                || assign_counts.get(dest).copied().unwrap_or(0) != 1
                || read_counts.get(dest).copied().unwrap_or(0) != 1
            {
                break;
            }
            folded.insert(*dest);
            value.for_each_operand(|operand| {
                if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                    operand
                {
                    wanted.insert(*local);
                }
            });
        }
    }
    folded
}
