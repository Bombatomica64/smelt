//! Core emission helpers.

use super::*;
use crate::emitter::literals::operand_local;
use smelt_hir::FunctionType;

impl<'mir> FunctionEmitter<'mir> {
    /// Creates a new function emitter for the given MIR and function.
    pub(crate) fn new(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
    ) -> Result<Self, EmitError> {
        let none_ty = context.none_ty;
        let unknown_ty = mir
            .types
            .all()
            .iter()
            .enumerate()
            .find_map(|(id, ty)| {
                (*ty == Type::Unknown)
                    .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
            })
            .transpose()?
            .unwrap_or(none_ty);
        let names = Self::local_names(mir, function)?;
        let declared_locals = function.params.iter().copied().collect();
        let predeclared_locals = predeclared_locals_for_function(mir, function);
        Ok(Self {
            mir,
            context,
            function,
            names,
            mutable_locals: assigned_locals(mir, function),
            declared_locals: RefCell::new(declared_locals),
            predeclared_locals,
            termination_cache: RefCell::new(HashMap::new()),
            loop_exit_cache: RefCell::new(HashMap::new()),
            borrowed_callback_names: HashSet::new(),
            none_ty,
            unknown_local: LocalDecl {
                ty: unknown_ty,
                kind: LocalKind::Temp,
                span: Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
            },
        })
    }

    /// Builds stable Rust names for every MIR local in a function.
    fn local_names(
        mir: &'mir Mir,
        function: &'mir MirFunction,
    ) -> Result<HashMap<LocalId, String>, EmitError> {
        let mut names = HashMap::new();
        let mut used = HashSet::new();
        let mut next_arg = 0usize;

        for (idx, local) in function.locals.iter().enumerate() {
            let local_id = LocalId(compact_index(idx, "local index does not fit u32")?);
            let base_name = match local.kind {
                LocalKind::Param { symbol } => {
                    if matches!(function.origin, HirOrigin::ClassMethod { .. })
                        && function.params.first() == Some(&local_id)
                    {
                        "self".to_owned()
                    } else if let Some(name) = symbol
                        .and_then(|param_symbol| mir.symbols.get(param_symbol))
                        .map(sanitize_ident)
                        .filter(|name| !name.is_empty())
                    {
                        name
                    } else if matches!(
                        function.origin,
                        HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
                    ) {
                        format!("arg_{}", local_id.0)
                    } else {
                        let name = format!("arg_{next_arg}");
                        next_arg = next_arg
                            .checked_add(1)
                            .ok_or_else(|| EmitError::new("argument index overflowed usize"))?;
                        name
                    }
                }
                LocalKind::Temp => format!("_smelt_tmp_{}", local_id.0),
                LocalKind::UserBinding(symbol) => {
                    let name = mir
                        .symbols
                        .get(symbol)
                        .ok_or_else(|| EmitError::new("local has unknown symbol"))?;
                    sanitize_ident(name)
                }
            };
            let name = unique_local_name(base_name, &mut used);
            names.insert(local_id, name);
        }

        Ok(names)
    }

    /// Emits a free function definition.
    pub(crate) fn emit(&mut self, out: &mut String) -> Result<(), EmitError> {
        let name = self.symbol_name(self.function.name)?;
        if self.function.is_test {
            if self.function.is_async {
                out.push_str("#[tokio::test]\n");
            } else {
                out.push_str("#[test]\n");
            }
        }
        if !self.function.is_test && name == "main" && self.function.return_ty == self.none_ty {
            if self.function.can_throw {
                if self.function.is_async {
                    out.push_str(
                        "#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n",
                    );
                } else {
                    out.push_str("fn main() -> Result<(), Box<dyn std::error::Error>> {\n");
                }
            } else if self.function.is_async {
                out.push_str("#[tokio::main]\nasync fn main() {\n");
            } else {
                out.push_str("fn main() {\n");
            }
        } else {
            let fn_params = self
                .function
                .params
                .iter()
                .map(|param| {
                    let mutability = if self.local_binding_needs_mut(*param) {
                        "mut "
                    } else {
                        ""
                    };
                    Ok(format!(
                        "{mutability}{}: {}",
                        self.local_name(*param)?,
                        self.parameter_decl_type_text(*param)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            out.push_str(&format!(
                "{}fn {}({fn_params}) -> {} {{\n",
                if self.function.is_async { "async " } else { "" },
                self.function_rust_name(self.function)?,
                self.return_type_text(self.function.return_ty)?
            ));
        }

        if self.function_uses_shared_captures() {
            out.push_str("    let smelt_capture_scope = smelt_next_capture_scope();\n");
        }
        self.emit_mutable_local_preludes(out)?;
        self.emit_block(self.entry_block()?, out)?;
        if !self.block_eventually_terminates(self.function.entry, &mut HashSet::new())?
            && !emitted_tail_returns(out)
        {
            self.emit_fallthrough_return(out)?;
        }
        out.push_str("}\n");
        Ok(())
    }

    /// Return whether this function constructs closures that need shared capture handles.
    fn function_uses_shared_captures(&self) -> bool {
        self.function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                let Statement::Assign {
                    value: Rvalue::Closure { id, .. },
                    ..
                } = statement
                else {
                    return false;
                };
                self.mir
                    .closures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                    .is_some_and(|closure| {
                        closure.captures.iter().any(|capture| {
                            self.closure_capture_needs_shared_access(closure, capture)
                        })
                    })
            })
        })
    }

    /// Emits a conservative return for non-terminating generated control flow.
    ///
    /// Some unstructured MIR shapes cannot yet be rendered as a single Rust
    /// expression with all branch joins preserved. When a non-void function can
    /// fall through, Rust would otherwise infer `()` and report E0308; returning
    /// the type default keeps the generated crate type-correct until the CFG
    /// shape is represented more precisely.
    pub(super) fn emit_fallthrough_return(&self, out: &mut String) -> Result<(), EmitError> {
        if self.function.can_throw {
            out.push_str(&format!(
                "    return Ok({});\n",
                self.default_value(self.function.return_ty)?
            ));
        } else if self.function.return_ty == self.none_ty {
            return Ok(());
        } else {
            out.push_str(&format!(
                "    return {};\n",
                self.default_value(self.function.return_ty)?
            ));
        }
        Ok(())
    }

    /// Emits function-scoped mutable local declarations before block emission.
    ///
    /// MIR locals are function-scoped, while generated Rust branch bodies are
    /// lexically scoped. Predeclaring mutable locals keeps repeated or
    /// unstructured block emission from creating branch-local bindings that are
    /// later reassigned outside the branch.
    pub(super) fn emit_mutable_local_preludes(&self, out: &mut String) -> Result<(), EmitError> {
        let params = self.function.params.iter().copied().collect::<HashSet<_>>();
        let mut locals = self.predeclared_locals().into_iter().collect::<Vec<_>>();
        locals.sort_by_key(|local| local.0);
        for local in locals {
            if params.contains(&local) || self.is_local_declared(local) {
                continue;
            }
            let name = self.local_name(local)?;
            if name == "_" {
                continue;
            }
            let decl = self.local_decl(local)?;
            if matches!(self.mir.types.get(decl.ty), Some(Type::Future(_))) {
                continue;
            }
            if matches!(self.mir.types.get(decl.ty), Some(Type::Class { .. }))
                && !self.is_erased_class_type(decl.ty)
            {
                continue;
            }
            let mutability = if self.local_binding_needs_mut(local) {
                "mut "
            } else {
                ""
            };
            if self.predeclared_local_needs_default(local)?
                || self.local_may_be_used_before_assignment(local)?
            {
                out.push_str(&format!(
                    "    let {mutability}{}: {} = {};\n",
                    name,
                    self.type_text_with_impl_trait(decl.ty, false)?,
                    self.default_value(decl.ty)?
                ));
            } else {
                out.push_str(&format!(
                    "    let {mutability}{}: {};\n",
                    name,
                    self.type_text_with_impl_trait(decl.ty, false)?
                ));
            }
            self.mark_local_declared(local);
        }
        Ok(())
    }

    /// Returns locals that should be declared before block emission.
    ///
    /// Locals first assigned outside the entry block may be introduced inside a
    /// Rust branch scope and then reused by a sibling or follow-up block. Moving
    /// those bindings to function scope preserves MIR's function-local storage
    /// without perturbing straight-line entry-block declarations.
    pub(super) fn predeclared_locals(&self) -> HashSet<LocalId> {
        self.predeclared_locals.clone()
    }

    /// Returns whether a predeclared local should keep a concrete default.
    fn predeclared_local_needs_default(&self, local: LocalId) -> Result<bool, EmitError> {
        let decl = self.local_decl(local)?;
        let name = self.local_name(local)?;
        let is_callable = matches!(self.mir.types.get(decl.ty), Some(Type::Function(_)))
            || self
                .type_text_with_impl_trait(decl.ty, false)?
                .contains("FnMut");
        if is_callable
            && (name.starts_with("_smelt_tmp_")
                || self.local_assignment_count(local) == 0
                || self.local_first_access_is_read(local))
        {
            return Ok(true);
        }
        Ok(matches!(self.mir.types.get(decl.ty), Some(Type::Float))
            && name.starts_with("index")
            && (name.contains('_') || self.local_first_access_is_read(local)))
    }

