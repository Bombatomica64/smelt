//! Local dataflow and mutability analysis: whether a MIR local needs a mutable Rust binding, its assignment/use ordering, and rvalue-level in-place mutation and mutable-borrow queries.

use super::core::terminator_successors;
use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// The function's [`LocalWriteIndex`], built on first use.
    pub(super) fn local_write_index(&self) -> &LocalWriteIndex {
        self.caches
            .local_write_index
            .get_or_init(|| self.build_local_write_index())
    }

    /// Scan every block once and record the write facts the per-local
    /// mutability queries need (see [`LocalWriteIndex`]).
    fn build_local_write_index(&self) -> LocalWriteIndex {
        let mut index = LocalWriteIndex::default();
        for block in &self.function.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::Assign { dest, value } => {
                        if let Rvalue::Closure { id, .. } = value
                            && let Some(closure) = self
                                .mir
                                .closures
                                .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                        {
                            index.shared_capture_locals.extend(
                                closure
                                    .captures
                                    .iter()
                                    .filter(|capture| {
                                        // A binding that can be unassigned when
                                        // captured is shared with the closure,
                                        // which reads the same `SmeltBinding`.
                                        self.closure_capture_needs_shared_access(closure, capture)
                                            || self
                                                .function
                                                .locals
                                                .get(capture.source_local.index())
                                                .is_some_and(|decl| {
                                                    decl.init == smelt_mir::LocalInit::MaybeUnbound
                                                })
                                    })
                                    .map(|capture| capture.source_local),
                            );
                        }
                        bump_u32(&mut index.writes, *dest);
                        index.last_write_block.insert(*dest, block.id);
                        if let Rvalue::Use(source) = value {
                            index
                                .use_sources
                                .entry(*dest)
                                .or_default()
                                .push(source.clone());
                        }
                        bump_usize(&mut index.assignments, *dest);
                        index.assign_blocks.entry(*dest).or_default().push(block.id);
                        // The rvalue's own mutation of a local is only judged
                        // for OTHER locals: a statement whose destination is
                        // the local itself is decided by the
                        // predeclared-in-a-loop rule alone.
                        for mutated in self.rvalue_mutable_locals(value) {
                            if mutated != *dest {
                                index.mutated.insert(mutated);
                            }
                        }
                    }
                    Statement::AssignPlace { place, .. } => {
                        if let Place::Local(local) = place {
                            bump_u32(&mut index.writes, *local);
                            index.last_write_block.insert(*local, block.id);
                            bump_usize(&mut index.assignments, *local);
                        }
                        if let Place::Local(candidate)
                        | Place::Field {
                            base: candidate, ..
                        }
                        | Place::Index {
                            base: candidate, ..
                        } = place
                        {
                            index.mutated.insert(*candidate);
                        }
                    }
                    // The container is borrowed mutably through its entry
                    // accessor (`SmeltJsMap::entry_or_insert` takes `&mut
                    // self`), exactly like an `Index` assignment target. The
                    // bound `current` follows the same predeclared-in-a-loop
                    // rule as a single `Assign` destination.
                    Statement::DictEntryUpdate { base, current, .. } => {
                        bump_u32(&mut index.writes, *current);
                        index.last_write_block.insert(*current, block.id);
                        bump_usize(&mut index.assignments, *current);
                        index
                            .assign_blocks
                            .entry(*current)
                            .or_default()
                            .push(block.id);
                        if self.dict_entry_update_needs_mut_base(*base) {
                            index.mutated.insert(*base);
                        }
                    }
                }
            }
            if let Some(
                Terminator::Call {
                    unwind: Some(handler),
                    ..
                }
                | Terminator::Await {
                    unwind: Some(handler),
                    ..
                },
            ) = &block.terminator
                && let Some(local) = handler.exception_local
            {
                index.unwind_written.insert(local);
            }
            if let Some(Terminator::Call {
                callee: Callee::Op(op, Some(_)),
                ..
            }) = &block.terminator
            {
                for mutated in self.rvalue_mutable_locals(op) {
                    index.mutated.insert(mutated);
                }
            }
            if let Some(Terminator::Call {
                callee: Callee::Static(function_id),
                args,
                ..
            }) = &block.terminator
                && let Some(callee) = self
                    .mir
                    .functions
                    .get(usize::try_from(function_id.0).unwrap_or(usize::MAX))
            {
                for (arg, param) in args.iter().zip(&callee.params) {
                    if let Some(local) = arg.local()
                        && self.parameter_needs_mutable_reference_in(callee, *param)
                    {
                        index.mutated.insert(local);
                    }
                }
            }
        }
        index.repeat_reachable = repeat_reachable_blocks(self.function);
        index
    }

    /// Test hook: the indexed repeat-reachable blocks next to the per-block
    /// reference predicates' answer, both sorted.
    #[cfg(test)]
    pub(crate) fn repeat_reachable_against_reference(
        &self,
    ) -> (Vec<smelt_mir::BlockId>, Vec<smelt_mir::BlockId>) {
        let mut indexed: Vec<_> = self
            .local_write_index()
            .repeat_reachable
            .iter()
            .copied()
            .collect();
        indexed.sort_by_key(|block| block.0);
        let reference = self
            .function
            .blocks
            .iter()
            .map(|block| block.id)
            .filter(|block| {
                self.block_can_repeat(*block, &mut BlockIdSet::default())
                    || self.block_is_reached_from_repeating_region(*block)
            })
            .collect();
        (indexed, reference)
    }

    /// The locals an rvalue mutates in place, borrows mutably, or captures
    /// into a closure that needs shared access to them.
    fn rvalue_mutable_locals(&self, value: &Rvalue) -> Vec<LocalId> {
        let mut locals = Vec::new();
        if let Some(root) = mutated_binding_root(value) {
            locals.push(root);
        }
        match value {
            Rvalue::DictAssign { target, .. } | Rvalue::ListCallback { list: target, .. } => {
                if let Some(local) = target.local()
                    && self.rvalue_borrows_local_mutably(value, local)
                {
                    locals.push(local);
                }
            }
            Rvalue::Closure { id, .. } => {
                if let Some(closure) = self
                    .mir
                    .closures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                {
                    locals.extend(
                        closure
                            .captures
                            .iter()
                            .filter(|capture| {
                                self.closure_capture_needs_shared_access(closure, capture)
                            })
                            .map(|capture| capture.source_local),
                    );
                }
            }
            _ => {}
        }
        locals
    }

    /// Returns whether the first Rust binding for `local` must be mutable.
    pub(super) fn local_binding_needs_mut(&self, local: LocalId) -> bool {
        // A parameter emitted as `&mut T` needs a mutable binding when it is
        // reborrowed into another mutable callback. Other structural values do
        // not need unconditional `mut`; the assignment and mutation analysis
        // below handles their actual writes.
        if self.function.params.contains(&local) && self.parameter_needs_mutable_reference(local) {
            return true;
        }
        if self.predeclared_locals.contains(&local)
            && (self.predeclared_local_needs_default(local).unwrap_or(true)
                || self
                    .local_may_be_used_before_assignment(local)
                    .unwrap_or(true))
        {
            return true;
        }
        if self.local_may_be_assigned_after_assignment(local) {
            return true;
        }
        if self.local_assignment_count(local) > 1 {
            return true;
        }
        let index = self.local_write_index();
        // Written by a statement AND by an unwind edge: a `finally`'s thrown
        // value is initialized before its `try` and overwritten when a throw
        // reaches the handler. (A `catch` binding is written by its unwind
        // edges alone, which all bind it once in their own handler code.)
        if index.unwind_written.contains(&local) && self.local_assignment_count(local) > 0 {
            return true;
        }
        // Written in place, borrowed mutably, captured for shared access, or
        // passed to a parameter emitted as `&mut T` (see `LocalWriteIndex`).
        if index.mutated.contains(&local) {
            return true;
        }
        // The repeating-region rule only forces `mut` when the binding's `let`
        // lives OUTSIDE the repeating region and is therefore reassigned across
        // iterations. A binding that reaches this point is assigned exactly
        // once (multi-assignment locals already returned `true` above via
        // `local_assignment_count`), so the only shape that still needs `mut`
        // is a *predeclared* local — one hoisted to function scope, whose `let`
        // sits outside the loop. A non-predeclared local instead emits its
        // `let` inline at the assignment site, re-running as a fresh binding
        // each iteration, and needs no `mut`.
        //
        // `block_is_reached_from_repeating_region` is deliberately kept
        // alongside `block_can_repeat`: the structured emitter can place a
        // MIR-diverging assignment textually inside `loop { .. }`, and Rust's
        // definite-assignment rules reject assigning an immutable hoisted local
        // from a loop body even when control flow always exits.
        // Under-approximating here would turn warnings into E0384/E0596 errors
        // in the generated crate, so the rule stays conservative.
        //
        // `block_can_repeat(b) || block_is_reached_from_repeating_region(b)`
        // is exactly "b lies on a cycle or is reachable from one", which the
        // index precomputes once ([`LocalWriteIndex::repeat_reachable`]):
        // asking it per assignment walked the rest of the function each time.
        self.predeclared_locals.contains(&local)
            && index.assign_blocks.get(&local).is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|block| index.repeat_reachable.contains(block))
            })
    }

    /// The function type a local's value carries in the EMITTED Rust, when the
    /// local receives the result of a call to a generated function.
    ///
    /// A generic callee's DECLARED return type keeps its type parameters, and the
    /// emitted signature spells the callback ABI from those: `make<T>(x: T) => (v:
    /// T) => boolean` is emitted as `fn make<T>(x: T) -> Rc<dyn Fn(&T) -> bool>`.
    /// The MIR type of the destination local is the INSTANTIATED return type,
    /// `(v: number) => boolean`, which has lost that provenance — asking
    /// [`FunctionEmitter::callback_param_is_shared_reference`] about `f64` answers
    /// "by value" while the value in hand is an `Rc<dyn Fn(&f64) -> bool>` (E0308).
    ///
    /// A call through such a local asks this instead, so the ABI question is
    /// answered where the ABI was decided — by the callee's own declaration —
    /// rather than by a substituted copy of it. Only the ABI question: argument
    /// coercion still runs against the local's own (instantiated) MIR types, which
    /// are the ones that describe the values.
    ///
    /// Restricted to a callee the crate emitted with REAL generics. That is the
    /// only case where the rendered ABI can disagree with MIR: such a callee spells
    /// its return from its own type parameters and the call site monomorphizes
    /// them, so the provenance is lost. A callee that erased its generics renders
    /// its return in MIR's own vocabulary, the destination's MIR type describes the
    /// value it actually holds, and the ordinary rendered-value adapter — which
    /// runs precisely because the two TypeIds differ there — has already fixed the
    /// shape. Firing on that case instead OVER-borrowed an already-adapted value.
    ///
    /// `None` whenever the local is not the destination of a static call, the
    /// callee erased its generics, or its declared return is not a function type.
    pub(super) fn emitted_call_result_function_type(&self, local: LocalId) -> Option<FunctionType> {
        let func_id = self
            .function
            .blocks
            .iter()
            .find_map(|block| match &block.terminator {
                Some(Terminator::Call {
                    callee: Callee::Static(func_id),
                    dest,
                    ..
                }) if *dest == local => Some(*func_id),
                _ => None,
            })?;
        let callee = self
            .mir
            .functions
            .get(usize::try_from(func_id.0).unwrap_or(usize::MAX))?;
        if !self.callee_emits_generics(callee.id) {
            return None;
        }
        let signature_key = self.emitted_signature_key(callee).ok()?;
        let return_ty = self.emitted_function_return_type(&signature_key)?;
        match self.mir.types.get(return_ty) {
            Some(Type::Function(function)) => Some(function.clone()),
            _ => None,
        }
    }

    /// The DECLARED function type of a class's function-typed field, for a
    /// callee operand that reads one.
    ///
    /// A field read on a GENERIC class hands the call site the field's type with
    /// the class's type arguments already substituted, which erases the one
    /// thing the callee's ABI depends on: whether the declaration wrote a bare
    /// type parameter in that position. `class Chain<T> { next: (value: T) =>
    /// Chain<T> }` is emitted as `Rc<dyn Fn(&T) -> Chain<T>>`, because
    /// `param_type_is_by_shared_reference` answers TRUE for a type parameter —
    /// the ABI has to be the callee's, and only its declaration knows it. Rust
    /// instantiates that slot at `Fn(&f64)` for a `Chain<number>`, while MIR
    /// tells the call site the parameter is `f64`, which the same predicate
    /// answers FALSE for, so the argument was packed by value against a slot
    /// spelled `&`. That is E0308, and it is the `&(SmeltUnknown, RouterRoute)`
    /// family recorded in `blocker-logs/hono-round30-types.md`.
    ///
    /// The rule: SUBSTITUTING A TYPE PARAMETER AT A CALL SITE DOES NOT CHANGE
    /// HOW THE CALLEE IS CALLED. The value is still rendered at the substituted
    /// type — that is what it has to be coerced to — but the ABI is asked of the
    /// declaration. The sibling [`Self::emitted_call_result_function_type`]
    /// answers the same question for a local that received a generic call's
    /// result.
    ///
    /// `None` for anything else — a local holding a closure, an erased callee, a
    /// non-class base — which leaves the substituted types in charge as before.
    /// A non-generic class answers with a parameter list identical to the
    /// substituted one, so the lookup changes nothing where there is nothing to
    /// substitute.
    pub(super) fn class_field_declared_function_type(
        &self,
        callee: &Operand,
    ) -> Option<FunctionType> {
        let (Operand::Copy(Place::Field { base, field })
        | Operand::Move(Place::Field { base, field })) = callee
        else {
            return None;
        };
        let base_ty = self.local_decl(*base).ok()?.ty;
        let Type::Class { name, .. } = self.mir.types.get(base_ty)? else {
            return None;
        };
        // `Type::Class` names a generated NOMINAL type, which is a class or an
        // interface record: both carry function-typed slots and both are
        // instantiated at their type arguments, so both have a declaration to
        // ask. Hono reaches this through the interface side — `Router<T>`
        // declares `add(method, path, handler: T)` and is instantiated at
        // `Router<[unknown, RouterRoute]>`.
        let declared_ty = crate::classes::find_class(self.mir, *name)
            .and_then(|class| {
                crate::classes::effective_class_fields(self.mir, class)
                    .into_iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|candidate| candidate.ty)
            })
            .or_else(|| {
                let interface = self
                    .mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)?;
                interface
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|candidate| candidate.ty)
            })?;
        match self.mir.types.get(declared_ty)? {
            Type::Function(declared_function) => Some(declared_function.clone()),
            _ => None,
        }
    }

    /// Count direct assignments to a MIR local (binding a dict entry's value
    /// to `current` counts as one).
    pub(super) fn local_assignment_count(&self, local: LocalId) -> usize {
        self.local_write_index()
            .assignments
            .get(&local)
            .copied()
            .unwrap_or(0)
    }

    /// Returns whether the first MIR access to a local reads the previous value.
    pub(super) fn local_first_access_is_read(&self, local: LocalId) -> bool {
        for block in &self.function.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::Assign { dest, value } => {
                        if value.reads_local(local) {
                            return true;
                        }
                        if *dest == local {
                            return false;
                        }
                    }
                    Statement::AssignPlace { place, value } => {
                        if place.target_reads_local(local) || value.reads_local(local) {
                            return true;
                        }
                        if matches!(place, Place::Local(candidate) if *candidate == local) {
                            return false;
                        }
                    }
                    // Evaluation order: container, key and seed are read first,
                    // then `current` is bound, then the stored rvalue runs.
                    Statement::DictEntryUpdate {
                        base,
                        index,
                        default,
                        current,
                        value,
                    } => {
                        if *base == local || index.reads_local(local) || default.reads_local(local)
                        {
                            return true;
                        }
                        if *current == local {
                            return false;
                        }
                        if value.reads_local(local) {
                            return true;
                        }
                    }
                }
            }
            if block
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator.reads_local(local))
            {
                return true;
            }
        }
        false
    }

    /// Return whether some control-flow path writes to `local` after it already
    /// has a value. Immutable Rust locals can be initialized from sibling
    /// branches, but any second assignment on one path requires `mut`.
    pub(super) fn local_may_be_assigned_after_assignment(&self, local: LocalId) -> bool {
        // The walk below can only answer `true` at a write, so a local with no
        // statement write (a call destination, say) is answered from
        // the index without walking every path of the function.
        let index = self.local_write_index();
        let Some(writes) = index.writes.get(&local) else {
            return false;
        };
        let assigned = self.function.params.contains(&local);
        // One write that no path can reach twice cannot be a second assignment,
        // and answering here keeps this lookup from walking the whole function
        // for every local of a straight-line body.
        if *writes == 1
            && !assigned
            && index
                .last_write_block
                .get(&local)
                .is_some_and(|block| !index.repeat_reachable.contains(block))
        {
            return false;
        }
        self.block_may_assign_local_after_assignment(
            self.function.entry,
            local,
            assigned,
            &mut HashSet::new(),
        )
    }

    /// Walk one control-flow path while tracking whether `local` is initialized.
    pub(super) fn block_may_assign_local_after_assignment(
        &self,
        block_id: smelt_mir::BlockId,
        local: LocalId,
        mut assigned: bool,
        seen: &mut HashSet<(smelt_mir::BlockId, bool)>,
    ) -> bool {
        if !seen.insert((block_id, assigned)) {
            return false;
        }
        let Some(block) = self.block(block_id).ok() else {
            return true;
        };
        for statement in &block.statements {
            let writes_local = match statement {
                Statement::Assign { dest, .. } => *dest == local,
                Statement::AssignPlace {
                    place: Place::Local(candidate),
                    ..
                } => *candidate == local,
                Statement::DictEntryUpdate { current, .. } => *current == local,
                Statement::AssignPlace { .. } => false,
            };
            if writes_local {
                if assigned {
                    return true;
                }
                assigned = true;
            }
        }
        block
            .terminator
            .as_ref()
            .into_iter()
            .flat_map(terminator_successors)
            .any(|successor| {
                self.block_may_assign_local_after_assignment(successor, local, assigned, seen)
            })
    }

    /// Returns whether evaluating `value` mutates `local` in-place.
    ///
    /// The receiver may be an index place rather than a bare local
    /// (`base[key].push(x)`, the form `smelt_mir::opt::DictEntryInPlaceMutation`
    /// produces): that mutates the CONTAINER in place, so the container is the
    /// local whose binding must be mutable.
    pub(super) fn rvalue_mutates_local(&self, value: &Rvalue, local: LocalId) -> bool {
        mutated_binding_root(value) == Some(local)
    }
}

