//! Local dataflow and mutability analysis: whether a MIR local needs a mutable Rust binding, its assignment/use ordering, and rvalue-level in-place mutation and mutable-borrow queries.

use super::*;
use smelt_hir::FunctionType;
use super::core::{
    assignment_place_reads_local, operand_uses_local, rvalue_uses_local, terminator_successors,
    terminator_uses_local,
};

impl FunctionEmitter<'_> {
    /// The function's [`LocalWriteIndex`], built on first use.
    pub(super) fn local_write_index(&self) -> &LocalWriteIndex {
        self.local_write_index.get_or_init(|| self.build_local_write_index())
    }

    /// Scan every block once and record the write facts the per-local
    /// mutability queries need (see [`LocalWriteIndex`]).
    fn build_local_write_index(&self) -> LocalWriteIndex {
        let mut index = LocalWriteIndex::default();
        for block in &self.function.blocks {
            for phi in &block.phis {
                bump_u32(&mut index.writes, phi.dest);
            }
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
                                        self.closure_capture_needs_shared_access(closure, capture)
                                    })
                                    .map(|capture| capture.source_local),
                            );
                        }
                        bump_u32(&mut index.writes, *dest);
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
                        bump_usize(&mut index.assignments, *current);
                        index.assign_blocks.entry(*current).or_default().push(block.id);
                        if self.dict_entry_update_needs_mut_base(*base) {
                            index.mutated.insert(*base);
                        }
                    }
                    Statement::StorageLive(_) | Statement::StorageDead(_) => {}
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
                    if let Some(local) = operand_local(arg)
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
        let mut indexed: Vec<_> = self.local_write_index().repeat_reachable.iter().copied().collect();
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
        if let Some(root) = rvalue_mutation_target(value).and_then(operand_mutation_root) {
            locals.push(root);
        }
        match value {
            Rvalue::DictAssign { target, .. } | Rvalue::ListCallback { list: target, .. } => {
                if let Some(local) = operand_local(target)
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
                            .filter(|capture| self.closure_capture_needs_shared_access(closure, capture))
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
        let func_id = self.function.blocks.iter().find_map(|block| match &block.terminator {
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
        let declared_ty = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == *name)
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
            for phi in &block.phis {
                if phi
                    .incoming
                    .iter()
                    .any(|(_, operand)| operand_uses_local(operand, local))
                {
                    return true;
                }
                if phi.dest == local {
                    return false;
                }
            }
            for statement in &block.statements {
                match statement {
                    Statement::Assign { dest, value } => {
                        if rvalue_uses_local(value, local) {
                            return true;
                        }
                        if *dest == local {
                            return false;
                        }
                    }
                    Statement::AssignPlace { place, value } => {
                        if assignment_place_reads_local(place, local)
                            || rvalue_uses_local(value, local)
                        {
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
                        if *base == local
                            || operand_uses_local(index, local)
                            || operand_uses_local(default, local)
                        {
                            return true;
                        }
                        if *current == local {
                            return false;
                        }
                        if rvalue_uses_local(value, local) {
                            return true;
                        }
                    }
                    Statement::StorageLive(_) | Statement::StorageDead(_) => {}
                }
            }
            if block
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator_uses_local(terminator, local))
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
        // phi or statement write (a call destination, say) is answered from
        // the index without walking every path of the function.
        if !self.local_write_index().writes.contains_key(&local) {
            return false;
        }
        let assigned = self.function.params.contains(&local);
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
        for phi in &block.phis {
            if phi.dest == local {
                if assigned {
                    return true;
                }
                assigned = true;
            }
        }
        for statement in &block.statements {
            let writes_local = match statement {
                Statement::Assign { dest, .. } => *dest == local,
                Statement::AssignPlace {
                    place: Place::Local(candidate),
                    ..
                } => *candidate == local,
                Statement::DictEntryUpdate { current, .. } => *current == local,
                Statement::AssignPlace { .. }
                | Statement::StorageLive(_)
                | Statement::StorageDead(_) => false,
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
    /// The receiver is [`rvalue_mutation_target`]'s operand.
    ///
    /// The receiver may be an index place rather than a bare local
    /// (`base[key].push(x)`, the form `smelt_mir::opt::DictEntryInPlaceMutation`
    /// produces): that mutates the CONTAINER in place, so the container is the
    /// local whose binding must be mutable.
    pub(super) fn rvalue_mutates_local(&self, value: &Rvalue, local: LocalId) -> bool {
        rvalue_mutation_target(value).and_then(operand_mutation_root) == Some(local)
    }
}

/// The container operand an rvalue mutates in place, if it is a mutating
/// collection operation.
fn rvalue_mutation_target(value: &Rvalue) -> Option<&Operand> {
        let mutated = match value {
            Rvalue::ListPush { list, .. }
            | Rvalue::ListExtend { list, .. }
            | Rvalue::ListInsert { list, .. }
            | Rvalue::ListSplice { list, .. }
            | Rvalue::ListReverse { list }
            | Rvalue::ListFill { list, .. }
            | Rvalue::ListCopyWithin { list, .. }
            | Rvalue::ListClear { list }
            | Rvalue::ListRemove { list, .. }
            | Rvalue::ListSort { list, .. }
            | Rvalue::ListPop { list }
            | Rvalue::ListShift { list }
            | Rvalue::ListNext { list }
            | Rvalue::SetAdd { set: list, .. }
            | Rvalue::SetRemove { set: list, .. }
            | Rvalue::SetClear { set: list }
            | Rvalue::DictClear { dict: list }
            | Rvalue::DictPop { dict: list, .. }
            | Rvalue::DictSet { dict: list, .. }
            | Rvalue::DictRemoveKey { dict: list, .. }
            | Rvalue::DictSetDefault { dict: list, .. }
            | Rvalue::DictUpdate { dict: list, .. } => list,
            _ => return None,
        };
        Some(mutated)
}

impl FunctionEmitter<'_> {

    /// Returns whether rendering an rvalue needs a mutable borrow of `local`.
    pub(super) fn rvalue_borrows_local_mutably(&self, value: &Rvalue, local: LocalId) -> bool {
        match value {
            Rvalue::DictAssign { target, .. } if operand_local(target) == Some(local) => {
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
            Rvalue::ListCallback { list, .. } if operand_local(list) == Some(local) => {
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

    /// Returns whether `local` may be read before a real MIR assignment.
    pub(super) fn local_may_be_used_before_assignment(&self, local: LocalId) -> Result<bool, EmitError> {
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
        for phi in &block.phis {
            if phi
                .incoming
                .iter()
                .any(|(_, operand)| operand_uses_local(operand, local))
                && !assigned
            {
                return Ok(true);
            }
            if phi.dest == local {
                assigned = true;
            }
        }
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, value } => {
                    if rvalue_uses_local(value, local) && !assigned {
                        return Ok(true);
                    }
                    if *dest == local {
                        assigned = true;
                    }
                }
                Statement::AssignPlace { place, value } => {
                    if assignment_place_reads_local(place, local) && !assigned {
                        return Ok(true);
                    }
                    if rvalue_uses_local(value, local) && !assigned {
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
                    if (*base == local
                        || operand_uses_local(index, local)
                        || operand_uses_local(default, local))
                        && !assigned
                    {
                        return Ok(true);
                    }
                    if *current == local {
                        assigned = true;
                    }
                    if rvalue_uses_local(value, local) && !assigned {
                        return Ok(true);
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        let Some(terminator) = &block.terminator else {
            return Ok(false);
        };
        if terminator_uses_local(terminator, local) && !assigned {
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
/// The mutability queries used to rescan every block for every local they
/// were asked about, which made emitting a body of `n` locals O(n²): a module
/// of a few hundred nested calls spent most of its codegen time there. The
/// index answers the same questions from one scan.
#[derive(Debug, Default)]
pub(super) struct LocalWriteIndex {
    /// Writes per local: phi destinations, `Assign` destinations, whole-local
    /// `AssignPlace` targets and `DictEntryUpdate` bindings. A local absent
    /// here is never written by a statement or phi.
    writes: HashMap<LocalId, u32>,
    /// Direct assignments per local (the writes above, minus phis).
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
            || successors.get(node).is_some_and(|targets| targets.contains(&node))
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