    /// Returns whether the first Rust binding for `local` must be mutable.
    pub(super) fn local_binding_needs_mut(&self, local: LocalId) -> bool {
        if self
            .local_decl(local)
            .ok()
            .and_then(|decl| self.mir.types.get(decl.ty))
            .is_some_and(|ty| matches!(ty, Type::Class { .. }))
        {
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
                        if self.block_can_repeat(block.id, &mut HashSet::new())
                            || self.block_is_reached_from_repeating_region(block.id)
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
    fn local_assignment_count(&self, local: LocalId) -> usize {
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
    fn local_first_access_is_read(&self, local: LocalId) -> bool {
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

    /// Return whether control flow from `block_id` can reach `block_id` again.
    fn block_can_repeat(
        &self,
        block_id: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> bool {
        let Some(block) = self
            .function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
        else {
            return true;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        visited.insert(block_id);
        terminator_successors(terminator)
            .into_iter()
            .any(|successor| {
                successor == block_id
                    || self.block_can_reach(successor, block_id, &mut visited.clone())
            })
    }

    /// Return whether `block_id` is emitted under a structured Rust loop.
    ///
    /// MIR control flow can prove that a branch-local assignment is followed by
    /// a return on every semantic path, but the structured Rust emitter may
    /// still place that assignment textually inside a `loop { ... }`. Rust's
    /// definite-assignment rules reject assigning to an immutable local from a
    /// loop body even when later control flow always exits, so locals assigned
    /// in blocks reached from repeatable regions need mutable bindings.
    fn block_is_reached_from_repeating_region(&self, block_id: smelt_mir::BlockId) -> bool {
        self.function
            .blocks
            .iter()
            .filter(|candidate| candidate.id != block_id)
            .any(|candidate| {
                self.block_can_repeat(candidate.id, &mut HashSet::new())
                    && self.block_can_reach(candidate.id, block_id, &mut HashSet::new())
            })
    }

    /// Return whether a successor path can reach `target`.
    fn block_can_reach(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> bool {
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        self.function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
            .and_then(|block| block.terminator.as_ref())
            .is_some_and(|terminator| {
                terminator_successors(terminator)
                    .into_iter()
                    .any(|next| self.block_can_reach(next, target, visited))
            })
    }

    /// Returns whether a local is read anywhere in the function body.
    pub(super) fn local_has_uses(&self, local: LocalId) -> bool {
        self.function.blocks.iter().any(|block| {
            block.phis.iter().any(|phi| {
                phi.incoming
                    .iter()
                    .any(|(_, operand)| operand_uses_local(operand, local))
            }) || block.statements.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => rvalue_uses_local(value, local),
                Statement::AssignPlace { place, value } => {
                    assignment_place_reads_local(place, local) || rvalue_uses_local(value, local)
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => false,
            }) || block
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator_uses_local(terminator, local))
        })
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

    /// Returns true when a local is produced by a call to an async throwing function.
    pub(super) fn local_is_async_throwing_call_result(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        for block in &self.function.blocks {
            let Some(Terminator::Call { callee, dest, .. }) = &block.terminator else {
                continue;
            };
            if *dest != local {
                continue;
            }
            let Callee::Static(function_id) = callee else {
                return Ok(false);
            };
            let function = self
                .mir
                .functions
                .get(id_index(
                    function_id.0,
                    "function index does not fit usize",
                )?)
                .ok_or_else(|| EmitError::new("call references an unknown function"))?;
            return Ok(function.is_async && function.can_throw);
        }
        Ok(false)
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

    /// Return whether some control-flow path writes to `local` after it already
    /// has a value. Immutable Rust locals can be initialized from sibling
    /// branches, but any second assignment on one path requires `mut`.
    fn local_may_be_assigned_after_assignment(&self, local: LocalId) -> bool {
        let assigned = self.function.params.contains(&local);
        self.block_may_assign_local_after_assignment(
            self.function.entry,
            local,
            assigned,
            &mut HashSet::new(),
        )
    }

    /// Walk one control-flow path while tracking whether `local` is initialized.
    fn block_may_assign_local_after_assignment(
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

    /// Returns whether a parameter should be passed by mutable reference.
    ///
    /// JavaScript collection parameters share object identity with the caller.
    /// When a function mutates such a parameter in place, an owned Rust `Vec` or
    /// map would mutate only a local copy. Rebinding the parameter itself still
    /// stays owned because that does not update the caller's binding in JS.
    pub(super) fn parameter_needs_mutable_reference(&self, local: LocalId) -> bool {
        self.parameter_needs_mutable_reference_in(self.function, local)
    }

    /// Returns whether a parameter in `function` needs mutable-reference ABI.
    pub(super) fn parameter_needs_mutable_reference_in(
        &self,
        function: &MirFunction,
        local: LocalId,
    ) -> bool {
        if !function.params.contains(&local) {
            return false;
        }
        if !self
            .function_local_decl(function, local)
            .ok()
            .and_then(|decl| self.mir.types.get(decl.ty))
            .is_some_and(|ty| matches!(ty, Type::List(_) | Type::Set(_) | Type::Dict(_, _)))
        {
            return false;
        }
        function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| match statement {
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
                } if *candidate == local => true,
                Statement::Assign { value, .. } => self.rvalue_mutates_local(value, local),
                _ => false,
            })
        })
    }

    /// Renders an argument for a mutable-reference collection parameter.
    pub(super) fn mutable_reference_argument_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let text = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if let Place::Local(local) = place
                    && self.parameter_needs_mutable_reference(*local)
                {
                    return self.place_text(place);
                }
                if self.place_ty(place)? == target {
                    self.place_text(place)?
                } else {
                    self.operand_as_type_text(operand, target)?
                }
            }
            Operand::Const(_) => self.operand_as_type_text(operand, target)?,
        };
        Ok(format!("&mut {text}"))
    }

    /// Returns whether a non-escaping capture must share outer storage.
    ///
    /// JavaScript closures observe the same binding as the outer scope. The
    /// generated Rust only uses shared local storage when the closure body
    /// writes through that capture; read-only captures can remain cloned.
    pub(super) fn closure_capture_needs_shared_access(
        &self,
        closure: &MirClosure,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        if capture.mode == smelt_hir::CaptureMode::ByMut {
            return true;
        }
        if closure.escapes
            && !self.function.params.contains(&capture.source_local)
            && !self
                .local_decl(capture.source_local)
                .is_ok_and(|local| matches!(self.mir.types.get(local.ty), Some(Type::Function(_))))
            && self.escaping_closure_capture_is_mutated(capture.source_local)
        {
            return true;
        }
        if capture.mode == smelt_hir::CaptureMode::ByValue {
            return false;
        }
        self.closure_capture_body_writes(closure, capture)
    }

    /// Returns whether an escaping closure mutates a captured source binding.
    ///
    /// Escaping closures can outlive their defining stack frame and can also
    /// share a captured binding with sibling closures. If any escaping closure
    /// writes the source binding, all escaping closures that capture it must
    /// use shared storage so read-only siblings observe the updated value.
    fn escaping_closure_capture_is_mutated(&self, source_local: LocalId) -> bool {
        self.mir.closures.iter().any(|candidate| {
            candidate.escapes
                && candidate.captures.iter().any(|candidate_capture| {
                    candidate_capture.source_local == source_local
                        && self.closure_capture_body_writes(candidate, candidate_capture)
                })
        })
    }

    /// Returns whether a closure body assigns to or mutates a captured local.
    pub(super) fn closure_capture_body_writes(
        &self,
        closure: &MirClosure,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        let Some(target) = capture.target_local else {
            return false;
        };
        closure.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| match statement {
                Statement::Assign { dest, .. } if *dest == target => true,
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
                } if *candidate == target => true,
                Statement::Assign { value, .. } => self.rvalue_mutates_local(value, target),
                _ => false,
            })
        })
    }

    /// Returns whether `local` may be read before a real MIR assignment.
    fn local_may_be_used_before_assignment(&self, local: LocalId) -> Result<bool, EmitError> {
        let assigned = self.function.params.contains(&local);
        self.block_may_use_local_before_assignment(
            self.function.entry,
            local,
            assigned,
            &mut HashSet::new(),
        )
    }

    /// Walk one control-flow path while tracking definite assignment for `local`.
    fn block_may_use_local_before_assignment(
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

    /// Returns whether `source` can be coerced before wrapping into `Option`.
    fn can_coerce_to_optional_inner(&self, source: TypeId, inner: TypeId) -> bool {
        matches!(
            self.mir.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(inner)
            || self.structural_record_adapter_available(source, inner)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(inner)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (Some(Type::List(_)), Some(Type::Tuple(_)))
                    | (Some(Type::List(_)), Some(Type::List(_)))
                    | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
            )
    }

    /// Returns the generated fields for a concrete class/interface storage type.
    pub(super) fn structural_record_fields(&self, ty: TypeId) -> Option<Vec<MirField>> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return Some(crate::classes::effective_interface_fields(
                self.mir, interface,
            ));
        }
        self.mir
            .classes
            .iter()
            .find(|class| class.name == *name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
    }

    /// Returns whether a generated storage type is an interface-shaped record.
    pub(super) fn is_interface_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. })
                if self
                    .mir
                    .interfaces
                    .iter()
                    .any(|interface| interface.name == *name)
        )
    }

    /// Returns true when `source` can be field-wise adapted to `target`.
    ///
    /// TypeScript option bags are structurally compatible, but generated Rust
    /// structs are nominal. This predicate intentionally only enables the
    /// adapter when the destination is an emitted interface record; ordinary
    /// class-to-class conversion still keeps Rust's nominal identity.
    fn structural_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        self.structural_record_adapter_fields(source, target)
            .is_some()
    }

    /// Returns matching source/target fields for a structural record adapter.
    fn structural_record_adapter_fields(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Option<Vec<(Option<MirField>, MirField)>> {
        if source == target || !self.is_structural_record_adapter_target(target) {
            return None;
        }
        let source_fields = self.structural_record_fields(source)?;
        let target_fields = self.structural_record_fields(target)?;
        if target_fields.is_empty() {
            return None;
        }
        let mut adapted_fields = Vec::new();
        for target_field in target_fields {
            let source_field = source_fields
                .iter()
                .find(|field| {
                    self.symbol_name(field.name).ok().map(sanitize_ident)
                        == self.symbol_name(target_field.name).ok().map(sanitize_ident)
                })
                .cloned();
            if source_field.is_none()
                && !matches!(self.mir.types.get(target_field.ty), Some(Type::Optional(_)))
            {
                return None;
            }
            adapted_fields.push((source_field, target_field));
        }
        Some(adapted_fields)
    }

    /// Returns whether a nominal Rust storage type may receive field-wise values.
    ///
    /// Interface records always use structural assignment. Class storage is
    /// also adapted when a source value exposes every destination field, which
    /// covers TypeScript base-class references emitted as concrete Rust
    /// structs.
    fn is_structural_record_adapter_target(&self, target: TypeId) -> bool {
        matches!(self.mir.types.get(target), Some(Type::Class { .. }))
    }

    /// Emits a field-wise adapter between structurally compatible record types.
    fn structural_record_adapter_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(adapted_fields) = self.structural_record_adapter_fields(source, target) else {
            return Ok(None);
        };
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let target_name = sanitize_ident(self.symbol_name(*name)?);
        let mut field_text = Vec::new();
        for (source_field_match, target_field) in adapted_fields {
            let field_name = sanitize_ident(self.symbol_name(target_field.name)?);
            let value = if let Some(source_field) = source_field_match {
                let source_field_name = sanitize_ident(self.symbol_name(source_field.name)?);
                let source_value = format!("smelt_struct_value.{source_field_name}.clone()");
                self.rendered_value_as_type_text(&source_value, source_field.ty, target_field.ty)?
            } else {
                self.default_value(target_field.ty)?
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        Ok(Some(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", ")
        )))
    }

    /// Emits a map-to-record adapter for object literals that were first
    /// materialized as string-keyed dictionaries.
    ///
    /// This preserves TypeScript's structural object-literal assignment
    /// semantics when the frontend has not retained the original literal at the
    /// destination site. Missing optional fields become `None`; missing required
    /// fields fall back to the target field default.
    fn string_dict_record_adapter_text(
        &self,
        value_text: &str,
        source_key: TypeId,
        source_value: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if self.mir.types.get(source_key) != Some(&Type::String)
            || !self.is_structural_record_adapter_target(target)
        {
            return Ok(None);
        }
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let Some(target_fields) = self.structural_record_fields(target) else {
            return Ok(None);
        };
        if target_fields.is_empty() {
            return Ok(None);
        }

        let target_name = sanitize_ident(self.symbol_name(*name)?);
        let mut field_text = Vec::new();
        for field in target_fields {
            let source_key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(source_key);
            let lookup_text = if source_key.contains('_') {
                let mut camel = String::new();
                let mut upper_next = false;
                for ch in source_key.chars() {
                    if ch == '_' {
                        upper_next = true;
                    } else if upper_next {
                        camel.push(ch.to_ascii_uppercase());
                        upper_next = false;
                    } else {
                        camel.push(ch);
                    }
                }
                format!(
                    "smelt_record_map.get({source_key:?}).or_else(|| smelt_record_map.get({camel:?}))"
                )
            } else {
                format!("smelt_record_map.get({source_key:?})")
            };
            let value = if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                if self.can_render_dict_value_as(source_value, *inner) {
                    let mapped = self.rendered_value_as_type_text("value", source_value, *inner)?;
                    format!("{lookup_text}.cloned().map(|value| {mapped})")
                } else {
                    "None".to_owned()
                }
            } else {
                let mapped = self.rendered_value_as_type_text("value", source_value, field.ty)?;
                format!(
                    "{lookup_text}.cloned().map_or({}, |value| {mapped})",
                    self.default_value(field.ty)?
                )
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        Ok(Some(format!(
            "{{ let smelt_record_map = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", ")
        )))
    }

    /// Emits a class/interface record as a string-keyed dictionary.
    ///
    /// Typed source records sometimes flow back into object operations. Rust
    /// structs cannot be matched as maps, so this reconstructs the source
    /// property names and converts each field to the requested map value type.
    fn structural_record_to_string_dict_adapter_text(
        &self,
        value_text: &str,
        source: TypeId,
        target_key: TypeId,
        target_value: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if self.mir.types.get(target_key) != Some(&Type::String) {
            return Ok(None);
        }
        let Some(source_fields) = self.structural_record_fields(source) else {
            return Ok(None);
        };
        if source_fields.is_empty() {
            return Ok(None);
        }

        let mut entries = Vec::new();
        for field in source_fields {
            let key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(key);
            let source_value = format!("smelt_struct_value.{field_name}.clone()");
            let value = if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                let mapped = self.rendered_value_as_type_text("value", *inner, target_value)?;
                format!(
                    "{source_value}.map_or({}, |value| {mapped})",
                    self.default_value(target_value)?
                )
            } else {
                self.rendered_value_as_type_text(&source_value, field.ty, target_value)?
            };
            entries.push(format!("({key:?}.to_owned(), {value})"));
        }

        Ok(Some(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); ::std::collections::HashMap::from([{}]) }}",
            entries.join(", ")
        )))
    }

    /// Returns whether a dictionary value can be meaningfully assigned to a field.
    fn can_render_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        source == target
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)),
                        _
                    )
                    | (
                        _,
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
            )
            || self.is_erased_class_type(source)
            || self.is_erased_class_type(target)
    }

    /// Return the emitted Rust name for a free MIR function.
    ///
    /// Source modules can contain same-named local helper functions. Because
    /// the current backend emits one flat Rust module, duplicate source names
    /// are disambiguated with the MIR function id while unique public names
    /// keep their readable spelling.
    pub(super) fn function_rust_name(&self, function: &MirFunction) -> Result<String, EmitError> {
        self.context
            .function_names
            .get(&function.id)
            .cloned()
            .map_or_else(|| Ok(sanitize_ident(self.symbol_name(function.name)?)), Ok)
    }

    /// Returns the parameter types of a generated function by its emitted Rust name.
    ///
    /// Function values can carry an instantiated generic call type even though
    /// this backend emits one erased Rust function. When a closure call points
    /// back at a generated function symbol, the emitted function signature is
    /// the ABI that call arguments must satisfy.
    pub(super) fn emitted_function_param_types(
        &self,
        rust_name: &str,
    ) -> Result<Option<Vec<TypeId>>, EmitError> {
        Ok(self.context.function_param_types.get(rust_name).cloned())
    }

    /// Returns the emitted return type of a generated function by its Rust name.
    ///
    /// Cross-module calls can reference an imported overload signature in the
    /// caller's MIR while resolving to a concrete Rust function emitted from the
    /// source module. This is the return-side counterpart to emitted parameter
    /// lookup, and lets call sites adapt from the actual Rust ABI.
    pub(super) fn emitted_function_return_type(&self, rust_name: &str) -> Option<TypeId> {
        self.context.function_return_types.get(rust_name).copied()
    }

    /// Return whether an emitted Rust function has a `Result` return ABI.
    pub(super) fn emitted_function_can_throw(&self, rust_name: &str) -> bool {
        self.context
            .function_can_throw
            .get(rust_name)
            .copied()
            .unwrap_or(false)
    }

    /// Return the emitted Rust name for a callback function symbol.
    pub(super) fn callback_function_rust_name(
        &self,
        function: Symbol,
    ) -> Result<String, EmitError> {
        self.context
            .callback_names
            .get(&function)
            .cloned()
            .map_or_else(|| Ok(sanitize_ident(self.symbol_name(function)?)), Ok)
    }

    /// Emits a method or constructor definition.
    /// Emits a method or constructor definition.
    pub(crate) fn emit_method(&mut self, out: &mut String) -> Result<(), EmitError> {
        match self.function.origin {
            HirOrigin::ClassConstructor { .. } => {
                let method_params = self
                    .function
                    .params
                    .iter()
                    .map(|param| {
                        let mutability = if self.local_binding_needs_mut(*param) {
                            "mut "
                        } else {
                            ""
                        };
                        Ok(format!(
                            "{mutability}{}: {}",
                            self.local_name(*param)?,
                            self.parameter_decl_type_text(*param)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                out.push_str(&format!(
                    "    fn new({method_params}) -> {} {{\n",
                    if self.function.can_throw {
                        "Result<Self, Box<dyn std::error::Error>>"
                    } else {
                        "Self"
                    }
                ));
            }
            HirOrigin::ClassMethod { method, .. } => {
                let name = sanitize_ident(self.symbol_name(method)?);
                let method_params = self
                    .function
                    .params
                    .iter()
                    .skip(1)
                    .map(|param| {
                        let mutability = if self.local_binding_needs_mut(*param) {
                            "mut "
                        } else {
                            ""
                        };
                        Ok(format!(
                            "{mutability}{}: {}",
                            self.local_name(*param)?,
                            self.parameter_decl_type_text(*param)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let receiver_text = if method_mutates_this(self.function) {
                    "&mut self"
                } else {
                    "&self"
                };
                let rendered_params = if method_params.is_empty() {
                    receiver_text.to_owned()
                } else {
                    format!("{receiver_text}, {method_params}")
                };
                out.push_str(&format!(
                    "    {}fn {name}({rendered_params}) -> {} {{\n",
                    if self.function.is_async { "async " } else { "" },
                    self.return_type_text(self.function.return_ty)?
                ));
            }
            HirOrigin::Body(_) => return self.emit(out),
        }
        self.emit_block(self.entry_block()?, out)?;
        out.push_str("    }\n");
        Ok(())
    }

    /// Converts a type ID to Rust text for storage positions.
    ///
    /// Struct fields, generic arguments, and other named storage positions
    /// cannot use root `impl Trait`, so function values are rendered as boxed
    /// trait objects here.
    pub(crate) fn type_text_for(mir: &Mir, ty: TypeId) -> Result<String, EmitError> {
        let context = EmitContext::new(mir)?;
        Self::type_text_for_with_context(mir, &context, ty)
    }

    /// Converts a type ID to Rust text using an existing emission context.
    pub(crate) fn type_text_for_with_context(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        FunctionEmitter {
            mir,
            context,
            function: mir
                .functions
                .first()
                .ok_or_else(|| EmitError::new("MIR has no functions"))?,
            names: HashMap::new(),
            mutable_locals: HashSet::new(),
            declared_locals: RefCell::new(HashSet::new()),
            predeclared_locals: HashSet::new(),
            termination_cache: RefCell::new(HashMap::new()),
            loop_exit_cache: RefCell::new(HashMap::new()),
            borrowed_callback_names: HashSet::new(),
            none_ty: ty,
            unknown_local: LocalDecl {
                ty,
                kind: LocalKind::Temp,
                span: Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
            },
        }
        .type_text_with_impl_trait(ty, false)
    }

    /// Converts a type ID to a Rust default expression using an existing context.
    pub(crate) fn default_value_for_with_context(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        FunctionEmitter {
            mir,
            context,
            function: mir
                .functions
                .first()
                .ok_or_else(|| EmitError::new("MIR has no functions"))?,
            names: HashMap::new(),
            mutable_locals: HashSet::new(),
            declared_locals: RefCell::new(HashSet::new()),
            predeclared_locals: HashSet::new(),
            termination_cache: RefCell::new(HashMap::new()),
            loop_exit_cache: RefCell::new(HashMap::new()),
            borrowed_callback_names: HashSet::new(),
            none_ty: ty,
            unknown_local: LocalDecl {
                ty,
                kind: LocalKind::Temp,
                span: Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
            },
        }
        .default_value(ty)
    }

    /// Emits a basic block's statements and terminator.
    /// Returns the Rust suffix needed when calling a throwing function.
    pub(super) fn throwing_call_suffix(&self, callee: &MirFunction) -> &'static str {
        if callee.can_throw && !callee.is_async {
            "?"
        } else {
            ""
        }
    }

    /// Converts an operand to its Rust text representation.
    /// Converts an operand to its Rust text representation.
    pub(super) fn operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) => {
                if matches!(
                    self.mir.types.get(self.place_ty(place)?),
                    Some(Type::Function(_))
                ) || self.type_contains_noncloneable(self.place_ty(place)?)
                {
                    self.place_text(place)
                } else {
                    Ok(format!("{}.clone()", self.place_text(place)?))
                }
            }
            Operand::Move(place) => self.place_text(place),
            Operand::Const(constant) => Ok(constant_text(constant)),
        }
    }

    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    /// Converts an operand to Rust text, wrapping into `SmeltUnknown` when needed.
    pub(super) fn operand_as_type_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if self.operand_ty(operand)? == target
            && !matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
            return self.operand_text(operand);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return self.unknown_wrap_text(operand);
        }
        if self.is_erased_class_type(target) {
            return self.unknown_wrap_text(operand);
        }
        if matches!(operand, Operand::Const(Constant::None)) {
            return self.default_value(target);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            let operand_ty = self.operand_ty(operand)?;
            if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(source_inner)) if matches!(self.mir.types.get(*source_inner), Some(Type::Unknown)))
                && (matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*inner))
            {
                return self.operand_text(operand);
            }
            if self.mir.types.get(operand_ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if operand_ty == *inner {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
            if self.can_coerce_to_optional_inner(operand_ty, *inner) {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
            if matches!(
                self.mir.types.get(*inner),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(*inner)
            {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
            if matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                && matches!(self.mir.types.get(operand_ty), Some(Type::Function(_)))
            {
                return Ok(format!(
                    "Some({})",
                    self.operand_as_type_text(operand, *inner)?
                ));
            }
            if matches!(
                self.mir.types.get(operand_ty),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            ) {
                return Ok("None".to_owned());
            }
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(self.operand_ty(operand)?)
        {
            return self.unknown_cast_text(operand, target);
        }
        if let Some(adapter) = self.structural_record_adapter_text(
            &self.operand_text(operand)?,
            self.operand_ty(operand)?,
            target,
        )? {
            return Ok(adapter);
        }
        if let (Some(Type::Class { .. }), Some(Type::Dict(target_key, target_value))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && let Some(adapter) = self.structural_record_to_string_dict_adapter_text(
            &self.operand_text(operand)?,
            self.operand_ty(operand)?,
            *target_key,
            *target_value,
        )? {
            return Ok(adapter);
        }
        if matches!(self.mir.types.get(target), Some(Type::String))
            && let Some(Type::Class { name, .. }) = self.mir.types.get(self.operand_ty(operand)?)
            && self.symbol_name(*name)? == "RegExp"
        {
            return Ok(format!("{}.source.clone()", self.operand_text(operand)?));
        }
        if self.is_match_fn_result_type(self.operand_ty(operand)?)?
            && !matches!(
                self.mir.types.get(target),
                Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "MatchFnResult"
            )
        {
            return self.unknown_cast_value_text(
                &format!("{}.value.clone()", self.operand_text(operand)?),
                target,
            );
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp"
        ) && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::String)
        {
            return Ok(format!(
                "SmeltRegExp::new({}, String::new())",
                self.operand_text(operand)?
            ));
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Function(_))
        ) && !matches!(
            self.mir.types.get(target),
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && !self.is_erased_class_type(target)
        {
            return self.default_value(target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Int)
        {
            return Ok(format!("({} as f64)", self.operand_text(operand)?));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::Float)
        {
            return Ok(format!(
                "(({} as f64).trunc() as i64)",
                self.operand_text(operand)?
            ));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::String)
        {
            return Ok(format!(
                "{}.parse::<f64>().unwrap_or(0.0)",
                self.operand_text(operand)?
            ));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Function(_))
            )
        {
            if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target, false)? {
                return Ok(adapter);
            }
            if let Some(adapter) = self.function_shape_adapter_text(operand, target, false)? {
                return Ok(adapter);
            }
            let text = self.operand_text(operand)?;
            if let Operand::Copy(place) | Operand::Move(place) = operand
                && self.is_function_parameter_place(place)?
            {
                return self.borrowed_function_handle_text(&text, target);
            }
            if self.is_borrowed_callback_capture_name(&text) {
                return self.borrowed_function_handle_text(&text, target);
            }
            return Ok(format!("{text}.clone()"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
            && matches!(self.mir.types.get(target), Some(Type::Function(_)))
        {
            return Ok(format!(
                "{}.clone().unwrap_or({})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner))
        {
            return Ok(format!("{}.clone().flatten()", self.operand_text(operand)?));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?)
            && *inner == target
        {
            return Ok(format!(
                "{}.clone().unwrap_or({})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(self.operand_ty(operand)?) {
            let value_text = self.rendered_value_as_type_text("value", *inner, target)?;
            return Ok(format!(
                "{}.clone().map_or({}, |value| {value_text})",
                self.operand_text(operand)?,
                self.default_value(target)?
            ));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && source_item != target_item
        {
            let value_text = if matches!(self.mir.types.get(*source_item), Some(Type::List(_)))
                && (matches!(
                    self.mir.types.get(*target_item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*target_item))
            {
                "value.into_smelt_unknown()".to_owned()
            } else {
                self.rendered_value_as_type_text("value", *source_item, *target_item)?
            };
            return Ok(format!(
                "{}.into_iter().map(|value| {value_text}).collect::<Vec<_>>()",
                self.operand_text(operand)?
            ));
        }
        if let Some(Type::List(target_item)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(*target_item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            )
        {
            return Ok(format!(
                "vec![{}]",
                self.operand_as_type_text(operand, *target_item)?
            ));
        }
        if let (Some(Type::Dict(_, _)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && matches!(self.mir.types.get(*target_item), Some(Type::Function(_)))
        {
            return Ok("Vec::new()".to_owned());
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text =
                self.rendered_value_as_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{}.into_iter().map(|(_, value)| {value_text}).collect::<Vec<_>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Dict(target_key, target_value))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let int_ty = self.type_id(Type::Int)?;
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                "index.to_string()".to_owned()
            } else {
                self.rendered_value_as_type_text("index as i64", int_ty, *target_key)?
            };
            let value_text =
                self.rendered_value_as_type_text("value", *source_item, *target_value)?;
            return Ok(format!(
                "{}.into_iter().enumerate().map(|(index, value)| ({key_text}, {value_text})).collect::<::std::collections::HashMap<_, _>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) {
            let value_text = self.operand_text(operand)?;
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.rendered_value_as_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source_key != target_key || source_value != target_value)
        {
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                self.property_key_to_string_text("key", *source_key)?
            } else {
                self.rendered_value_as_type_text("key", *source_key, *target_key)?
            };
            let mapped_value_text =
                self.rendered_value_as_type_text("value", *source_value, *target_value)?;
            return Ok(format!(
                "{}.into_iter().map(|(key, value)| ({key_text}, {mapped_value_text})).collect::<::std::collections::HashMap<_, _>>()",
                self.operand_text(operand)?
            ));
        }
        if let (Some(Type::Dict(source_key, source_value)), Some(Type::Class { .. })) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && let Some(adapter) = self.string_dict_record_adapter_text(
            &self.operand_text(operand)?,
            *source_key,
            *source_value,
            target,
        )? {
            return Ok(adapter);
        }
        if let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) && (source.params.len() < target_function.params.len()
            || (source.may_throw || self.operand_closure_can_throw(operand)?)
                != target_function.may_throw
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown)
            ))
        {
            return self
                .function_shape_adapter_text(operand, target, false)?
                .ok_or_else(|| EmitError::new("function adapter was unexpectedly unavailable"));
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return self.default_value(target);
        }
        self.operand_text(operand)
    }

    /// Return true when a place names a borrowed function parameter.
    pub(super) fn is_function_parameter_place(&self, place: &Place) -> Result<bool, EmitError> {
        let Place::Local(local_id) = place else {
            return Ok(false);
        };
        let local_decl = self.local_decl(*local_id)?;
        if self.local_name(*local_id)?.starts_with("closure_arg_") {
            return Ok(false);
        }
        Ok(matches!(local_decl.kind, LocalKind::Param { .. })
            && matches!(self.mir.types.get(local_decl.ty), Some(Type::Function(_)))
            && !self.function_parameter_requires_owned(*local_id)?)
    }

    /// Return whether a function-typed parameter needs an owned callback handle.
    pub(super) fn function_parameter_requires_owned(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        self.function_parameter_requires_owned_in(self.function, local)
    }

    /// Return whether a function-typed parameter in `function` needs ownership.
    pub(super) fn function_parameter_requires_owned_in(
        &self,
        function: &MirFunction,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let Some(local_decl) = function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
        else {
            return Ok(false);
        };
        if !matches!(local_decl.kind, LocalKind::Param { .. })
            || !matches!(self.mir.types.get(local_decl.ty), Some(Type::Function(_)))
        {
            return Ok(false);
        }
        if matches!(function.origin, HirOrigin::ClassConstructor { .. }) {
            return Ok(true);
        }
        Ok(self
            .context
            .owned_callback_params
            .contains(&(function.id, local)))
    }

    /// Return true when emitted text names a source function parameter.
    ///
    /// Callback bodies can reference captured source parameters through MIR
    /// locals that are not themselves in the nested closure parameter list. The
    /// emitted Rust type is still `&mut dyn FnMut`, so calls must use direct
    /// callable syntax instead of the `Rc<RefCell<_>>` handle path.
    pub(super) fn is_function_parameter_name(&self, name: &str) -> Result<bool, EmitError> {
        if name.starts_with("closure_arg_") {
            return Ok(false);
        }
        Ok(self.function.params.iter().any(|param| {
            self.local_name(*param)
                .is_ok_and(|param_name| param_name == name)
                && self.local_decl(*param).is_ok_and(|local| {
                    matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                })
                && !self
                    .function_parameter_requires_owned(*param)
                    .unwrap_or(false)
        }))
    }

    /// Return true for captured callback parameters that keep borrowed Rust type.
    ///
    /// Closure MIR currently records captured outer callback parameters as
    /// ordinary function-typed locals, so name shape is the remaining signal
    /// that the emitted Rust value is a borrowed `&mut dyn FnMut` rather than
    /// an owned callable handle.
    pub(super) fn is_borrowed_callback_capture_name(&self, name: &str) -> bool {
        self.borrowed_callback_names.contains(name)
    }

    /// Return true when a captured local is a borrowed callback parameter.
    ///
    /// Such captures must not force a `move` closure: moving the `&mut dyn
    /// FnMut` borrow prevents later direct uses in the same generated function.
    pub(super) fn capture_is_borrowed_callback_param(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let decl = self.local_decl(local)?;
        let local_name = self.local_name(local)?.to_owned();
        Ok(
            matches!(self.mir.types.get(decl.ty), Some(Type::Function(_)))
                && self.function.params.iter().any(|param| {
                    self.local_name(*param)
                        .is_ok_and(|param_name| param_name == local_name)
                        && self
                            .function_parameter_requires_owned(*param)
                            .is_ok_and(|requires_owned| !requires_owned)
                }),
        )
    }

    /// Return true when a closure capture symbol names a borrowed callback parameter.
    pub(super) fn capture_symbol_is_borrowed_callback_param(
        &self,
        symbol: Symbol,
        ty: TypeId,
    ) -> Result<bool, EmitError> {
        if !matches!(self.mir.types.get(ty), Some(Type::Function(_))) {
            return Ok(false);
        }
        let capture_name = self.symbol_name(symbol)?;
        Ok(self.function.params.iter().any(|param| {
            self.local_name(*param)
                .is_ok_and(|param_name| param_name == capture_name)
                && self
                    .function_parameter_requires_owned(*param)
                    .is_ok_and(|requires_owned| !requires_owned)
        }))
    }

    /// Render a callback argument for a borrowed function parameter.
    ///
    /// Borrowed callback params are used only when crate-level callback ABI
    /// analysis proves the callee does not retain the callback. Owned callback
    /// values are borrowed from their `Rc<RefCell<_>>` handle for the duration
    /// of the call; already-borrowed callback parameters are reborrowed.
    pub(super) fn borrowed_function_argument_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Function(_))
        ) {
            return self.borrowed_default_function_text(target);
        }
        if let Some(adapter) = self.rest_vector_function_adapter_text(operand, target, true)? {
            return Ok(adapter);
        }
        if let Some(adapter) = self.function_shape_adapter_text(operand, target, true)? {
            return Ok(adapter);
        }
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                let place_text = self.place_text(place)?;
                if self.is_function_parameter_place(place)? {
                    Ok(format!("&mut *{place_text}"))
                } else {
                    Ok(format!("&mut *{place_text}.borrow_mut()"))
                }
            }
            Operand::Const(_) => self.borrowed_default_function_text(target),
        }
    }

    /// Render an inline typed no-op callback for a borrowed function parameter.
    pub(super) fn borrowed_default_function_text(
        &self,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::Function(function)) = self.mir.types.get(target) else {
            return Err(EmitError::new(
                "borrowed default callback target must be a function",
            ));
        };
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "arg{index}: {}",
                    self.type_text_with_impl_trait(*param, false)?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let return_value = self.default_value(function.return_ty)?;
        let return_text = if function.may_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({return_value})")
        } else {
            return_value
        };
        Ok(format!("&mut |{params}| {return_text}"))
    }

    /// Wrap a borrowed callback parameter in a cloneable owned callable handle.
    pub(super) fn borrowed_function_handle_text(
        &self,
        function_text: &str,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::Function(function)) = self.mir.types.get(target) else {
            return Err(EmitError::new(
                "borrowed callback handle target must be a function",
            ));
        };
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "arg{index}: {}",
                    self.type_text_with_impl_trait(*param, false)?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let args = (0..function.params.len())
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "::std::rc::Rc::new(::std::cell::RefCell::new(move |{}| {function_text}({args})))",
            params.join(", ")
        ))
    }

    /// Coerces already-rendered Rust value text from a known source type to a destination type.
    pub(super) fn rendered_value_as_type_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if source == target && !matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            return Ok(value_text.to_owned());
        }
        if source == target && matches!(self.mir.types.get(target), Some(Type::Function(_))) {
            if self.is_borrowed_callback_capture_name(value_text) {
                return self.borrowed_function_handle_text(value_text, target);
            }
            return Ok(format!("{value_text}.clone()"));
        }
        if let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let awaited =
                self.rendered_value_as_type_text("smelt_future_value", *source_item, *target_item)?;
            return Ok(format!(
                "Box::pin(async move {{ let smelt_future_value = {value_text}.await; {awaited} }})"
            ));
        }
        if let (Some(Type::Tuple(source_items)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_items.len() == target_items.len()
        {
            let items_text = source_items
                .iter()
                .zip(target_items.iter())
                .enumerate()
                .map(|(index, (source_item, target_item))| {
                    self.rendered_value_as_type_text(
                        &format!("{value_text}.{index}"),
                        *source_item,
                        *target_item,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return if target_items.len() == 1 {
                Ok(format!("({items_text},)"))
            } else {
                Ok(format!("({items_text})"))
            };
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_)))
            && value_text == "Default::default()"
        {
            return self.default_value(target);
        }
        if let Some(adapter) = self.structural_record_adapter_text(value_text, source, target)? {
            return Ok(adapter);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(target)
        {
            return self.unknown_wrap_value_text(value_text, source);
        }
        if let (Some(Type::Class { .. }), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.structural_record_to_string_dict_adapter_text(
                value_text,
                source,
                *target_key,
                *target_value,
            )?
        {
            return Ok(adapter);
        }
        if matches!(
            self.mir.types.get(source),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(source)
        {
            return self.unknown_cast_value_text(value_text, target);
        }
        if self.is_match_fn_result_type(source)?
            && !matches!(
                self.mir.types.get(target),
                Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "MatchFnResult"
            )
        {
            return self.unknown_cast_value_text(&format!("{value_text}.value.clone()"), target);
        }
        if self.mir.types.get(source) == Some(&Type::None) {
            return self.default_value(target);
        }
        if matches!(self.mir.types.get(source), Some(Type::Function(_)))
            && !matches!(
                self.mir.types.get(target),
                Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !self.is_erased_class_type(target)
        {
            return self.default_value(target);
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::Int)
        {
            return Ok(format!("({value_text} as f64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Int)
            && self.mir.types.get(source) == Some(&Type::Float)
        {
            return Ok(format!("(({value_text} as f64).trunc() as i64)"));
        }
        if self.mir.types.get(target) == Some(&Type::Float)
            && self.mir.types.get(source) == Some(&Type::String)
        {
            return Ok(format!("{value_text}.parse::<f64>().unwrap_or(0.0)"));
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp"
        ) && self.mir.types.get(source) == Some(&Type::String)
        {
            return Ok(format!("SmeltRegExp::new({value_text}, String::new())"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
        {
            let mapped_value = self.rendered_value_as_type_text("value", source, *inner)?;
            return Ok(format!(
                "match {value_text}.clone() {{ SmeltUnknown::Null => None, value => Some({mapped_value}) }}"
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target)
            && source == *inner
        {
            return Ok(format!("Some({value_text})"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            if self.mir.types.get(source) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if self.can_coerce_to_optional_inner(source, *inner) {
                let mapped_value = self.rendered_value_as_type_text(value_text, source, *inner)?;
                return Ok(format!("Some({mapped_value})"));
            }
        }
        if let (Some(Type::Optional(source_inner)), Some(Type::Optional(target_inner))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && self.mir.types.get(*source_inner) == Some(&Type::Optional(*target_inner))
        {
            return Ok(format!("{value_text}.clone().flatten()"));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source)
            && *inner == target
        {
            return Ok(format!(
                "{value_text}.clone().unwrap_or({})",
                self.default_value(target)?
            ));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(source) {
            let mapped_value = self.rendered_value_as_type_text("value", *inner, target)?;
            return Ok(format!(
                "{value_text}.clone().map_or({}, |value| {mapped_value})",
                self.default_value(target)?
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && source_item != target_item
        {
            let item_text =
                self.rendered_value_as_type_text("value", *source_item, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>()"
            ));
        }
        if let Some(Type::List(target_item)) = self.mir.types.get(target)
            && matches!(
                self.mir.types.get(*target_item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && !matches!(
                self.mir.types.get(source),
                Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Tuple(_))
            )
        {
            let item_text = self.rendered_value_as_type_text(value_text, source, *target_item)?;
            return Ok(format!("vec![{item_text}]"));
        }
        if let (Some(Type::List(source_item)), Some(Type::Tuple(target_items))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let items_text = target_items
                .iter()
                .enumerate()
                .map(|(index, target_item)| {
                    let item = format!(
                        "smelt_tuple_values.get({index}).cloned().unwrap_or({})",
                        self.default_value(*source_item)?
                    );
                    self.rendered_value_as_type_text(&item, *source_item, *target_item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let tuple_text = if target_items.len() == 1 {
                format!("({items_text},)")
            } else {
                format!("({items_text})")
            };
            return Ok(format!(
                "{{ let smelt_tuple_values = {value_text}.clone(); {tuple_text} }}"
            ));
        }
        if let (Some(Type::Dict(_, source_value)), Some(Type::List(target_item))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let item_text =
                self.rendered_value_as_type_text("value", *source_value, *target_item)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(_, value)| {item_text}).collect::<Vec<_>>()"
            ));
        }
        if let (Some(Type::List(source_item)), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        {
            let int_ty = self.type_id(Type::Int)?;
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                "index.to_string()".to_owned()
            } else {
                self.rendered_value_as_type_text("index as i64", int_ty, *target_key)?
            };
            let item_text =
                self.rendered_value_as_type_text("value", *source_item, *target_value)?;
            return Ok(format!(
                "{value_text}.into_iter().enumerate().map(|(index, value)| ({key_text}, {item_text})).collect::<::std::collections::HashMap<_, _>>()"
            ));
        }
        if let (
            Some(Type::Dict(source_key, source_value)),
            Some(Type::Dict(target_key, target_value)),
        ) = (self.mir.types.get(source), self.mir.types.get(target))
            && (source_key != target_key || source_value != target_value)
        {
            let key_text = if self.mir.types.get(*target_key) == Some(&Type::String) {
                self.property_key_to_string_text("key", *source_key)?
            } else {
                self.rendered_value_as_type_text("key", *source_key, *target_key)?
            };
            let mapped_value_text =
                self.rendered_value_as_type_text("value", *source_value, *target_value)?;
            return Ok(format!(
                "{value_text}.into_iter().map(|(key, value)| ({key_text}, {mapped_value_text})).collect::<::std::collections::HashMap<_, _>>()"
            ));
        }
        if let (Some(Type::Dict(source_key, source_value)), Some(Type::Class { .. })) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && let Some(adapter) = self.string_dict_record_adapter_text(
                value_text,
                *source_key,
                *source_value,
                target,
            )?
        {
            return Ok(adapter);
        }
        Ok(value_text.to_owned())
    }

    /// Render conversion from a JavaScript property-key value to an owned Rust string.
    pub(super) fn property_key_to_string_text(
        &self,
        value_text: &str,
        source_key: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(source_key) {
            Some(Type::String) => Ok(format!("{value_text}.clone()")),
            Some(Type::Bool | Type::Int | Type::Float) => Ok(format!("{value_text}.to_string()")),
            Some(Type::Optional(inner)) => {
                let inner_text = self.property_key_to_string_text("value", *inner)?;
                Ok(format!(
                    "{value_text}.clone().map_or(String::new(), |value| {inner_text})"
                ))
            }
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "RegExp" => {
                Ok(format!("{value_text}.source.clone()"))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Class { .. }) => {
                Ok(format!(
                    "match {value_text} {{ SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned() }}"
                ))
            }
            _ => Ok("\"[object Object]\".to_owned()".to_owned()),
        }
    }

    /// Adapts a concrete callback to a single `Vec<SmeltUnknown>` rest callback.
    fn rest_vector_function_adapter_text(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let Some(0) = target_function.rest else {
            return Ok(None);
        };
        let [rest_param] = target_function.params.as_slice() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        if !matches!(
            self.mir.types.get(*rest_item),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
        ) {
            return Ok(None);
        }
        let function_text = self.operand_text(operand)?;
        let is_borrowed_param = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.is_function_parameter_place(place)?
            }
            Operand::Const(_) => false,
        };
        let args = self.function_args_from_smelt_args_text(source)?;
        let callback_text = if is_borrowed_param {
            function_text.clone()
        } else {
            "smelt_callback".to_owned()
        };
        let call = if is_borrowed_param {
            format!("{callback_text}({args})")
        } else {
            format!("(&mut *{callback_text}.borrow_mut())({args})")
        };
        let source_returns_future =
            matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)));
        let source_async_output_may_throw = source.may_throw
            || self.operand_closure_can_throw(operand)?
            || self
                .type_text_with_impl_trait(self.operand_ty(operand)?, false)?
                .contains("Future<Output = Result");
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) {
            let awaited =
                self.rendered_value_as_type_text("smelt_async_output", *source_item, *target_item)?;
            let target_future_ty =
                self.type_text_with_impl_trait(target_function.return_ty, false)?;
            if is_borrowed_param {
                format!(
                    "Box::pin(async move {{ let smelt_async_output = {call}.await.unwrap_or_else(|error| panic!(\"{{}}\", error)); {awaited} }}) as {target_future_ty}"
                )
            } else {
                let async_call = call.replace(&function_text, "smelt_async_callback");
                format!(
                    "{{ let smelt_async_callback = {function_text}.clone(); Box::pin(async move {{ let smelt_async_output = {async_call}.await.unwrap_or_else(|error| panic!(\"{{}}\", error)); {awaited} }}) as {target_future_ty} }}"
                )
            }
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            format!("{call}?")
        } else if source.may_throw && !source_returns_future {
            format!("{call}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call
        };
        let converted_return_text = self.rendered_value_as_type_text(
            &call_value,
            source.return_ty,
            target_function.return_ty,
        )?;
        let default_adjusted_return_text = if converted_return_text == "Default::default()"
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) {
            "SmeltUnknown::Null".to_owned()
        } else {
            converted_return_text
        };
        let return_text = if target_function.may_throw
            && source_returns_future
            && !source_async_output_may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(source.return_ty)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await) }})"
            )
        } else if target_function.may_throw
            && default_adjusted_return_text.contains("::std::future::Future")
        {
            default_adjusted_return_text
        } else if target_function.may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(target_function.return_ty)
            && !(source.may_throw && source_returns_future)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await) }})"
            )
        } else if target_function.may_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({default_adjusted_return_text})")
        } else {
            default_adjusted_return_text
        };
        let closure = format!("move |smelt_args: Vec<SmeltUnknown>| {return_text}");
        Ok(Some(if borrowed {
            format!("&mut {closure}")
        } else if is_borrowed_param {
            format!("::std::rc::Rc::new(::std::cell::RefCell::new({closure}))")
        } else {
            format!(
                "{{ let smelt_callback = {function_text}.clone(); ::std::rc::Rc::new(::std::cell::RefCell::new({closure})) }}"
            )
        }))
    }

    /// Render concrete callback arguments extracted from an erased JS argument vector.
    pub(super) fn function_args_from_smelt_args_text(
        &self,
        function: &FunctionType,
    ) -> Result<String, EmitError> {
        function
            .params
            .iter()
            .enumerate()
            .map(|(index, param_ty)| {
                if function.rest == Some(index)
                    && let Some(Type::List(item_ty)) = self.mir.types.get(*param_ty)
                {
                    if self.mir.types.get(*item_ty) == Some(&Type::Unknown) {
                        return Ok(format!(
                            "smelt_args.iter().skip({index}).cloned().collect::<Vec<_>>()"
                        ));
                    }
                    let item_text = self.unknown_cast_value_text("value", *item_ty)?;
                    return Ok(format!(
                        "smelt_args.iter().skip({index}).cloned().map(|value| {item_text}).collect::<Vec<_>>()"
                    ));
                }
                let item = format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                let value = self.unknown_cast_value_text(&item, *param_ty)?;
                if function
                    .required_params
                    .is_some_and(|required_params| index >= required_params)
                {
                    let default = self.default_value(*param_ty)?;
                    Ok(format!(
                        "if smelt_args.get({index}).is_some() {{ {value} }} else {{ {default} }}"
                    ))
                } else {
                    Ok(value)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|args| args.join(", "))
    }

    /// Adapt a callback to a compatible target callback shape.
    fn function_shape_adapter_text(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
    ) -> Result<Option<String>, EmitError> {
        let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) else {
            return Ok(None);
        };
        let parameter_mismatch = source.params.len() != target_function.params.len()
            || source
                .params
                .iter()
                .zip(target_function.params.iter())
                .any(|(source_param, target_param)| source_param != target_param);
        let return_mismatch = source.return_ty != target_function.return_ty;
        let throws_mismatch = (source.may_throw || self.operand_closure_can_throw(operand)?)
            != target_function.may_throw;
        if !parameter_mismatch && !return_mismatch && !throws_mismatch {
            return Ok(None);
        }
        let (Operand::Copy(place) | Operand::Move(place)) = operand else {
            return Ok(None);
        };
        let function_text = if self.is_function_parameter_place(place)? {
            format!("&mut *{}", self.place_text(place)?)
        } else {
            self.place_text(place)?
        };
        let arg_decls = target_function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "arg{index}: {}",
                    self.type_text_with_impl_trait(*param, false)?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let forwarded = source
            .params
            .iter()
            .enumerate()
            .map(|(index, source_param)| {
                if source.rest == Some(index)
                    && target_function.params.len() > source.params.len()
                    && let Some(Type::List(source_item)) = self.mir.types.get(*source_param)
                    && target_function.params.get(index).is_some()
                {
                    let mut text = String::from("{ let mut smelt_forwarded_args = Vec::new(); ");
                    for (target_index, target_param) in
                        target_function.params.iter().enumerate().skip(index)
                    {
                        if let Some(Type::List(target_item)) = self.mir.types.get(*target_param) {
                            let item_text = if matches!(
                                self.mir.types.get(*source_item),
                                Some(Type::Unknown | Type::Never | Type::None)
                            ) && self.mir.types.get(*target_item) == Some(&Type::Unknown)
                            {
                                "value".to_owned()
                            } else {
                                self.rendered_value_as_type_text(
                                    "value",
                                    *target_item,
                                    *source_item,
                                )?
                            };
                            text.push_str(&format!(
                                "smelt_forwarded_args.extend(arg{target_index}.into_iter().map(|value| {item_text})); "
                            ));
                        } else {
                            let item_text = self.rendered_value_as_type_text(
                                &format!("arg{target_index}"),
                                *target_param,
                                *source_item,
                            )?;
                            text.push_str(&format!("smelt_forwarded_args.push({item_text}); "));
                        }
                    }
                    text.push_str("smelt_forwarded_args }");
                    return Ok(text);
                }
                if let Some(target_param) = target_function.params.get(index) {
                    self.rendered_value_as_type_text(
                        &format!("arg{index}"),
                        *target_param,
                        *source_param,
                    )
                } else {
                    self.default_value(*source_param)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let (call_text, callback_prelude) = if self.is_function_parameter_place(place)? {
            (format!("({function_text})({forwarded})"), None)
        } else {
            (
                format!("(&mut *_smelt_adapted_callback.borrow_mut())({forwarded})"),
                Some(format!(
                    "let mut _smelt_adapted_callback = {function_text}.clone();"
                )),
            )
        };
        let uses_adapted_callback = callback_prelude.is_some();
        let source_returns_future =
            matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)));
        let source_async_output_may_throw = source.may_throw
            || self.operand_closure_can_throw(operand)?
            || self
                .type_text_with_impl_trait(self.operand_ty(operand)?, false)?
                .contains("Future<Output = Result");
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) {
            let awaited =
                self.rendered_value_as_type_text("smelt_async_output", *source_item, *target_item)?;
            let target_future_ty =
                self.type_text_with_impl_trait(target_function.return_ty, false)?;
            if uses_adapted_callback {
                let async_call =
                    call_text.replace("_smelt_adapted_callback", "smelt_async_callback");
                format!(
                    "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); Box::pin(async move {{ let smelt_async_output = {async_call}.await.unwrap_or_else(|error| panic!(\"{{}}\", error)); {awaited} }}) as {target_future_ty} }}"
                )
            } else {
                format!(
                    "Box::pin(async move {{ let smelt_async_output = {call_text}.await.unwrap_or_else(|error| panic!(\"{{}}\", error)); {awaited} }}) as {target_future_ty}"
                )
            }
        } else if source_returns_future
            && uses_adapted_callback
            && let (Some(Type::Future(_)), Some(Type::Future(_))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            )
        {
            let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) else {
                unreachable!();
            };
            let awaited =
                self.rendered_value_as_type_text("smelt_async_output", *source_item, *target_item)?;
            let target_future_ty =
                self.type_text_with_impl_trait(target_function.return_ty, false)?;
            let async_call = call_text.replace("_smelt_adapted_callback", "smelt_async_callback");
            if source_item == target_item
                && self.mir.types.get(*target_item) == Some(&Type::Unknown)
            {
                format!(
                    "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); Box::pin(async move {{ let smelt_async_output = {async_call}.await.unwrap_or_else(|error| panic!(\"{{}}\", error)); {awaited} }}) as {target_future_ty} }}"
                )
            } else {
                format!(
                    "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); Box::pin(async move {{ let smelt_async_output = {async_call}.await; {awaited} }}) as {target_future_ty} }}"
                )
            }
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            format!("{call_text}?")
        } else if source.may_throw && !source_returns_future {
            format!("{call_text}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call_text
        };
        let converted_return_text = if source_returns_future && uses_adapted_callback {
            call_value.clone()
        } else {
            self.rendered_value_as_type_text(
                &call_value,
                source.return_ty,
                target_function.return_ty,
            )?
        };
        let field_adjusted_return_text = if matches!(
            self.mir.types.get(target_function.return_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.class_has_no_known_fields(source.return_ty)
        {
            call_value
        } else {
            converted_return_text
        };
        let default_adjusted_return_text = if field_adjusted_return_text == "Default::default()"
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) {
            "SmeltUnknown::Null".to_owned()
        } else {
            field_adjusted_return_text
        };
        let return_text = if target_function.may_throw
            && source_returns_future
            && !source_async_output_may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(source.return_ty)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await) }})"
            )
        } else if target_function.may_throw
            && default_adjusted_return_text.contains("::std::future::Future")
        {
            default_adjusted_return_text
        } else if target_function.may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(target_function.return_ty)
            && !(source.may_throw && source_returns_future)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await) }})"
            )
        } else if target_function.may_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({default_adjusted_return_text})")
        } else {
            default_adjusted_return_text
        };
        let closure = if let Some(prelude) = callback_prelude {
            format!(
                "{{ {prelude} move |{}| {return_text} }}",
                arg_decls.join(", ")
            )
        } else {
            format!("move |{}| {return_text}", arg_decls.join(", "))
        };
        Ok(Some(if borrowed {
            format!("&mut {closure}")
        } else {
            format!("::std::rc::Rc::new(::std::cell::RefCell::new({closure}))")
        }))
    }

    /// Return true for structural class/interface placeholders that have no
    /// emitted fields Smelt can use to construct an erased object.
    pub(super) fn class_has_no_known_fields(&self, ty: TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
            return crate::classes::effective_class_fields(self.mir, class).is_empty();
        }
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return crate::classes::effective_interface_fields(self.mir, interface).is_empty();
        }
        true
    }

    /// Adapts a concrete callback to Remeda's erased purry callback surface.
    pub(super) fn rest_vector_unknown_adapter_text(
        &self,
        operand: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let function_text = self.operand_text(operand)?;
        let args = self.function_args_from_smelt_args_text(source)?;
        let needs_owned_callback = matches!(
            operand,
            Operand::Copy(place) | Operand::Move(place) if !self.is_function_parameter_place(place)?
        );
        let callback_text = if needs_owned_callback {
            "smelt_callback".to_owned()
        } else {
            function_text.clone()
        };
        let call = match operand {
            Operand::Copy(place) | Operand::Move(place)
                if !self.is_function_parameter_place(place)? =>
            {
                format!("(&mut *{callback_text}.borrow_mut())({args})")
            }
            _ => format!("{callback_text}({args})"),
        };
        let return_text = if self.mir.types.get(source.return_ty) == Some(&Type::None) {
            if source.may_throw {
                format!(
                    "{{ {call}?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null) }}"
                )
            } else {
                format!(
                    "{{ {call}; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null) }}"
                )
            }
        } else if self.class_has_no_known_fields(source.return_ty) {
            if source.may_throw {
                call
            } else {
                format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({call})")
            }
        } else if source.may_throw {
            let value = self.unknown_wrap_value_text(&format!("{call}?"), source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        } else {
            let value = self.unknown_wrap_value_text(&call, source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        };
        let closure = format!(
            "::std::rc::Rc::new(::std::cell::RefCell::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}))"
        );
        Ok(Some(if needs_owned_callback {
            format!("{{ let smelt_callback = {function_text}.clone(); {closure} }}")
        } else {
            closure
        }))
    }

    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    /// Gets the type of an operand.
    pub(super) fn operand_ty(&self, operand: &Operand) -> Result<TypeId, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Const(Constant::None) => Ok(self.none_ty),
            Operand::Const(Constant::Bool(_)) => self.type_id(Type::Bool),
            Operand::Const(Constant::Int(_)) => self.type_id(Type::Int),
            Operand::Const(Constant::Float(_)) => self.type_id(Type::Float),
            Operand::Const(Constant::String(_)) => self.type_id(Type::String),
            Operand::Const(Constant::Symbol(_)) => self.type_id(Type::Unknown),
        }
    }

    /// Returns whether a type contains a non-cloneable function value.
    pub(super) fn type_contains_function(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Function(_)) => true,
            Some(
                Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item),
            ) => self.type_contains_function(*item),
            Some(Type::Dict(key, value)) => {
                self.type_contains_function(*key) || self.type_contains_function(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => {
                items.iter().any(|item| self.type_contains_function(*item))
            }
            Some(
                Type::None
                | Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. }
                | Type::Class { .. },
            )
            | None => false,
        }
    }

    /// Returns whether a type contains values that cannot be cloned by the
    /// generated Rust representation.
    pub(super) fn type_contains_noncloneable(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Future(_)) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
                self.type_contains_noncloneable(*item)
            }
            Some(Type::Dict(key, value)) => {
                self.type_contains_noncloneable(*key) || self.type_contains_noncloneable(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .any(|item| self.type_contains_noncloneable(*item)),
            _ => false,
        }
    }

    /// Returns whether a type contains an erased `SmeltUnknown` value.
    pub(super) fn type_contains_unknown(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
            Some(
                Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item),
            ) => self.type_contains_unknown(*item),
            Some(Type::Dict(key, value)) => {
                self.type_contains_unknown(*key) || self.type_contains_unknown(*value)
            }
            Some(Type::Tuple(items)) => items.iter().any(|item| self.type_contains_unknown(*item)),
            Some(
                Type::None
                | Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Never
                | Type::Function(_)
                | Type::Class { .. },
            )
            | None => false,
        }
    }

    /// Returns whether a Rust `HashSet` can store values of this type.
    pub(super) fn type_is_hash_set_key_safe(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Float | Type::Dict(_, _) | Type::Set(_) | Type::Function(_)) => false,
            Some(Type::List(_) | Type::Tuple(_)) => false,
            Some(Type::Optional(inner) | Type::Future(inner)) => {
                self.type_is_hash_set_key_safe(*inner)
            }
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.type_is_hash_set_key_safe(*item)),
            Some(
                Type::Bool
                | Type::Int
                | Type::String
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. }
                | Type::None
                | Type::Class { .. },
            )
            | None => true,
        }
    }

    /// Returns whether a class-shaped type is emitted as `SmeltUnknown`.
    pub(super) fn is_erased_class_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, .. }) => {
                if self
                    .symbol_name(*name)
                    .is_ok_and(|type_name| type_name == "RegExp")
                {
                    return false;
                }
                !self.mir.classes.iter().any(|class| class.name == *name)
                    && !self
                        .mir
                        .interfaces
                        .iter()
                        .any(|interface| interface.name == *name)
            }
            _ => false,
        }
    }

    /// Returns whether `default_value` is a concrete literal/container value.
    ///
    /// This excludes classes and callable/composite fallback cases where
    /// `default_value` currently emits `Default::default()` and the generated
    /// Rust type may not implement `Default`.
    pub(super) fn has_plain_default_value(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(
                Type::Bool
                    | Type::Int
                    | Type::Float
                    | Type::String
                    | Type::Unknown
                    | Type::Never
                    | Type::None
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Optional(_)
            )
        )
    }

    /// Converts a place to its Rust text representation.
    /// Gets the entry block of the function.
    pub(super) fn entry_block(&self) -> Result<&BasicBlock, EmitError> {
        self.block(self.function.entry)
    }

    /// Gets a basic block by ID.
    /// Gets a basic block by ID.
    pub(super) fn block(&self, block: smelt_mir::BlockId) -> Result<&BasicBlock, EmitError> {
        self.function
            .blocks
            .get(id_index(block.0, "block index does not fit usize")?)
            .ok_or_else(|| EmitError::new("terminator references an unknown block"))
    }

    /// Gets the declaration of a local by ID.
    /// Gets the declaration of a local by ID.
    pub(super) fn local_decl(&self, local: LocalId) -> Result<&LocalDecl, EmitError> {
        Ok(self
            .function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
            .unwrap_or(&self.unknown_local))
    }

    /// Marks a MIR local as introduced in the current generated Rust function.
    pub(super) fn mark_local_declared(&self, local: LocalId) {
        self.declared_locals.borrow_mut().insert(local);
    }

    /// Returns whether a MIR local has already been introduced in Rust output.
    pub(super) fn is_local_declared(&self, local: LocalId) -> bool {
        self.declared_locals.borrow().contains(&local)
    }

    /// Captures the currently visible Rust local declarations.
    ///
    /// MIR locals are function-scoped, but generated Rust branch bodies create
    /// nested lexical scopes. Code that emits a branch restores this snapshot
    /// after the branch so locals introduced only inside that branch do not
    /// leak into later sibling or outer Rust scopes.
    pub(super) fn declared_locals_snapshot(&self) -> HashSet<LocalId> {
        self.declared_locals.borrow().clone()
    }

    /// Restores a previously captured Rust local declaration scope.
    pub(super) fn restore_declared_locals(&self, snapshot: HashSet<LocalId>) {
        *self.declared_locals.borrow_mut() = snapshot;
    }

    /// Gets the declaration of a local owned by another MIR function.
    ///
    /// MIR local IDs are scoped to their function. Call emission uses this when
    /// adapting arguments to a callee's parameter types, because looking those
    /// IDs up in the caller's local table can silently pick an unrelated local.
    pub(super) fn function_local_decl<'a>(
        &self,
        function: &'a MirFunction,
        local: LocalId,
    ) -> Result<&'a LocalDecl, EmitError> {
        function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
            .ok_or_else(|| EmitError::new("callee local reference out of bounds"))
    }

    /// Gets the generated variable name for a local.
    /// Gets the generated variable name for a local.
    pub(super) fn local_name(&self, local: LocalId) -> Result<&str, EmitError> {
        self.names
            .get(&local)
            .map(String::as_str)
            .map_or(Ok("SmeltUnknown::Null"), Ok)
    }

    /// Gets the string name of a symbol.
    /// Gets the string name of a symbol.
    pub(super) fn symbol_name(&self, symbol: Symbol) -> Result<&str, EmitError> {
        self.mir
            .symbols
            .get(symbol)
            .ok_or_else(|| EmitError::new("MIR references an unknown symbol"))
    }

    /// Return whether a type is Remeda's internal `MatchFnResult` wrapper.
    pub(super) fn is_match_fn_result_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        Ok(matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "MatchFnResult"
        ))
    }

    /// Gets the original source spelling of a symbol when runtime object keys
    /// need JavaScript names rather than Rust-safe normalized identifiers.
    pub(super) fn symbol_source_name(&self, symbol: Symbol) -> Result<&str, EmitError> {
        self.mir
            .names
            .get(symbol)
            .or_else(|| self.mir.symbols.get(symbol))
            .ok_or_else(|| EmitError::new("MIR references an unknown symbol"))
    }
}

