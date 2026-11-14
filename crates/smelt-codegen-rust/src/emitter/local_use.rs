//! What a local's uses ask of it: the single target its value is read at, and
//! whether an erased definition is dead or can be rendered at the use instead.

use super::*;

impl FunctionEmitter<'_> {
    /// Returns whether a `Function`-typed call-result temp is dead because its
    /// only consumer re-evaluates the call.
    ///
    /// A data-last call such as `capitalize()` produces a value whose MIR local
    /// is typed `Function` (the curried callback) but whose generated function
    /// actually returns `SmeltUnknown`. When that result is only ever *erased*
    /// back to `SmeltUnknown` (e.g. pushed into a `pipe(...)` argument list), the
    /// erase seam re-derives the value by re-rendering the defining call at
    /// `Unknown` (see `coercion::erased_call_assignment_expr`). In that case the
    /// typed-callback binding the call terminator/statement would emit is never
    /// read: it is a dead store whose extraction-from-`SmeltUnknown` template
    /// also evaluates the call a *second* time, double-moving the arguments.
    ///
    /// This predicate detects exactly that shape so the binding can be
    /// suppressed, leaving the single re-inlined erase as the lone evaluation —
    /// the same idiomatic "call once, reuse" code a human would write. It is
    /// deliberately narrow (a single erasing use) so it can never hide a live
    /// reader: any local with a non-erasing or additional use keeps its binding.
    pub(super) fn function_call_result_dead_when_erased(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        // Only typed-callback temps suffer the redundant extraction template;
        // an `Unknown`-typed call result already binds at its natural type and
        // its erase simply reads the binding (no re-inline).
        if !matches!(self.local_decl(local)?.kind, LocalKind::Temp)
            || !matches!(
                self.mir.types.get(self.local_decl(local)?.ty),
                Some(Type::Function(_))
            )
        {
            return Ok(false);
        }
        // The erase seam only re-inlines results whose definition is a
        // `ClosureCall` statement or a `Call` terminator; mirror that gate so a
        // suppressed binding is always reconstructable at the use site.
        if !self.local_defined_by_reinlinable_call(local) {
            return Ok(false);
        }
        // Require a single use, and require that use to erase the local. With one
        // erasing consumer the binding is provably dead: the erase re-renders the
        // call, so nothing reads the name we would otherwise bind.
        match self.sole_use_erase_target(local)? {
            Some(target) => Ok(self.has_dynamic_shape(target)),
            None => Ok(false),
        }
    }

    /// Returns whether a local's defining definition is a call the erase seam
    /// can re-render at `Unknown` (a `ClosureCall` statement or `Call`
    /// terminator). Mirrors `coercion::erased_call_assignment_expr`'s gate.
    fn local_defined_by_reinlinable_call(&self, local: LocalId) -> bool {
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, value } = statement
                    && *dest == local
                {
                    return matches!(value, Rvalue::ClosureCall { .. });
                }
            }
            if let Some(Terminator::Call { dest, .. }) = &block.terminator
                && *dest == local
            {
                return true;
            }
        }
        false
    }

    /// Returns the coercion target type for a local that is used exactly once,
    /// or `None` if the local is used zero or multiple times, or used in a
    /// context whose target this analysis does not classify.
    ///
    /// The classified contexts are the ones in which a `Function`-typed value
    /// realistically flows to an erased target: elements of a list/set/tuple
    /// literal, dictionary values, call arguments, and `return`. Any unhandled
    /// context yields `None`, which keeps the conservative "do not suppress"
    /// answer in [`Self::function_call_result_dead_when_erased`].
    fn sole_use_erase_target(&self, local: LocalId) -> Result<Option<TypeId>, EmitError> {
        let mut found: Option<TypeId> = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if !value.reads_local(local) {
                    continue;
                }
                let Some(target) = self.rvalue_use_target(value, *dest, local)? else {
                    return Ok(None);
                };
                if found.replace(target).is_some() {
                    return Ok(None);
                }
            }
            if let Some(terminator) = &block.terminator
                && terminator.reads_local(local)
            {
                let Some(target) = self.terminator_use_target(terminator, local)? else {
                    return Ok(None);
                };
                if found.replace(target).is_some() {
                    return Ok(None);
                }
            }
        }
        Ok(found)
    }

    /// Classifies the target type a local is coerced to inside an rvalue whose
    /// destination is `dest`. Returns `None` for unhandled rvalue shapes or when
    /// the local appears more than once.
    fn rvalue_use_target(
        &self,
        value: &Rvalue,
        dest: LocalId,
        local: LocalId,
    ) -> Result<Option<TypeId>, EmitError> {
        match value {
            Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
                if items.iter().filter(|item| item.reads_local(local)).count() != 1 {
                    return Ok(None);
                }
                let dest_ty = self.local_decl(dest)?.ty;
                let item_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::List(item) | Type::Set(item)) => Some(*item),
                    Some(Type::Tuple(item_tys)) => {
                        let position = items.iter().position(|item| item.reads_local(local));
                        position.and_then(|index| item_tys.get(index).copied())
                    }
                    _ => None,
                };
                Ok(item_ty)
            }
            Rvalue::Dict(entries) => {
                // A `Function`-typed call result used as a dictionary value (e.g.
                // an `evolve({ id: add(1), ... })` field) is erased to the dict's
                // value type. Only the value position erases the local; a local
                // appearing in a key would not be this typed-callback shape.
                if entries
                    .iter()
                    .filter(|(key, entry_value)| {
                        key.reads_local(local) || entry_value.reads_local(local)
                    })
                    .count()
                    != 1
                {
                    return Ok(None);
                }
                let Some((key, _)) = entries
                    .iter()
                    .find(|(_, entry_value)| entry_value.reads_local(local))
                else {
                    return Ok(None);
                };
                if key.reads_local(local) {
                    return Ok(None);
                }
                let dest_ty = self.local_decl(dest)?.ty;
                let value_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::Dict(_, value_ty)) => Some(*value_ty),
                    _ => None,
                };
                Ok(value_ty)
            }
            Rvalue::Use(operand) if operand.reads_local(local) => {
                Ok(Some(self.local_decl(dest)?.ty))
            }
            _ => Ok(None),
        }
    }

    /// Classifies the target type a local is coerced to inside a terminator.
    /// Returns `None` for unhandled terminators or multiple appearances.
    fn terminator_use_target(
        &self,
        terminator: &Terminator,
        local: LocalId,
    ) -> Result<Option<TypeId>, EmitError> {
        match terminator {
            Terminator::Return(operand) if operand.reads_local(local) => {
                Ok(Some(self.function.return_ty))
            }
            _ => Ok(None),
        }
    }

    /// Returns whether a local is read anywhere in the function body.
    pub(super) fn local_has_uses(&self, local: LocalId) -> bool {
        self.function.blocks.iter().any(|block| {
            block
                .statements
                .iter()
                .any(|statement| statement.reads_local(local))
                || block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator.reads_local(local))
        })
    }

    /// Returns whether the emitted Rust names a closure local's binding.
    ///
    /// Stricter than [`Self::local_has_uses`], which answers for the MIR: a read
    /// through [`inlined_callback_operands`] re-renders the closure's text at the
    /// use site, so a binding for it would be dead. An optional-typed callback is
    /// matched by name at runtime, so only a `Function`-typed local can skip it.
    pub(super) fn closure_binding_is_read(&self, local: LocalId) -> Result<bool, EmitError> {
        let is_function = matches!(
            self.mir.types.get(self.local_decl(local)?.ty),
            Some(Type::Function(_))
        );
        let statement_names_local = |statement: &Statement| {
            let inlined = if is_function {
                inlined_callback_operands(statement.value())
            } else {
                Vec::new()
            };
            let mut named = false;
            statement.reads(|read| {
                let skipped = matches!(
                    read,
                    Read::Operand(operand)
                        if inlined.iter().any(|callback| std::ptr::eq(*callback, operand))
                );
                if !skipped {
                    read.read_locals(&mut |candidate| named |= candidate == local);
                }
            });
            named
        };
        Ok(self.function.blocks.iter().any(|block| {
            block.statements.iter().any(statement_names_local)
                || block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator.reads_local(local))
        }))
    }

    /// Returns the source operand for a single-assignment unknown cast local.
    ///
    /// TypeScript type assertions and control-flow narrows do not convert the
    /// runtime value. When a narrowed temporary is immediately returned as
    /// `unknown`, the original tagged value should flow through instead of
    /// being unwrapped and rewrapped as the narrowed shape.
    pub(super) fn single_assignment_unknown_cast_source(&self, local: LocalId) -> Option<&Operand> {
        let mut found = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign {
                    dest,
                    value: Rvalue::UnknownCast { value, target: _ },
                } = statement
                else {
                    continue;
                };
                if *dest != local {
                    continue;
                }
                if found.is_some() {
                    return None;
                }
                found = Some(value);
            }
        }
        found
    }

    /// Returns whether an operand is a local closure whose body can throw.
    pub(super) fn operand_closure_can_throw(&self, operand: &Operand) -> Result<bool, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => return Ok(false),
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == local
                {
                    let closure = self
                        .mir
                        .closures
                        .get(id_index(id.0, "closure index does not fit usize")?)
                        .ok_or_else(|| {
                            EmitError::new("closure operand references unknown closure")
                        })?;
                    return Ok(closure.can_throw);
                }
            }
        }
        Ok(false)
    }
}

/// The callback operands an rvalue renders by re-emitting the closure's text at
/// the use site (`closure_operand_expr` and its declared-type sibling) rather than
/// by naming the closure's local.
///
/// `ListCallback` inlines too, but is left out: its closures keep a binding.
/// Anything not listed counts as naming the local, so a new callback rvalue costs
/// at most a dead binding, never a missing one.
fn inlined_callback_operands(value: &Rvalue) -> Vec<&Operand> {
    match value {
        Rvalue::ListReduce { callback, .. }
        | Rvalue::ListFromLengthMap { callback, .. }
        | Rvalue::RegexReplaceCallback { callback, .. } => vec![callback],
        Rvalue::ListSort {
            comparator, key, ..
        } => comparator.iter().chain(key).collect(),
        Rvalue::ListSorted { key, .. } => key.iter().collect(),
        Rvalue::FormDataOp {
            op: smelt_hir::FormDataOp::ForEach,
            args,
            ..
        } => args.first().into_iter().collect(),
        _ => Vec::new(),
    }
}
