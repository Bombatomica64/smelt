//! Local dataflow and mutability analysis: whether a MIR local needs a mutable Rust binding, its assignment/use ordering, and rvalue-level in-place mutation and mutable-borrow queries.

use super::*;
use super::core::{
    assignment_place_reads_local, operand_uses_local, rvalue_uses_local, terminator_successors,
    terminator_uses_local,
};

impl FunctionEmitter<'_> {
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
        for block in &self.function.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::Assign { dest, .. } if *dest == local => {
                        // The repeating-region rule only forces `mut` when the
                        // binding's `let` lives OUTSIDE the repeating region and
                        // is therefore reassigned across iterations. A binding
                        // that reaches this point is assigned exactly once
                        // (multi-assignment locals already returned `true` above
                        // via `local_assignment_count`), so the only shape that
                        // still needs `mut` is a *predeclared* local — one
                        // hoisted to function scope, whose `let` sits outside the
                        // loop. A non-predeclared local instead emits its `let`
                        // inline at the assignment site, re-running as a fresh
                        // binding each iteration, and needs no `mut`.
                        //
                        // `block_is_reached_from_repeating_region` is
                        // deliberately kept alongside `block_can_repeat`: the
                        // structured emitter can place a MIR-diverging assignment
                        // textually inside `loop { .. }`, and Rust's
                        // definite-assignment rules reject assigning an immutable
                        // hoisted local from a loop body even when control flow
                        // always exits. Under-approximating here would turn
                        // warnings into E0384/E0596 errors in the generated
                        // crate, so the rule stays conservative.
                        if self.predeclared_locals.contains(&local)
                            && (self.block_can_repeat(block.id, &mut HashSet::new())
                                || self.block_is_reached_from_repeating_region(block.id))
                        {
                            return true;
                        }
                    }
                    Statement::AssignPlace {
                        place:
                            Place::Local(candidate)
                            | Place::Field {
                                base: candidate, ..
                            }
                            | Place::Index {
                                base: candidate, ..
                            },
                        ..
                    } if *candidate == local => return true,
                    Statement::Assign { value, .. } => {
                        if self.rvalue_mutates_local(value, local) {
                            return true;
                        }
                        if self.rvalue_borrows_local_mutably(value, local) {
                            return true;
                        }
                        if let Rvalue::Closure { id, .. } = value
                            && let Some(closure) = self
                                .mir
                                .closures
                                .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                            && closure.captures.iter().any(|capture| {
                                capture.source_local == local
                                    && self.closure_capture_needs_shared_access(closure, capture)
                            })
                        {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(Terminator::Call {
                callee: Callee::Static(function_id),
                args,
                ..
            }) = &block.terminator
                && args.iter().enumerate().any(|(index, arg)| {
                    operand_local(arg) == Some(local)
                        && self
                            .mir
                            .functions
                            .get(usize::try_from(function_id.0).unwrap_or(usize::MAX))
                            .and_then(|function| {
                                function.params.get(index).map(|param| {
                                    self.parameter_needs_mutable_reference_in(function, *param)
                                })
                            })
                            .unwrap_or(false)
                })
            {
                return true;
            }
        }
        false
    }

    /// Count direct assignments to a MIR local.
    pub(super) fn local_assignment_count(&self, local: LocalId) -> usize {
        self.function
            .blocks
            .iter()
            .flat_map(|block| block.statements.iter())
            .filter(|statement| match statement {
                Statement::Assign { dest, .. } => *dest == local,
                Statement::AssignPlace {
                    place: Place::Local(candidate),
                    ..
                } => *candidate == local,
                _ => false,
            })
            .count()
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
        let Some(block) = self
            .function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
        else {
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
    pub(super) fn rvalue_mutates_local(&self, value: &Rvalue, local: LocalId) -> bool {
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
            _ => return false,
        };
        operand_local(mutated) == Some(local)
    }

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