/// The local whose Rust binding an in-place mutation makes `mut`: the root of
/// [`Rvalue::mutated_receiver`], except where the receiver's representation
/// decides. `DictAssign` is settled per representation by
/// `rvalue_borrows_local_mutably`, and `ListUnshift` writes through the list's
/// shared `borrow_mut` handle, so the binding itself stays immutable.
fn mutated_binding_root(value: &Rvalue) -> Option<LocalId> {
    match value {
        Rvalue::DictAssign { .. } | Rvalue::ListUnshift { .. } => None,
        other => other.mutated_receiver().and_then(operand_mutation_root),
    }
}

impl FunctionEmitter<'_> {
    /// Returns whether rendering an rvalue needs a mutable borrow of `local`.
    pub(super) fn rvalue_borrows_local_mutably(&self, value: &Rvalue, local: LocalId) -> bool {
        match value {
            Rvalue::DictAssign { target, .. } if target.local() == Some(local) => {
                let Ok(target_ty) = self.operand_ty(target) else {
                    return false;
                };
                let Some(Type::Dict(key_ty, _)) = self.mir.types.get(target_ty) else {
                    return false;
                };
                // String-keyed object records use interior mutability, while
                // `SmeltJsMap` and plain `HashMap` expose `extend` through a
                // mutable receiver. Object-spread targets using either latter
                // representation therefore need a mutable Rust binding.
                !self.dict_uses_smelt_record(*key_ty)
            }
            Rvalue::ListCallback { list, .. } if list.local() == Some(local) => {
                let Ok(list_ty) = self.operand_ty(list) else {
                    return false;
                };
                matches!(
                    self.mir.types.get(list_ty),
                    Some(Type::List(item)) if matches!(self.mir.types.get(*item), Some(Type::Function(_)))
                )
            }
            _ => false,
        }
    }

    /// Whether a predeclared shared-capture local can have its cell created at
    /// its one assignment instead of being seeded at function entry.
    ///
    /// A captured local lives in an `Rc<RefCell<T>>` that closures clone. Creating
    /// that cell at entry around `T`'s default would build (and drop) a whole
    /// placeholder instance per call for a class, only to overwrite it a few
    /// statements later. When the local has exactly one write, a plain `Assign` in
    /// a block that cannot repeat (so it runs at most once and the cell is never
    /// re-created under a closure that already shares it), is not a parameter or
    /// an unwind target, and the definite-assignment walk finds no read before it,
    /// the cell is declared uninitialized and the assignment creates it. Rust's
    /// own initialization check then guards the emitted code as well.
    pub(super) fn capture_cell_created_at_assignment(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        if self.function.params.contains(&local) || self.is_base_initializer_receiver(local) {
            return Ok(false);
        }
        let index = self.local_write_index();
        if index.unwind_written.contains(&local)
            || index.writes.get(&local).copied().unwrap_or(0) != 1
            || index.assignments.get(&local).copied().unwrap_or(0) != 1
        {
            return Ok(false);
        }
        let Some([block]) = index.assign_blocks.get(&local).map(Vec::as_slice) else {
            return Ok(false);
        };
        if index.repeat_reachable.contains(block) {
            return Ok(false);
        }
        Ok(!self.local_may_be_used_before_assignment(local)?)
    }

    /// Returns whether `local` may be read before a real MIR assignment.
    pub(super) fn local_may_be_used_before_assignment(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let assigned = self.function.params.contains(&local);
        self.block_may_use_local_before_assignment(
            self.function.entry,
            local,
            assigned,
            &mut HashSet::new(),
        )
    }

    /// Walk one control-flow path while tracking definite assignment for `local`.
    pub(super) fn block_may_use_local_before_assignment(
        &self,
        block_id: smelt_mir::BlockId,
        local: LocalId,
        mut assigned: bool,
        seen: &mut HashSet<(smelt_mir::BlockId, bool)>,
    ) -> Result<bool, EmitError> {
        if !seen.insert((block_id, assigned)) {
            return Ok(false);
        }
        let block = self.block(block_id)?;
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, value } => {
                    if value.reads_local(local) && !assigned {
                        return Ok(true);
                    }
                    if *dest == local {
                        assigned = true;
                    }
                }
                Statement::AssignPlace { place, value } => {
                    if place.target_reads_local(local) && !assigned {
                        return Ok(true);
                    }
                    if value.reads_local(local) && !assigned {
                        return Ok(true);
                    }
                    if matches!(place, Place::Local(candidate) if *candidate == local) {
                        assigned = true;
                    }
                }
                Statement::DictEntryUpdate {
                    base,
                    index,
                    default,
                    current,
                    value,
                } => {
                    if (*base == local || index.reads_local(local) || default.reads_local(local))
                        && !assigned
                    {
                        return Ok(true);
                    }
                    if *current == local {
                        assigned = true;
                    }
                    if value.reads_local(local) && !assigned {
                        return Ok(true);
                    }
                }
            }
        }
        let Some(terminator) = &block.terminator else {
            return Ok(false);
        };
        if terminator.reads_local(local) && !assigned {
            return Ok(true);
        }
        for successor in terminator_successors(terminator) {
            let successor_assigned = if matches!(terminator, Terminator::Call { dest, .. } if *dest == local)
            {
                true
            } else {
                assigned
            };
            if self.block_may_use_local_before_assignment(
                successor,
                local,
                successor_assigned,
                seen,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Per-local write facts for one function, gathered in a single pass.
///
/// Answers the mutability queries from one scan, so emitting a body of `n`
/// locals is not O(n²): a module of a few hundred nested calls would otherwise
/// spend most of its codegen time rescanning blocks.
#[derive(Debug, Default)]
pub(super) struct LocalWriteIndex {
    /// Writes per local: `Assign` destinations, whole-local
    /// `AssignPlace` targets and `DictEntryUpdate` bindings. A local absent
    /// here is never written by a statement.
    writes: HashMap<LocalId, u32>,
    /// The block of each local's last statement write, which for a local written
    /// once is its only one.
    last_write_block: HashMap<LocalId, smelt_mir::BlockId>,
    /// Sources of each local's plain-copy `Assign`s, in statement order.
    pub(super) use_sources: HashMap<LocalId, Vec<Operand>>,
    /// Direct assignments per local.
    assignments: HashMap<LocalId, usize>,
    /// Blocks holding an `Assign` to the local or a `DictEntryUpdate` binding
    /// it, for the predeclared-in-a-loop rule.
    assign_blocks: HashMap<LocalId, Vec<smelt_mir::BlockId>>,
    /// Locals that need a mutable binding wherever they are declared: an
    /// `AssignPlace` target or field/index base, a dict entry-update base, an
    /// in-place mutation root, a mutable borrow, a closure capture needing
    /// shared access, or an argument to a `&mut T` parameter.
    mutated: HashSet<LocalId>,
    /// Locals some closure in the function captures with shared access
    /// (`closure_capture_needs_shared_access`), whatever the closure's own
    /// destination.
    pub(super) shared_capture_locals: HashSet<LocalId>,
    /// Blocks that lie on a CFG cycle or are reachable from one: the blocks
    /// for which `block_can_repeat || block_is_reached_from_repeating_region`
    /// holds (see [`repeat_reachable_blocks`]).
    repeat_reachable: HashSet<smelt_mir::BlockId>,
    /// Locals an unwind edge writes: the handler local of a `Call` or `Await`
    /// terminator, which receives the thrown value.
    unwind_written: HashSet<LocalId>,
}

/// The blocks of `function` that lie on a cycle, or are reachable from a block
/// that does.
///
/// A block lies on a cycle when its strongly connected component has more
/// than one block or it has an edge to itself. Components come from
/// Kosaraju's algorithm (both passes iterative, so a long straight-line body
/// cannot overflow the stack), and a forward sweep from every cyclic block
/// adds what they reach. Linear in blocks plus edges.
fn repeat_reachable_blocks(function: &MirFunction) -> HashSet<smelt_mir::BlockId> {
    let position: HashMap<smelt_mir::BlockId, usize> = function
        .blocks
        .iter()
        .enumerate()
        .map(|(position, block)| (block.id, position))
        .collect();
    let successors: Vec<Vec<usize>> = function
        .blocks
        .iter()
        .map(|block| {
            block
                .terminator
                .as_ref()
                .map(terminator_successors)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|successor| position.get(&successor).copied())
                .collect()
        })
        .collect();
    let count = successors.len();
    let mut predecessors = vec![Vec::new(); count];
    for (from, targets) in successors.iter().enumerate() {
        for &to in targets {
            if let Some(list) = predecessors.get_mut(to) {
                list.push(from);
            }
        }
    }
    // Pass 1: finishing order of a DFS over the forward graph.
    let mut visited = vec![false; count];
    let mut finished = Vec::with_capacity(count);
    for root in 0..count {
        if visited.get(root).copied().unwrap_or(true) {
            continue;
        }
        let mut stack = vec![(root, 0_usize)];
        if let Some(seen) = visited.get_mut(root) {
            *seen = true;
        }
        while let Some((node, next)) = stack.pop() {
            let targets = successors.get(node).map_or(&[][..], Vec::as_slice);
            if let Some(&target) = targets.get(next) {
                stack.push((node, next.saturating_add(1)));
                if let Some(seen) = visited.get_mut(target)
                    && !*seen
                {
                    *seen = true;
                    stack.push((target, 0));
                }
            } else {
                finished.push(node);
            }
        }
    }
    // Pass 2: components of the reverse graph, in reverse finishing order.
    let mut component = vec![usize::MAX; count];
    let mut component_sizes = Vec::new();
    for &root in finished.iter().rev() {
        if component.get(root).copied() != Some(usize::MAX) {
            continue;
        }
        let id = component_sizes.len();
        let mut size = 0_usize;
        let mut stack = vec![root];
        if let Some(slot) = component.get_mut(root) {
            *slot = id;
        }
        while let Some(node) = stack.pop() {
            size = size.saturating_add(1);
            for &source in predecessors.get(node).map_or(&[][..], Vec::as_slice) {
                if let Some(slot) = component.get_mut(source)
                    && *slot == usize::MAX
                {
                    *slot = id;
                    stack.push(source);
                }
            }
        }
        component_sizes.push(size);
    }
    let cyclic = |node: usize| {
        component
            .get(node)
            .and_then(|id| component_sizes.get(*id))
            .is_some_and(|size| *size > 1)
            || successors
                .get(node)
                .is_some_and(|targets| targets.contains(&node))
    };
    // Forward sweep from every cyclic block.
    let mut reached = vec![false; count];
    let mut stack: Vec<usize> = (0..count).filter(|node| cyclic(*node)).collect();
    for &node in &stack {
        if let Some(seen) = reached.get_mut(node) {
            *seen = true;
        }
    }
    while let Some(node) = stack.pop() {
        for &target in successors.get(node).map_or(&[][..], Vec::as_slice) {
            if let Some(seen) = reached.get_mut(target)
                && !*seen
            {
                *seen = true;
                stack.push(target);
            }
        }
    }
    function
        .blocks
        .iter()
        .zip(reached)
        .filter_map(|(block, is_reached)| is_reached.then_some(block.id))
        .collect()
}

/// Add one to `local`'s write count.
fn bump_u32(counts: &mut HashMap<LocalId, u32>, local: LocalId) {
    let count = counts.entry(local).or_default();
    *count = count.saturating_add(1);
}

/// Add one to `local`'s assignment count.
fn bump_usize(counts: &mut HashMap<LocalId, usize>, local: LocalId) {
    let count = counts.entry(local).or_default();
    *count = count.saturating_add(1);
}