/// Returns a unique local name derived from `base_name`.
fn unique_local_name(base_name: String, used: &mut HashSet<String>) -> String {
    if used.insert(base_name.clone()) {
        return base_name;
    }

    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base_name}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

/// Return whether an operand reads a specific local.
fn operand_uses_local(operand: &Operand, local: LocalId) -> bool {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => place_reads_local(place, local),
        Operand::Const(_) => false,
    }
}

/// Return whether reading a place observes a specific local.
fn place_reads_local(place: &Place, local: LocalId) -> bool {
    match place {
        Place::Local(candidate)
        | Place::Field {
            base: candidate, ..
        } => *candidate == local,
        Place::Index { base, index } => *base == local || operand_uses_local(index, local),
    }
}

/// Return whether an assignment place reads a local before writing.
fn assignment_place_reads_local(place: &Place, local: LocalId) -> bool {
    match place {
        Place::Local(_) => false,
        Place::Field { base, .. } => *base == local,
        Place::Index { base, index } => *base == local || operand_uses_local(index, local),
    }
}

/// Return whether an rvalue may read a local.
fn rvalue_uses_local(value: &Rvalue, local: LocalId) -> bool {
    match value {
        Rvalue::Use(operand)
        | Rvalue::Len(operand)
        | Rvalue::NumericAbs(operand)
        | Rvalue::PrimitiveCast { operand, .. }
        | Rvalue::StringCase { operand, .. }
        | Rvalue::StringTrim { operand, .. }
        | Rvalue::Await(operand) => operand_uses_local(operand, local),
        Rvalue::List(items)
        | Rvalue::Set(items)
        | Rvalue::Tuple(items)
        | Rvalue::Closure {
            captures: items, ..
        }
        | Rvalue::NumericExtrema { args: items, .. }
        | Rvalue::NumericHypot { args: items }
        | Rvalue::AsyncOp { args: items, .. } => items
            .iter()
            .any(|operand| operand_uses_local(operand, local)),
        Rvalue::Dict(entries) => entries.iter().any(|(key, entry_value)| {
            operand_uses_local(key, local) || operand_uses_local(entry_value, local)
        }),
        Rvalue::Binary { lhs, rhs, .. }
        | Rvalue::NumericPow {
            base: lhs,
            exponent: rhs,
        }
        | Rvalue::NumericAtan2 { y: lhs, x: rhs }
        | Rvalue::StringAffix {
            haystack: lhs,
            needle: rhs,
            ..
        }
        | Rvalue::StringSearch {
            haystack: lhs,
            needle: rhs,
            ..
        }
        | Rvalue::StringContains {
            haystack: lhs,
            needle: rhs,
        }
        | Rvalue::ListContains {
            list: lhs,
            item: rhs,
        }
        | Rvalue::SetContains {
            set: lhs,
            item: rhs,
        }
        | Rvalue::ListCallback {
            list: lhs,
            callback: rhs,
            ..
        }
        | Rvalue::DictContainsKey {
            dict: lhs,
            key: rhs,
        }
        | Rvalue::StringJoin {
            items: lhs,
            separator: rhs,
        } => operand_uses_local(lhs, local) || operand_uses_local(rhs, local),
        Rvalue::Unary { operand, .. }
        | Rvalue::NumericRound { operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::NumericUnaryFunc { operand, .. }
        | Rvalue::UnknownIs { value: operand, .. }
        | Rvalue::UnknownCast { value: operand, .. }
        | Rvalue::DateFromValue { value: operand }
        | Rvalue::InstanceOf { value: operand, .. } => operand_uses_local(operand, local),
        Rvalue::Conditional {
            cond,
            then_operand,
            else_operand,
        } => {
            operand_uses_local(cond, local)
                || operand_uses_local(then_operand, local)
                || operand_uses_local(else_operand, local)
        }
        Rvalue::OptionalField { receiver, .. } => operand_uses_local(receiver, local),
        Rvalue::OptionalIndex { receiver, index } => {
            operand_uses_local(receiver, local) || operand_uses_local(index, local)
        }
        Rvalue::OptionalMethod { receiver, args, .. } => {
            operand_uses_local(receiver, local)
                || args
                    .iter()
                    .any(|operand| operand_uses_local(operand, local))
        }
        Rvalue::OptionalCoalesce { optional, fallback } => {
            operand_uses_local(optional, local) || operand_uses_local(fallback, local)
        }
        Rvalue::Struct { fields, .. } | Rvalue::CallableObjectAssign { props: fields, .. } => {
            fields
                .iter()
                .any(|(_, field_value)| operand_uses_local(field_value, local))
        }
        Rvalue::ExternalClassInstance { args, .. } => args
            .iter()
            .any(|operand| operand_uses_local(operand, local)),
        Rvalue::ClosureCall { callee, args } => {
            operand_uses_local(callee, local)
                || args
                    .iter()
                    .any(|operand| operand_uses_local(operand, local))
        }
        Rvalue::ClosureCallSpread { callee, args } => {
            operand_uses_local(callee, local) || operand_uses_local(args, local)
        }
        _ => false,
    }
}

/// Return whether a terminator may read a local.
fn terminator_uses_local(terminator: &Terminator, local: LocalId) -> bool {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => false,
        Terminator::Call { callee, args, .. } => {
            matches!(callee, Callee::Indirect(operand) if operand_uses_local(operand, local))
                || args.iter().any(|arg| operand_uses_local(arg, local))
        }
        Terminator::Await { future, .. } => operand_uses_local(future, local),
        Terminator::Switch { cond, .. } => operand_uses_local(cond, local),
        Terminator::Match { scrutinee, .. } => operand_uses_local(scrutinee, local),
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            operand_uses_local(operand, local)
        }
    }
}

/// Return all direct successor blocks of a terminator.
pub(super) fn terminator_successors(terminator: &Terminator) -> Vec<smelt_mir::BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Call { target, unwind, .. } | Terminator::Await { target, unwind, .. } => {
            unwind
                .iter()
                .map(|handler| handler.catch_block)
                .chain(std::iter::once(*target))
                .collect()
        }
        Terminator::Switch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Match { arms, default, .. } => arms
            .iter()
            .map(|arm| arm.target)
            .chain(default.iter().copied())
            .collect(),
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}

/// Compute locals that need function-scope bindings before Rust block emission.
fn predeclared_locals_for_function(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
    let mut seen = function.params.iter().copied().collect::<HashSet<_>>();
    let mut locals = HashSet::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let Statement::Assign { dest, .. } = statement
                && !seen.insert(*dest)
            {
                locals.insert(*dest);
            }
            if let Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            } = statement
                && let Some(closure) = mir
                    .closures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            {
                for capture in &closure.captures {
                    if !seen.contains(&capture.source_local) {
                        locals.insert(capture.source_local);
                    }
                }
            }
        }
    }
    if function.blocks.len() <= 1 {
        return locals;
    }
    for block in &function.blocks {
        if block.id == function.entry {
            continue;
        }
        for statement in &block.statements {
            if let Statement::Assign { dest, .. } = statement {
                locals.insert(*dest);
            }
        }
    }
    locals
}

/// Return whether the current emitted function body already ends in a return.
fn emitted_tail_returns(out: &str) -> bool {
    out.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim_start().starts_with("return "))
}

// Constant formatting continues in `literals.rs`.
