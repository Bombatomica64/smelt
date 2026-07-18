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
            mutable_locals: assigned_locals(mir, context, function),
            declared_locals: RefCell::new(declared_locals),
            predeclared_locals,
            termination_cache: RefCell::new(HashMap::new()),
            loop_exit_cache: RefCell::new(HashMap::new()),
            borrowed_callback_names: HashSet::new(),
            record_conversion_stack: RefCell::new(Vec::new()),
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
            suppress_type_params: RefCell::new(false),
            enclosing_type_params: HashSet::new(),
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
                        // An async method is emitted as a synchronous
                        // `fn(&self) -> SmeltFuture<T>` whose body runs inside a
                        // moved `async` block (see `emit_method`). The moved block
                        // cannot borrow `&self`, so the receiver is cloned once
                        // into an owned `self_owned` handle and every body
                        // reference renders through that name instead of `self`.
                        if function.is_async {
                            "self_owned".to_owned()
                        } else {
                            "self".to_owned()
                        }
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
            self.emit_body(out)?;
            out.push_str("}\n");
            return Ok(());
        }

        // Whether this free function emits real Rust generics was decided once,
        // crate-wide, by `EmitContext::populate_generic_functions` (signature
        // safety + a body-cleanliness trial). Suppressing the type parameters
        // when the function is NOT in that set makes the signature, body, and
        // every call site agree on the erased shape.
        if !self.context.is_generic_function(self.function.id) {
            *self.suppress_type_params.borrow_mut() = true;
        }
        let mut body = String::new();
        self.emit_body(&mut body)?;

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
        // A generic free function emits real Rust generics
        // (`fn identity<T: ..>(x: T) -> T`); the suffix is empty otherwise,
        // including when the body-cleanliness trial forced a fall back to
        // erasure (`suppress_type_params`). In that case the parameters and body
        // are already rendered as `SmeltUnknown`, so declaring `<T>` would leave
        // an unconstrained, uninferable type parameter on the signature.
        let generics = if *self.suppress_type_params.borrow() {
            String::new()
        } else {
            crate::classes::function_impl_generics_text(self.mir, self.function)?
        };
        out.push_str(&format!(
            "{}fn {}{generics}({fn_params}) -> {} {{\n",
            if self.function.is_async { "async " } else { "" },
            self.function_rust_name(self.function)?,
            self.return_type_text(self.function.return_ty)?
        ));
        out.push_str(&body);
        out.push_str("}\n");
        Ok(())
    }

    /// Emit a free function's body (parameter preludes, block, and fallthrough
    /// return) without the signature or closing brace.
    ///
    /// Split out from [`FunctionEmitter::emit`] so a generic free function can
    /// trial-render its body to decide whether real generics are safe (the body
    /// must keep each type parameter opaque). The `main` and test-preamble paths
    /// do not use this helper.
    fn emit_body(&self, out: &mut String) -> Result<(), EmitError> {
        self.emit_shared_parameter_preludes(out)?;
        if self.function.is_test && self.context.needs_date_now_runtime {
            out.push_str("    SMELT_DATE_NOW.with(|value| value.set(None));\n");
        }
        if self.function.is_test && self.context.needs_timer_helpers {
            out.push_str(&format!(
                "    {reset_timers}();\n",
                reset_timers = smelt_stdlib::runtime_symbols::timers::RESET_TIMERS,
            ));
        }
        self.emit_mutable_local_preludes(out)?;
        self.emit_block(self.entry_block()?, out)?;
        // `block_eventually_terminates` walks the MIR CFG, which can keep a
        // phantom fall-through edge that the structured emitter never renders
        // (e.g. a `match` whose every arm returns). `last_emit_diverged`
        // reports whether the *rendered* tail already diverges, so consulting
        // it as well suppresses a trailing `return` that would otherwise be
        // dead `unreachable_code`.
        if !self.block_eventually_terminates(self.function.entry, &mut BlockIdSet::default())?
            && !control_flow::last_emit_diverged()
            && !emitted_tail_returns(out)
        {
            self.emit_fallthrough_return(out)?;
        }
        Ok(())
    }

    /// Reset the per-emission scratch state so a function body can be rendered
    /// again (the generic body-cleanliness trial renders the body once to decide
    /// on generics, then the real emit renders it again).
    ///
    /// Body rendering records which locals have already been declared; that must
    /// start empty on the next pass or the re-rendered body would omit
    /// declarations. Only the parameter locals are pre-declared at function
    /// entry, matching [`FunctionEmitter::new`].
    fn reset_body_emission_state(&self) {
        let mut declared = self.declared_locals.borrow_mut();
        declared.clear();
        declared.extend(self.function.params.iter().copied());
    }

    /// Return whether this generic free function's body keeps every type
    /// parameter opaque, so the function can emit real Rust generics.
    ///
    /// Trial-renders the body with the type parameters in scope and checks
    /// whether the rendered body still needs the erased carrier (see
    /// [`body_needs_erased_carrier`]). A clean body (pure passthrough) renders no
    /// erased tokens and keeps its generics; a body that inspects, compares, or
    /// erases a `T`-typed value falls back to full erasure. The scratch
    /// declared-locals state is reset afterwards so the real emit renders from a
    /// clean slate. The caller only invokes this for signature-generic-safe
    /// functions, so the type parameters are in scope during the trial.
    pub(crate) fn renders_real_generics(&self) -> Result<bool, EmitError> {
        let mut body = String::new();
        self.emit_body(&mut body)?;
        self.reset_body_emission_state();
        Ok(!body_needs_erased_carrier(&body))
    }

    /// Emits shared cells for parameters mutated through lexical closures.
    ///
    /// Parameters already exist at function entry, so their shared binding
    /// must be created before statements can observe it.
    fn emit_shared_parameter_preludes(&self, out: &mut String) -> Result<(), EmitError> {
        for local in &self.function.params {
            if !self.local_uses_shared_capture_storage(*local) {
                continue;
            }
            let name = self.local_name(*local)?;
            // A reference-class receiver is `&self` and cannot be moved into the
            // shared cell, so bind a cheap `self.clone()` (an `Rc::clone` of the
            // handle). The clone shares the SAME underlying object, so both the
            // method body and the escaping closure observe one identity. This is
            // the escaping-`this` mechanism that clears the E0425 cluster.
            if self.is_reference_self_shared_capture(*local) {
                out.push_str(&format!(
                    "    let smelt_capture_{name} = ::std::rc::Rc::new(::std::cell::RefCell::new({name}.clone()));\n"
                ));
            } else {
                out.push_str(&format!(
                    "    let smelt_capture_{name} = ::std::rc::Rc::new(::std::cell::RefCell::new({name}));\n"
                ));
            }
        }
        Ok(())
    }

    /// Emits a conservative return for non-terminating generated control flow.
    ///
    /// Some unstructured MIR shapes cannot yet be rendered as a single Rust
    /// expression with all branch joins preserved. When a non-void function can
    /// fall through, Rust would otherwise infer `()` and report E0308; returning
    /// the type default keeps the generated crate type-correct until the CFG
    /// shape is represented more precisely.
    pub(super) fn emit_fallthrough_return(&self, out: &mut String) -> Result<(), EmitError> {
        // A fallthrough return diverges (or, for `void`, needs no continuation),
        // so the structural tail is terminated either way.
        control_flow::set_last_emit_diverged(true);
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
                && (self.predeclared_local_needs_default(local)?
                    || self.local_may_be_used_before_assignment(local)?)
            {
                continue;
            }
            let mutability = if self.local_binding_needs_mut(local) {
                "mut "
            } else {
                ""
            };
            if self.local_uses_shared_capture_storage(local) {
                out.push_str(&format!(
                    "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>> = ::std::rc::Rc::new(::std::cell::RefCell::new({}));\n",
                    self.type_text_with_impl_trait(decl.ty, false)?,
                    self.default_value(decl.ty)?
                ));
            } else if self.predeclared_local_needs_default(local)?
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
    pub(super) fn predeclared_local_needs_default(&self, local: LocalId) -> Result<bool, EmitError> {
        let decl = self.local_decl(local)?;
        let name = self.local_name(local)?;
        let is_callable = matches!(self.mir.types.get(decl.ty), Some(Type::Function(_)))
            || self
                .type_text_with_impl_trait(decl.ty, false)?
                .contains("dyn Fn");
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

    /// Returns whether a `Function`-typed call-result temp is dead because its
    /// only consumer re-evaluates the call.
    ///
    /// A data-last call such as `capitalize()` produces a value whose MIR local
    /// is typed `Function` (the curried callback) but whose generated function
    /// actually returns `SmeltUnknown`. When that result is only ever *erased*
    /// back to `SmeltUnknown` (e.g. pushed into a `pipe(...)` argument list), the
    /// erase seam re-derives the value by re-rendering the defining call at
    /// `Unknown` (see `coercion::erased_call_assignment_text`). In that case the
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
            || !matches!(self.mir.types.get(self.local_decl(local)?.ty), Some(Type::Function(_)))
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
            Some(target) => Ok(self.target_is_erased(target)),
            None => Ok(false),
        }
    }

    /// Returns whether a local's defining definition is a call the erase seam
    /// can re-render at `Unknown` (a `ClosureCall` statement or `Call`
    /// terminator). Mirrors `coercion::erased_call_assignment_text`'s gate.
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
                if !rvalue_uses_local(value, local) {
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
                && terminator_uses_local(terminator, local)
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
                if items
                    .iter()
                    .filter(|item| operand_uses_local(item, local))
                    .count()
                    != 1
                {
                    return Ok(None);
                }
                let dest_ty = self.local_decl(dest)?.ty;
                let item_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::List(item) | Type::Set(item)) => Some(*item),
                    Some(Type::Tuple(item_tys)) => {
                        let position = items
                            .iter()
                            .position(|item| operand_uses_local(item, local));
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
                        operand_uses_local(key, local) || operand_uses_local(entry_value, local)
                    })
                    .count()
                    != 1
                {
                    return Ok(None);
                }
                let Some((key, _)) = entries
                    .iter()
                    .find(|(_, entry_value)| operand_uses_local(entry_value, local))
                else {
                    return Ok(None);
                };
                if operand_uses_local(key, local) {
                    return Ok(None);
                }
                let dest_ty = self.local_decl(dest)?.ty;
                let value_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::Dict(_, value_ty)) => Some(*value_ty),
                    _ => None,
                };
                Ok(value_ty)
            }
            Rvalue::Use(operand) if operand_uses_local(operand, local) => {
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
            Terminator::Return(operand) if operand_uses_local(operand, local) => {
                Ok(Some(self.function.return_ty))
            }
            _ => Ok(None),
        }
    }

    /// Returns whether a coercion target is an erased boundary (`Unknown`,
    /// a type parameter, a union, or an erased class) — i.e. one that routes a
    /// value through [`Self::erase`].
    fn target_is_erased(&self, target: TypeId) -> bool {
        matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(target)
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

    /// Renders a read of a local through shared closure storage.
    ///
    /// Shared captures live in `Rc<RefCell<T>>`, so a read expands to
    /// `(*smelt_capture_x.borrow())`. The returned `Ref` guard is a temporary
    /// that Rust keeps alive to the end of the FULL enclosing statement, so if
    /// this text were interpolated into a statement that also invokes a closure
    /// re-borrowing the same cell, the nested `borrow_mut` would panic with
    /// "already borrowed" — a RefCell double-borrow crash single-threaded JS
    /// never produces.
    ///
    /// That never happens because MIR is three-address form: every
    /// `Call`/`ClosureCall` is lowered to its own SSA temp binding before any
    /// statement that consumes the result, and the copy-propagation /
    /// move-on-last-use passes only rewrite local aliases (they never fuse a
    /// call rvalue into a consuming statement). So the operands of the
    /// statement this borrow text lands in are always already-materialized
    /// locals or literals — never a live call — and the borrow guard drops at
    /// the statement boundary before any sibling closure can re-enter. Callers
    /// must preserve this: do not build a single emitted statement that both
    /// borrows a shared capture and evaluates a call. See the regression test
    /// `shared_capture_borrow_never_spans_a_sibling_closure_call`.
    pub(super) fn local_value_text(&self, local: LocalId) -> Result<String, EmitError> {
        let name = self.local_name(local)?;
        if name.starts_with("(*smelt_capture_") {
            return Ok(name.replace(".borrow_mut()", ".borrow()"));
        }
        if self.local_uses_shared_capture_storage(local) && self.is_local_declared(local) {
            Ok(format!("(*smelt_capture_{name}.borrow())"))
        } else {
            Ok(name.to_owned())
        }
    }

    /// Renders a mutable receiver or assignment through shared closure storage.
    ///
    /// Shared captures live in `Rc<RefCell<T>>`, so a write/receiver expands to
    /// `(*smelt_capture_x.borrow_mut())`. The returned `RefMut` guard lives to
    /// the end of the FULL enclosing statement; if that statement also invoked
    /// a closure re-borrowing the same cell, the nested borrow would panic with
    /// "already borrowed". The same three-address invariant documented on
    /// [`Self::local_value_text`] prevents this: every call is a separate SSA
    /// temp statement, so an assignment target's `borrow_mut` guard only ever
    /// coexists with already-evaluated value operands (e.g.
    /// `(*smelt_capture_count.borrow_mut()) = _smelt_tmp_N;`), never with a live
    /// call. Callers must not construct a single statement that both takes this
    /// mutable borrow and evaluates a call.
    pub(super) fn local_mut_value_text(&self, local: LocalId) -> Result<String, EmitError> {
        let name = self.local_name(local)?;
        if name.starts_with("(*smelt_capture_") {
            return Ok(name.to_owned());
        }
        if self.local_uses_shared_capture_storage(local) && self.is_local_declared(local) {
            Ok(format!("(*smelt_capture_{name}.borrow_mut())"))
        } else {
            Ok(name.to_owned())
        }
    }

    /// Returns whether `source` can be coerced before wrapping into `Option`.
    pub(super) fn can_coerce_to_optional_inner(&self, source: TypeId, inner: TypeId) -> bool {
        matches!(
            self.mir.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(inner)
            || self.structural_record_adapter_available(source, inner)
            || self.string_dict_record_adapter_available(source, inner)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(inner)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (Some(Type::List(_)), Some(Type::Tuple(_)))
                    | (Some(Type::List(_)), Some(Type::List(_)))
                    | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
                    | (Some(Type::Function(_)), Some(Type::Function(_)))
            )
    }

    /// Returns true when a generated storage field is itself a callable value.
    ///
    /// Callable fields must be read from the struct before method-reference
    /// fallback runs; otherwise abstract virtual slots such as `this.parse`
    /// are erased into placeholder method references instead of dispatching
    /// through the concrete adapter-bound closure.
    pub(super) fn storage_field_is_function(&self, ty: TypeId, field: Symbol) -> bool {
        self.structural_record_fields(ty)
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|candidate| candidate.name == field)
                    .map(|candidate| candidate.ty)
            })
            .is_some_and(|field_ty| matches!(self.mir.types.get(field_ty), Some(Type::Function(_))))
    }

    /// Returns true when a string-keyed dictionary can fill a structural record.
    fn string_dict_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        let Some(Type::Dict(source_key, _)) = self.mir.types.get(source) else {
            return false;
        };
        self.mir.types.get(*source_key) == Some(&Type::String)
            && self.is_structural_record_adapter_target(target)
            && self
                .structural_record_fields(target)
                .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns whether an erased object can be safely expanded into a record.
    ///
    /// Expansion depth is guarded while rendering so callback-bearing records
    /// may retain nested option bags without infinitely expanding cyclic shapes.
    pub(super) fn can_extract_unknown_object_record(&self, target: TypeId) -> bool {
        self.structural_record_fields(target)
            .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns matching source/target fields for a structural record adapter.
    pub(super) fn structural_record_adapter_fields(
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
                && !self.is_virtual_method_storage_field(target, target_field.name)
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
    pub(super) fn structural_record_adapter_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        // A reference class is a handle newtype over `Rc<RefCell<Inner>>`, not a
        // field-wise struct, so it cannot be rebuilt with a `Name { field: .. }`
        // literal (that fails with E0560). When the source already has the
        // target's reference-class type, cloning the handle is the correct
        // adaptation: it shares the same underlying cell, matching JavaScript
        // reference identity. (Cross-type adaptation into a reference class is
        // not modeled here and falls through to `None`.)
        if self.is_reference_class_type(target)
            && matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Class { name: src_name, .. }), Some(Type::Class { name: tgt_name, .. })) if src_name == tgt_name
            )
        {
            return Ok(Some(format!("{value_text}.clone()")));
        }
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
            let value = if let Some(value) =
                self.virtual_method_storage_field_text(source, target, target_field.name)?
            {
                value
            } else if let Some(source_field) = source_field_match {
                let source_field_name = sanitize_ident(self.symbol_name(source_field.name)?);
                let source_value = format!("smelt_struct_value.{source_field_name}.clone()");
                self.value_at_type_text(&source_value, source_field.ty, target_field.ty)?
            } else {
                self.default_value_with_scoped_type_params(target_field.ty, &HashSet::new())?
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

    /// Returns true when `field` is a callable slot that represents a class method.
    ///
    /// Abstract/base classes store virtual method members as function fields so
    /// structurally adapted subclass values can keep overriding behavior after
    /// they are viewed through the base class type.
    fn is_virtual_method_storage_field(&self, ty: TypeId, field: Symbol) -> bool {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) else {
            return false;
        };
        class
            .abstract_methods
            .iter()
            .any(|method| method.name == field)
            || class.methods.iter().any(|method_id| {
                self.function_by_id(*method_id).is_some_and(|function| {
                    matches!(
                        function.origin,
                        HirOrigin::ClassMethod { method, .. } if method == field
                    )
                })
            })
    }

    /// Emits a bound closure for a virtual method storage field when possible.
    ///
    /// The closure captures the concrete source value and dispatches to the
    /// source class implementation. This preserves JavaScript/TypeScript
    /// overridable method behavior for base-typed structural storage such as
    /// `Record<string, Parser<any>>`.
    pub(super) fn virtual_method_storage_field_text(
        &self,
        source: TypeId,
        target: TypeId,
        method: Symbol,
    ) -> Result<Option<String>, EmitError> {
        if !self.is_virtual_method_storage_field(target, method) {
            return Ok(None);
        }
        let Some(Type::Function(function_ty)) = self
            .structural_record_fields(target)
            .and_then(|fields| fields.into_iter().find(|field| field.name == method))
            .and_then(|field| self.mir.types.get(field.ty))
            .cloned()
        else {
            return Ok(None);
        };
        let direct_source_function = self
            .class_for_type(source)
            .and_then(|class| self.find_direct_class_method_function(class.name, method));
        let inherited_source_function = self
            .class_for_type(source)
            .and_then(|class| self.find_class_method_function(class.name, method));
        let dispatches_to_source_field =
            inherited_source_function.is_none() && self.storage_field_is_function(source, method);
        if dispatches_to_source_field {
            let params = function_ty
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    Ok(format!(
                        "arg{index}: {}",
                        self.function_type_param_text(
                            &function_ty,
                            index,
                            *param,
                            &HashSet::new()
                        )?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let param_types = function_ty
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    self.function_type_param_text(&function_ty, index, *param, &HashSet::new())
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let args = function_ty
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    if function_ty.mutable_params.contains(&index) {
                        Ok(format!("arg{index}"))
                    } else {
                        self.value_at_type_text(&format!("arg{index}.clone()"), *param, *param)
                    }
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let method_name = sanitize_ident(self.symbol_name(method)?);
            let call = if args.is_empty() {
                format!("(smelt_method_receiver.{method_name}.clone())()")
            } else {
                format!("(smelt_method_receiver.{method_name}.clone())({args})")
            };
            let body =
                self.value_at_type_text(&call, function_ty.return_ty, function_ty.return_ty)?;
            let return_ty = if function_ty.may_throw {
                format!(
                    "Result<{}, Box<dyn std::error::Error>>",
                    self.type_text_with_impl_trait(function_ty.return_ty, false)?
                )
            } else {
                self.type_text_with_impl_trait(function_ty.return_ty, false)?
            };
            let wrapped_body = if function_ty.may_throw {
                format!("Ok::<_, Box<dyn std::error::Error>>({body})")
            } else {
                body
            };
            return Ok(Some(format!(
                "{{ let smelt_virtual_receiver = smelt_struct_value.clone(); let smelt_virtual_method: ::std::rc::Rc<dyn Fn({param_types}) -> {return_ty}> = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let smelt_method_receiver = smelt_virtual_receiver.clone(); {wrapped_body} }}); smelt_virtual_method }}"
            )));
        }
        let Some(source_function) = direct_source_function
            .or(inherited_source_function)
            .or_else(|| {
                self.class_for_type(target)
                    .and_then(|class| self.find_class_method_function(class.name, method))
            })
        else {
            return Ok(Some(
                self.default_value(
                    self.mir
                        .types
                        .all()
                        .iter()
                        .position(|ty| *ty == Type::Function(function_ty.clone()))
                        .and_then(|index| u32::try_from(index).ok())
                        .map(TypeId)
                        .ok_or_else(|| EmitError::new("virtual method field type is missing"))?,
                )?,
            ));
        };
        let params = function_ty
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "arg{index}: {}",
                    self.function_type_param_text(&function_ty, index, *param, &HashSet::new())?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let param_types = function_ty
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                self.function_type_param_text(&function_ty, index, *param, &HashSet::new())
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let source_arity = if dispatches_to_source_field {
            function_ty.params.len()
        } else {
            source_function.params.len().saturating_sub(1)
        };
        let mut arg_preludes = Vec::new();
        let args = function_ty
            .params
            .iter()
            .take(source_arity)
            .enumerate()
            .map(|(index, target_param)| {
                let source_param = if dispatches_to_source_field {
                    *target_param
                } else {
                    source_function
                        .params
                        .get(index.saturating_add(1))
                        .and_then(|param| self.function_local_decl(source_function, *param).ok())
                        .map_or(*target_param, |decl| decl.ty)
                };
                let arg_source_text = if function_ty.mutable_params.contains(&index) {
                    format!("(*arg{index}).clone()")
                } else {
                    format!("arg{index}.clone()")
                };
                let arg_text =
                    self.value_at_type_text(&arg_source_text, *target_param, source_param)?;
                if !dispatches_to_source_field
                    && let Some(source_local) =
                        source_function.params.get(index.saturating_add(1)).copied()
                    && self.parameter_needs_mutable_reference_in(source_function, source_local)
                {
                    if function_ty.mutable_params.contains(&index) {
                        Ok(format!("arg{index}"))
                    } else {
                        let local_name = format!("smelt_arg_{index}");
                        arg_preludes.push(format!("let mut {local_name} = {arg_text};"));
                        Ok(format!("&mut {local_name}"))
                    }
                } else {
                    Ok(arg_text)
                }
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let receiver_mut = if method_mutates_this(source_function) {
            "mut "
        } else {
            ""
        };
        let receiver_value = if direct_source_function.is_some() || dispatches_to_source_field {
            "smelt_struct_value.clone()".to_owned()
        } else {
            let target_ty = self.type_text_with_impl_trait(target, false)?;
            format!("<{target_ty} as Default>::default()")
        };
        let call = if dispatches_to_source_field {
            if args.is_empty() {
                format!("(smelt_method_receiver.{method_name}.clone())()")
            } else {
                format!("(smelt_method_receiver.{method_name}.clone())({args})")
            }
        } else if args.is_empty() {
            format!("smelt_method_receiver.{method_name}()")
        } else {
            format!("smelt_method_receiver.{method_name}({args})")
        };
        let source_can_throw = !dispatches_to_source_field && source_function.can_throw;
        let source_return_ty = if dispatches_to_source_field {
            function_ty.return_ty
        } else {
            source_function.return_ty
        };
        let adjusted_call = if source_can_throw && function_ty.may_throw {
            call
        } else if source_can_throw {
            format!("{call}.unwrap_or_else(|_| Default::default())")
        } else {
            call
        };
        let body =
            self.value_at_type_text(&adjusted_call, source_return_ty, function_ty.return_ty)?;
        let return_ty = if function_ty.may_throw {
            format!(
                "Result<{}, Box<dyn std::error::Error>>",
                self.type_text_with_impl_trait(function_ty.return_ty, false)?
            )
        } else {
            self.type_text_with_impl_trait(function_ty.return_ty, false)?
        };
        let wrapped_body = if function_ty.may_throw && !source_can_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({body})")
        } else {
            body
        };
        let prelude = if arg_preludes.is_empty() {
            String::new()
        } else {
            format!("{} ", arg_preludes.join(" "))
        };
        Ok(Some(format!(
            "{{ let smelt_virtual_receiver = {receiver_value}; let smelt_virtual_method: ::std::rc::Rc<dyn Fn({param_types}) -> {return_ty}> = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let {receiver_mut}smelt_method_receiver = smelt_virtual_receiver.clone(); {prelude}{wrapped_body} }}); smelt_virtual_method }}"
        )))
    }

    /// Return the MIR class described by a class type.
    fn class_for_type(&self, ty: TypeId) -> Option<&MirClass> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        self.mir.classes.iter().find(|class| class.name == *name)
    }

    /// Find a concrete method implementation on a class or its base chain.
    fn find_class_method_function(
        &self,
        class_name: Symbol,
        method: Symbol,
    ) -> Option<&MirFunction> {
        let class = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == class_name)?;
        for method_id in &class.methods {
            let Some(function) = self.function_by_id(*method_id) else {
                continue;
            };
            if matches!(
                function.origin,
                HirOrigin::ClassMethod { method: function_method, .. }
                    if function_method == method
            ) {
                return Some(function);
            }
        }
        class
            .base
            .and_then(|base| self.find_class_method_function(base, method))
    }

    /// Find a concrete method implementation declared directly on a class.
    fn find_direct_class_method_function(
        &self,
        class_name: Symbol,
        method: Symbol,
    ) -> Option<&MirFunction> {
        let class = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == class_name)?;
        class.methods.iter().find_map(|method_id| {
            let function = self.function_by_id(*method_id)?;
            matches!(
                function.origin,
                HirOrigin::ClassMethod { method: function_method, .. }
                    if function_method == method
            )
            .then_some(function)
        })
    }

    /// Resolve a MIR function by stable function id.
    fn function_by_id(&self, function_id: FuncId) -> Option<&MirFunction> {
        id_index(function_id.0, "function index does not fit usize")
            .ok()
            .and_then(|index| self.mir.functions.get(index))
    }

    /// Emits a map-to-record adapter for object literals that were first
    /// materialized as string-keyed dictionaries.
    ///
    /// This preserves TypeScript's structural object-literal assignment
    /// semantics when the frontend has not retained the original literal at the
    /// destination site. Missing optional fields become `None`; missing required
    /// fields fall back to the target field default.
    pub(super) fn string_dict_record_adapter_text(
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
        // The target class/interface may be parameterized by type params (e.g.
        // `CurriedFunction1<T1, _>`). Those names are only legal to spell in the
        // adapter body when they are actually in scope for the function being
        // emitted. Inside a generic class member or generic free function the
        // enclosing signature declares them, so a field default may keep the
        // generic shape. At a non-generic call site (e.g. a spec function) the
        // same name is unresolvable and must erase to `SmeltUnknown` instead of
        // printing a dangling `T1` (was E0425). Intersect the target's type-param
        // args with the current function's in-scope type params so only genuinely
        // scoped names survive.
        let in_scope = self.current_function_type_params();
        let scoped_type_params = args
            .iter()
            .filter_map(|arg| match self.mir.types.get(*arg) {
                Some(Type::TypeParam {
                    name: type_param_name,
                }) if in_scope.contains(type_param_name) => Some(*type_param_name),
                _ => None,
            })
            .collect::<HashSet<_>>();
        let mut field_text = Vec::new();
        for field in target_fields {
            let field_key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(field_key);
            let lookup_text = if field_key.contains('_') {
                let mut camel = String::new();
                let mut upper_next = false;
                for ch in field_key.chars() {
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
                    "smelt_record_map.get({field_key:?}).or_else(|| smelt_record_map.get({camel:?}))"
                )
            } else {
                format!("smelt_record_map.get({field_key:?})")
            };
            let lookup_value = if let Some(Type::Dict(key, _)) = self.mir.types.get(source_value) {
                if self.dict_uses_smelt_record(*key) {
                    lookup_text
                } else {
                    format!("{lookup_text}.cloned()")
                }
            } else {
                format!("{lookup_text}.cloned()")
            };
            let value = if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                if self.can_render_dict_value_as(source_value, *inner) {
                    let mapped = self.value_at_type_text("value", source_value, *inner)?;
                    format!("{lookup_value}.map(|value| {mapped})")
                } else {
                    "None".to_owned()
                }
            } else if self.can_render_dict_value_as(source_value, field.ty) {
                let mapped = self.value_at_type_text("value", source_value, field.ty)?;
                format!(
                    "{lookup_value}.map_or({}, |value| {mapped})",
                    self.default_value_with_scoped_type_params(field.ty, &scoped_type_params)?
                )
            } else {
                self.default_value_with_scoped_type_params(field.ty, &HashSet::new())?
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        // A reference class is a `Rc<RefCell<Inner>>` newtype, not a named-field
        // struct. The record round-trip must mint a fresh shared cell around the
        // reconstructed inner record rather than emit a struct literal against a
        // tuple struct (was E0560/E0609).
        if self.context.is_reference_class(*name) {
            return Ok(Some(format!(
                "{{ let smelt_record_map = {value_text}.clone(); {target_name}(::std::rc::Rc::new(::std::cell::RefCell::new({target_name}Inner {{ {} }}))) }}",
                field_text.join(", ")
            )));
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
    pub(super) fn structural_record_to_string_dict_adapter_text(
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

        // A reference-class source keeps its fields inside the shared cell, so
        // the read must go through `.0.borrow()` rather than a direct named-field
        // access against the newtype (was E0609).
        let source_is_reference_class = self.is_reference_class_type(source);
        let mut entries = Vec::new();
        for field in source_fields {
            let key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(key);
            let source_value = if source_is_reference_class {
                format!("smelt_struct_value.0.borrow().{field_name}.clone()")
            } else {
                format!("smelt_struct_value.{field_name}.clone()")
            };
            let value = if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                let mapped = self.value_at_type_text("value", *inner, target_value)?;
                format!(
                    "{source_value}.map_or({}, |value| {mapped})",
                    self.default_value(target_value)?
                )
            } else {
                self.value_at_type_text(&source_value, field.ty, target_value)?
            };
            entries.push(format!("({key:?}.to_owned(), {value})"));
        }

        let constructor = if self.dict_uses_smelt_record(target_key) {
            "SmeltRecord::from"
        } else {
            "::std::collections::HashMap::from"
        };
        Ok(Some(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {constructor}([{}]) }}",
            entries.join(", ")
        )))
    }

    /// Returns whether a dictionary value can be meaningfully assigned to a field.
    fn can_render_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        self.can_render_non_function_dict_value_as(source, target)
            || self.can_adapt_rendered_function_value(source, target)
    }

    /// Returns whether a non-callback dictionary value can populate a field.
    fn can_render_non_function_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            return self.can_render_non_function_dict_value_as(source, *inner)
                || matches!(self.mir.types.get(source), Some(Type::None));
        }
        source == target
            || self.structural_record_adapter_available(source, target)
            || self.string_dict_record_adapter_available(source, target)
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

    /// Returns whether a callback map value can be wrapped for a typed field.
    ///
    /// The wrapper may discard arguments, as TypeScript permits, but it cannot
    /// invent a different return value or convert incompatible argument types.
    fn can_adapt_rendered_function_value(&self, source: TypeId, target: TypeId) -> bool {
        let (Some(Type::Function(source_function)), Some(Type::Function(target_function))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return false;
        };
        // The adapter closure takes the *target* parameters and forwards them to
        // the source callback. The source may legally declare more parameters than
        // the target when the surplus are optional (their values are filled with
        // defaults at the forwarding site); this is what lets a `Promise` `resolve`
        // typed `(value?) => void` flow into a `() => void` slot. We still require
        // every target parameter the source actually consumes to be renderable into
        // the matching source parameter type.
        let shared = source_function.params.len().min(target_function.params.len());
        !source_function.is_async
            && !target_function.is_async
            && self.can_render_non_function_dict_value_as(
                source_function.return_ty,
                target_function.return_ty,
            )
            && source_function
                .params
                .iter()
                .take(shared)
                .zip(target_function.params.iter().take(shared))
                .all(|(source_param, target_param)| {
                    self.can_render_non_function_dict_value_as(*target_param, *source_param)
                })
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
                // Reference classes carry interior mutability, so every method
                // takes `&self` uniformly; the `&mut self` decision only applies
                // to by-value value classes. An async method never takes
                // `&mut self`: its body is cloned into an owned handle and runs
                // inside a moved `async` block, so it uniformly borrows `&self`.
                let receiver_text = if method_mutates_this(self.function)
                    && !self.method_owner_is_reference_class()
                    && !self.function.is_async
                {
                    "&mut self"
                } else {
                    "&self"
                };
                let rendered_params = if method_params.is_empty() {
                    receiver_text.to_owned()
                } else {
                    format!("{receiver_text}, {method_params}")
                };
                if self.function.is_async {
                    // Async-method owned-self transform: emit an ordinary
                    // `fn(&self, ..) -> SmeltFuture<T>` that clones `self` into an
                    // owned handle and runs the awaited body inside a moved
                    // `async` block. The returned future therefore owns its state
                    // and is `'static`, so specs can spawn `receiver.method()` as
                    // a detached task without the future borrowing the local
                    // receiver (which previously produced E0597). Reference-class
                    // receivers clone as a cheap `Rc` handle preserving identity;
                    // value classes derive `Clone`.
                    let inner_ret = self.type_text_with_impl_trait(self.function.return_ty, false)?;
                    out.push_str(&format!(
                        "    fn {name}({rendered_params}) -> SmeltFuture<{inner_ret}> {{\n"
                    ));
                    return self.emit_async_method_owned_self_body(&inner_ret, out);
                }
                out.push_str(&format!(
                    "    fn {name}({rendered_params}) -> {} {{\n",
                    self.return_type_text(self.function.return_ty)?
                ));
            }
            HirOrigin::ClassStaticMethod { method, .. } => {
                // A static method takes no receiver: emit every parameter and no
                // `self`, producing an associated function `Class::name(..)`.
                let name = sanitize_ident(self.symbol_name(method)?);
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
                    "    {}fn {name}({method_params}) -> {} {{\n",
                    if self.function.is_async { "async " } else { "" },
                    self.return_type_text(self.function.return_ty)?
                ));
            }
            HirOrigin::Body(_) => return self.emit(out),
        }
        // Reference-class methods may capture `self` into an escaping closure;
        // that receiver must be bound once as a cloned handle before the body.
        // Value-class methods emit no prelude here, keeping their output
        // byte-identical to the pre-reference-class emitter.
        if self.method_owner_is_reference_class() {
            self.emit_shared_parameter_preludes(out)?;
        }
        self.emit_block(self.entry_block()?, out)?;
        out.push_str("    }\n");
        Ok(())
    }

    /// Emits the body of an async method under the owned-self transform.
    ///
    /// The signature (`fn m(&self, ..) -> SmeltFuture<T>`) has already been
    /// written. The awaited body is rendered into a moved `async` block so the
    /// returned future owns its captures and is `'static`. Because the block is
    /// `async move`, it cannot borrow the `&self` receiver, so `self` is cloned
    /// once into an owned `self_owned` handle before the block; the receiver
    /// local renders as `self_owned` throughout the body (see `local_names`),
    /// and reference-class shared-capture preludes clone from that handle. The
    /// clone is only emitted when the body actually references the receiver, so
    /// receiver-free async methods do not trip an unused-variable warning.
    fn emit_async_method_owned_self_body(
        &self,
        inner_ret: &str,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let mut body = String::new();
        if self.method_owner_is_reference_class() {
            self.emit_shared_parameter_preludes(&mut body)?;
        }
        self.emit_block(self.entry_block()?, &mut body)?;
        if body.contains("self_owned") {
            out.push_str("    let self_owned = self.clone();\n");
        }
        out.push_str(&format!(
            "    SmeltFuture::<{inner_ret}>::from_future(Box::pin(async move {{\n"
        ));
        out.push_str(&body);
        out.push_str("    }))\n    }\n");
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
            record_conversion_stack: RefCell::new(Vec::new()),
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
            suppress_type_params: RefCell::new(false),
            enclosing_type_params: HashSet::new(),
        }
        .type_text_with_impl_trait(ty, false)
    }

    /// Converts a type ID to Rust text while preserving the provided type
    /// parameters as real Rust generics.
    ///
    /// This is used for class and interface storage emission, where MIR has
    /// already retained the declaring type parameter list. Other contexts keep
    /// falling back to `SmeltUnknown` for type parameters until function-level
    /// generic declarations are represented in MIR.
    pub(crate) fn type_text_for_with_scoped_type_params(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
        scoped_type_params: &HashSet<Symbol>,
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
            record_conversion_stack: RefCell::new(Vec::new()),
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
            suppress_type_params: RefCell::new(false),
            enclosing_type_params: HashSet::new(),
        }
        .type_text_with_scoped_type_params(ty, false, scoped_type_params)
    }

    /// Converts a type ID to a Rust default expression using an existing context.
    #[expect(
        dead_code,
        reason = "kept for non-generic storage default callers outside the current parse work"
    )]
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
            record_conversion_stack: RefCell::new(Vec::new()),
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
            suppress_type_params: RefCell::new(false),
            enclosing_type_params: HashSet::new(),
        }
        .default_value(ty)
    }

    /// Converts a type ID to a default expression with scoped type parameters.
    pub(crate) fn default_value_for_with_scoped_type_params(
        mir: &Mir,
        context: &EmitContext,
        ty: TypeId,
        scoped_type_params: &HashSet<Symbol>,
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
            record_conversion_stack: RefCell::new(Vec::new()),
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
            suppress_type_params: RefCell::new(false),
            enclosing_type_params: HashSet::new(),
        }
        .default_value_with_scoped_type_params(ty, scoped_type_params)
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
                    // A reference-class field read already clones the value out of
                    // the borrowed cell, so it is owned; a second `.clone()` would
                    // be redundant.
                    || self.place_is_reference_class_field(place)
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

    /// Return whether a function type is represented as an erased JS rest callable.
    pub(super) fn is_erased_unknown_rest_function(&self, function: &FunctionType) -> bool {
        if function.is_async
            || matches!(
                self.mir.types.get(function.return_ty),
                Some(Type::Future(_))
            )
        {
            return false;
        }
        let Some(0) = function.rest else {
            return false;
        };
        let [param] = function.params.as_slice() else {
            return false;
        };
        matches!(
            self.mir.types.get(*param),
            Some(Type::List(item))
                if matches!(
                    self.mir.types.get(*item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
                )
        )
    }

    /// Adapt a concrete callable to an erased JS rest callable while preserving
    /// the source `Function.length` metadata.
    pub(super) fn erased_rest_function_value_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if !self.is_erased_unknown_rest_function(target_function) || target_function.may_throw {
            return Ok(None);
        }
        // The source is ALREADY lowered to `SmeltErasedFunction` (the exact
        // predicate `types.rs` uses to choose that Rust type). Re-wrapping it
        // into a fresh `SmeltErasedFunction` would mint a new callback `Rc`,
        // breaking function reference identity (`Rc::ptr_eq`) for singletons
        // like `doNothing()`. Pass it through unchanged: the caller falls to
        // `{text}.clone()`, which shares the inner `Rc`.
        if let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?)
            && self.is_erased_unknown_rest_function(source)
            && !source.may_throw
        {
            return Ok(None);
        }
        let Some(callback) = self.smelt_erased_function_callback_text(operand, target)? else {
            return Ok(None);
        };
        let length = self.operand_function_length(operand)?;
        Ok(Some(format!(
            "SmeltErasedFunction {{ callback: {callback}, length: {length}.0, object: None }}"
        )))
    }

    /// Adapt a typed function operand to the runtime callback stored by
    /// `SmeltErasedFunction`.
    ///
    /// The erased callable ABI always stores a `Vec<SmeltUnknown> ->
    /// SmeltUnknown` callback, even when the static function type has a more
    /// precise return. This keeps function values first-class while ensuring the
    /// erased representation can be called uniformly by JavaScript-style helper
    /// code.
    fn smelt_erased_function_callback_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if !self.is_erased_unknown_rest_function(target_function) || target_function.may_throw {
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
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            format!("{callback_text}.call({args})")
        } else if is_borrowed_param {
            format!("{callback_text}({args})")
        } else {
            format!("({callback_text})({args})")
        };
        // An async callback carries its throw path inside the returned future's
        // `Result`; invoking the callback yields the future directly. Only a
        // synchronous throwing callback returns a `Result` at this call site.
        let source_returns_future = matches!(
            self.mir.types.get(source.return_ty),
            Some(Type::Future(_))
        );
        let call_value = if source.may_throw && !source_returns_future {
            format!("{call}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let return_text = if self.mir.types.get(source.return_ty) == Some(&Type::None) {
            let null_text = self.null_value_text();
            format!("{{ {call_value}; {null_text} }}")
        } else {
            self.value_at_type_text(&call_value, source.return_ty, unknown_ty)?
        };
        let closure = format!("move |smelt_args: Vec<SmeltUnknown>| {return_text}");
        Ok(Some(if is_borrowed_param {
            format!("::std::rc::Rc::new({closure})")
        } else {
            format!(
                "{{ let smelt_callback = {function_text}.clone(); ::std::rc::Rc::new({closure}) }}"
            )
        }))
    }

    /// Return the JavaScript `Function.length` represented by a function operand.
    pub(super) fn operand_function_length(&self, operand: &Operand) -> Result<usize, EmitError> {
        if let Some(local) = operand_local(operand)
            && let Some(closure_id) = closure_definitions(self.function)?.get(&local).copied()
            && let Some(closure) = self
                .mir
                .closures
                .get(id_index(closure_id.0, "closure index does not fit usize")?)
        {
            return Ok(closure
                .required_params
                .unwrap_or_else(|| closure.rest.unwrap_or(closure.params.len())));
        }

        Ok(match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Function(function)) => function
                .required_params
                .unwrap_or_else(|| function.rest.unwrap_or(function.params.len())),
            _ => 1,
        })
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
    /// emitted Rust type is still `&dyn Fn`, so calls must use direct
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
    /// that the emitted Rust value is a borrowed `&dyn Fn` rather than
    /// an owned callable handle.
    pub(super) fn is_borrowed_callback_capture_name(&self, name: &str) -> bool {
        self.borrowed_callback_names.contains(name)
    }

    /// Return true when a captured local is a borrowed callback parameter.
    ///
    /// Such captures must not force a `move` closure: moving the borrowed
    /// callback prevents later direct uses in the same generated function.
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
    /// values are borrowed from their reentrant `Rc<dyn Fn>` handle for the
    /// duration of the call; already-borrowed callback parameters are reborrowed.
    pub(super) fn borrowed_function_argument_text(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Function(_))
        ) {
            // A dynamically-typed source (e.g. a `SmeltUnknown` element spread
            // from a `...args` rest array) still carries a callable at runtime.
            // Recover the owned `Rc<dyn Fn>` through checked extraction and
            // reborrow it for the duration of the call instead of substituting a
            // no-op default callback, which would silently swap the caller's
            // predicate/mapper for one that always returns the type's default.
            // The erased-unknown-rest shape extracts to a `SmeltErasedFunction`
            // (not an `Rc<dyn Fn>`) so it cannot be reborrowed this way; leave it
            // to the existing default fallback.
            if (matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(self.operand_ty(operand)?))
                && let Some(Type::Function(function)) = self.mir.types.get(target)
                && !self.is_erased_unknown_rest_function(function)
            {
                let owned = self.extract(operand, target)?;
                return Ok(format!("&*({owned})"));
            }
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
                Ok(format!("&*{place_text}"))
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
        Ok(format!("&|{params}| {return_text}"))
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
            "::std::rc::Rc::new(move |{}| {function_text}({args}))",
            params.join(", ")
        ))
    }

    /// Adapt an extracted callback value to a compatible callback field shape.
    ///
    /// Structural record construction emits map values from rendered Rust text,
    /// rather than MIR operands. JavaScript permits object callback fields to
    /// omit arguments supplied by their consumer, so map-extracted callbacks
    /// require the same wrapper semantics as ordinary callback operands.
    pub(super) fn rendered_function_shape_adapter_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let (Some(Type::Function(source_function)), Some(Type::Function(target_function))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return Ok(None);
        };
        // `SmeltErasedFunction` already erases static return differences at the ABI.
        if self.is_erased_unknown_rest_function(source_function)
            && !source_function.may_throw
            && self.is_erased_unknown_rest_function(target_function)
            && !target_function.may_throw
        {
            return Ok(None);
        }
        if source == target
            || !self.can_adapt_rendered_function_value(source, target)
            || matches!(
                self.mir.types.get(source_function.return_ty),
                Some(Type::Future(_))
            )
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Future(_))
            )
        {
            return Ok(None);
        }

        let arg_decls = target_function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "arg{index}: {}",
                    self.function_type_param_text(
                        target_function,
                        index,
                        *param,
                        &HashSet::new(),
                    )?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        // Forward one argument per *source* parameter. Where the target supplies a
        // matching argument (`arg{index}`), coerce it into the source parameter
        // type; where the source declares more (optional) parameters than the
        // target provides, fill the surplus with the source parameter's default
        // value so the source callback is still called with full arity.
        let forwarded = source_function
            .params
            .iter()
            .enumerate()
            .map(|(index, source_param)| match target_function.params.get(index) {
                Some(target_param) => {
                    self.value_at_type_text(&format!("arg{index}"), *target_param, *source_param)
                }
                None => self.default_value(*source_param),
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        // An erased-rest source is a `SmeltErasedFunction` value, which is not a
        // Rust `Fn` and cannot be invoked with call syntax. Route it through the
        // erased callable ABI (`.call(..)`) instead of `(callback)(..)`.
        let call = if self.is_erased_unknown_rest_function(source_function) {
            if self.is_erased_unknown_rest_function(target_function) {
                // The target is itself an erased-rest callable: its single
                // argument already *is* the packed argument list, so forward it
                // unchanged.
                "smelt_callback.call(arg0)".to_owned()
            } else {
                // The target has fixed arity. Pack every positional argument
                // into the erased rest list. The per-source-parameter mapping in
                // `forwarded` would instead coerce `arg0` *into* the list —
                // spreading its elements and dropping `arg1..` — which panics on
                // non-iterable values and corrupts multi-argument adapters (see
                // `partial`/`partialRight` two-argument callbacks).
                let packed = target_function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, target_param)| {
                        self.erase_value_text(&format!("arg{index}"), *target_param)
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                format!("smelt_callback.call(vec![{packed}])")
            }
        } else {
            format!("(smelt_callback)({forwarded})")
        };
        let call_value = if source_function.may_throw && target_function.may_throw {
            format!("{call}?")
        } else if source_function.may_throw {
            format!("{call}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call
        };
        let converted = self.value_at_type_text(
            &call_value,
            source_function.return_ty,
            target_function.return_ty,
        )?;
        let returned = if target_function.may_throw && !source_function.may_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({converted})")
        } else {
            converted
        };
        let target_text = self.type_text_with_impl_trait(target, false)?;
        Ok(Some(format!(
            "{{ let smelt_callback = {value_text}.clone(); let smelt_adapted: {target_text} = ::std::rc::Rc::new(move |{}| {returned}); smelt_adapted }}",
            arg_decls.join(", ")
        )))
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
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(format!("{value_text}.source.clone()"))
            }
            Some(Type::List(item_ty)) => {
                let item_text = self.property_key_to_string_text("value", *item_ty)?;
                Ok(format!(
                    "{value_text}.clone().into_iter().map(|value| {item_text}).collect::<Vec<_>>().join(\",\")"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Class { .. }) => {
                // `smelt_property_key` inspects `SmeltUnknown` discriminants, so
                // its argument must be erased. A concrete union (`SmeltUnion…`) or
                // class instance is not a `SmeltUnknown`; erase it first so the
                // helper receives the runtime shape it matches over (E0308). A
                // source already spelled as `Unknown` erases to itself.
                let erased = self.erase_value_text(value_text, source_key)?;
                Ok(format!(
                    "{{ fn smelt_property_key(value: SmeltUnknown) -> String {{ match value {{ SmeltUnknown::String(value) => value, SmeltUnknown::Symbol(value) => format!(\"__smelt_symbol:{{value}}\"), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(values) => values.into_vec().into_iter().map(smelt_property_key).collect::<Vec<_>>().join(\",\"), SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }} }} smelt_property_key({erased}) }}"
                ))
            }
            _ => Ok("\"[object Object]\".to_owned()".to_owned()),
        }
    }

    /// Adapts a concrete callback to a single `Vec<SmeltUnknown>` rest callback.
    pub(super) fn rest_vector_function_adapter_text(
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
        // When producing an owned value (not a borrowed `&` argument) and the
        // source is ALREADY a `SmeltErasedFunction` of the same erased-rest
        // shape as the target, re-wrapping would mint a fresh callback `Rc` and
        // break function reference identity. Fall through so the caller emits
        // `{text}.clone()` (shares the inner `Rc`). The borrowed path keeps its
        // existing adapter behaviour. Mirrors the guard in
        // `erased_rest_function_value_text`.
        if !borrowed
            && self.is_erased_unknown_rest_function(source)
            && !source.may_throw
        {
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
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            format!("{callback_text}.call({args})")
        } else if is_borrowed_param {
            format!("{callback_text}({args})")
        } else {
            format!("({callback_text})({args})")
        };
        let source_type_text = self.type_text_with_impl_trait(self.operand_ty(operand)?, false)?;
        let source_returns_future = source.is_async
            || matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)))
            || source_type_text.contains("Future<Output");
        let source_async_output_may_throw = source.may_throw
            || self.operand_closure_can_throw(operand)?
            || source_type_text.contains("Future<Output = Result");
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) {
            let awaited =
                self.value_at_type_text("smelt_async_output", *source_item, *target_item)?;
            if is_borrowed_param {
                format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = {call}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }}))"
                )
            } else {
                let async_call = call
                    .replace(&function_text, "smelt_async_callback")
                    .replace("smelt_callback", "smelt_async_callback");
                format!(
                    "{{ let smelt_async_callback = {function_text}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = {async_call}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }})) }}"
                )
            }
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            format!("{call}?")
        } else if source.may_throw && !source_returns_future {
            format!("{call}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call
        };
        let converted_return_text =
            self.value_at_type_text(&call_value, source.return_ty, target_function.return_ty)?;
        let default_adjusted_return_text = if converted_return_text == "Default::default()"
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) {
            self.null_value_text()
        } else {
            converted_return_text
        };
        let return_text = if self.mir.types.get(target_function.return_ty) == Some(&Type::None)
            && !source_returns_future
        {
            if target_function.may_throw {
                format!("{{ {call_value}; Ok::<(), Box<dyn std::error::Error>>(()) }}")
            } else {
                format!("{{ {call_value}; () }}")
            }
        } else if self.mir.types.get(target_function.return_ty) == Some(&Type::None)
            && source_returns_future
        {
            if target_function.may_throw {
                format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); Ok::<(), Box<dyn std::error::Error>>(()) }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                )
            } else {
                format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); () }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                )
            }
        } else if target_function.may_throw
            && source_returns_future
            && !source_async_output_may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(source.return_ty)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await?) }}))"
            )
        } else if target_function.may_throw
            && default_adjusted_return_text.contains("SmeltFuture::")
        {
            default_adjusted_return_text
        } else if target_function.may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(target_function.return_ty)
            && !(source.may_throw && source_returns_future)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await?) }}))"
            )
        } else if target_function.may_throw
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Future(_))
            )
            && source.may_throw
            && source_returns_future
        {
            // A callback that returns a future renders its Rust type as
            // `-> SmeltFuture<..>` with NO outer `Result` — the possible throw is
            // carried inside the future's `Result` output (see the `Type::Function`
            // arm in the default-value emitter). So when both the source and the
            // fallible target return a future, forward the future value directly
            // rather than re-wrapping it in `Ok(..)`. Wrapping here would make the
            // adapter closure return `Result<Future, _>`, and a chain of such
            // adapters (or a later erasure into a promise) would then observe a
            // nested `Result<Result<Future, _>, _>` at its await seam (was E0277).
            default_adjusted_return_text
        } else if target_function.may_throw {
            format!("Ok::<_, Box<dyn std::error::Error>>({default_adjusted_return_text})")
        } else {
            default_adjusted_return_text
        };
        // Match the adapter's parameter to its cast target: a pure-rest callback
        // whose first param is a list is `Fn(SmeltList<..>)`, not `Fn(Vec<..>)`.
        // The body reads `smelt_args` only through `.iter()`/`.get()`/`.skip()`,
        // which work on `SmeltList` via `Deref`.
        let smelt_args_ty = if matches!(
            target_function
                .params
                .first()
                .map(|param| self.mir.types.get(*param)),
            Some(Some(Type::List(_)))
        ) {
            "SmeltList<SmeltUnknown>"
        } else {
            "Vec<SmeltUnknown>"
        };
        let closure = format!("move |smelt_args: {smelt_args_ty}| {return_text}");
        // When the source callback is NOT itself a function parameter, the
        // adapter body references it through a `smelt_callback` binding (see
        // `callback_text`), so that binding must be introduced in the emitted
        // scope. The borrowed (`&mut`) path also needs it: without the enclosing
        // `let smelt_callback = ..`, the moved closure references an unbound
        // `smelt_callback` (E0425). A function-parameter source binds nothing
        // because the closure captures the parameter directly.
        Ok(Some(if borrowed {
            // An owned callback closure refers to a `smelt_callback` binding; a
            // borrowed function parameter is called by its own name and needs no
            // binding. When the source is owned, introduce the `smelt_callback`
            // binding inside the borrowed temporary so the closure body resolves
            // (mirrors the owned `::std::rc::Rc::new` branch below).
            if is_borrowed_param {
                format!("&mut {closure}")
            } else {
                format!("&mut {{ let smelt_callback = {function_text}.clone(); {closure} }}")
            }
        } else if is_borrowed_param {
            format!("::std::rc::Rc::new({closure})")
        } else {
            format!(
                "{{ let smelt_callback = {function_text}.clone(); ::std::rc::Rc::new({closure}) }}"
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
                            "smelt_args.iter().skip({index}).cloned().collect::<SmeltList<_>>()"
                        ));
                    }
                    let item_text = self.extract_value_text("value", *item_ty)?;
                    return Ok(format!(
                        "smelt_args.iter().skip({index}).cloned().map(|value| {item_text}).collect::<SmeltList<_>>()"
                    ));
                }
                let item = format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                let value = self.extract_value_text(&item, *param_ty)?;
                let arg = if function
                    .required_params
                    .is_some_and(|required_params| index >= required_params)
                {
                    let default = self.default_value(*param_ty)?;
                    format!(
                        "if smelt_args.get({index}).is_some() {{ {value} }} else {{ {default} }}"
                    )
                } else {
                    value
                };
                if function.mutable_params.contains(&index) {
                    Ok(format!("&mut ({arg})"))
                } else {
                    Ok(arg)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|args| args.join(", "))
    }

    /// Adapt a callback to a compatible target callback shape.
    pub(super) fn function_shape_adapter_text(
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
        // `SmeltErasedFunction` already erases static return differences at the ABI.
        if self.is_erased_unknown_rest_function(source)
            && !source.may_throw
            && self.is_erased_unknown_rest_function(target_function)
            && !target_function.may_throw
        {
            return Ok(None);
        }
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
        // A borrowed callback parameter is bound as an immutable `&dyn Fn`
        // (see `param_type_text`), so the adapter reborrows it immutably. A
        // `&mut *` reborrow through the shared reference would fail to compile
        // (E0596); the fresh `move` wrapper closure below supplies whatever
        // `FnMut` shape the target expects on its own.
        let function_text = if self.is_function_parameter_place(place)? {
            format!("&*{}", self.place_text(place)?)
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
                        if target_index > index
                            && let Some(Type::List(target_item)) = self.mir.types.get(*target_param)
                        {
                            let item_text = if matches!(
                                self.mir.types.get(*source_item),
                                Some(Type::Unknown | Type::Never | Type::None)
                            ) && self.mir.types.get(*target_item) == Some(&Type::Unknown)
                            {
                                "value".to_owned()
                            } else {
                                self.value_at_type_text(
                                    "value",
                                    *target_item,
                                    *source_item,
                                )?
                            };
                            text.push_str(&format!(
                                "smelt_forwarded_args.extend(arg{target_index}.into_iter().map(|value| {item_text})); "
                            ));
                        } else {
                            let item_text = self.value_at_type_text(
                                &format!("arg{target_index}"),
                                *target_param,
                                *source_item,
                            )?;
                            text.push_str(&format!("smelt_forwarded_args.push({item_text}); "));
                        }
                    }
                    text.push_str("Into::<SmeltList<_>>::into(smelt_forwarded_args) }");
                    return Ok(text);
                }
                if let Some(target_param) = target_function.params.get(index) {
                    self.value_at_type_text(
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
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let (call_text, callback_prelude) = if source_is_erased {
            (
                format!("_smelt_adapted_callback.call({forwarded})"),
                Some(format!(
                    "let _smelt_adapted_callback = {function_text}.clone();"
                )),
            )
        } else if self.is_function_parameter_place(place)? {
            (format!("({function_text})({forwarded})"), None)
        } else {
            // The adapted callback is a cloned, shareable callback value
            // (`Rc<dyn Fn(..)>`-style) that is only *called* and *cloned* here —
            // never reassigned or mutably borrowed — exactly like the erased
            // `.call` path above, which binds it without `mut`. The async
            // rewrite (below) also only `.clone()`s it, so no `mut` is required.
            (
                format!("(_smelt_adapted_callback)({forwarded})"),
                Some(format!(
                    "let _smelt_adapted_callback = {function_text}.clone();"
                )),
            )
        };
        let uses_adapted_callback = callback_prelude.is_some();
        let source_type_text = self.type_text_with_impl_trait(self.operand_ty(operand)?, false)?;
        let source_returns_future = source.is_async
            || matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)))
            || source_type_text.contains("Future<Output");
        let source_async_output_may_throw = source.may_throw
            || self.operand_closure_can_throw(operand)?
            || source_type_text.contains("Future<Output = Result");
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) {
            let awaited =
                self.value_at_type_text("smelt_async_output", *source_item, *target_item)?;
            if uses_adapted_callback {
                let async_call = call_text
                    .replace("_smelt_adapted_callback", "smelt_async_callback")
                    .replace("smelt_callback", "smelt_async_callback");
                format!(
                    "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = {async_call}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }})) }}"
                )
            } else {
                format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = {call_text}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }}))"
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
                return Err(EmitError::new(
                    "async callback adapter requires future types",
                ));
            };
            let awaited =
                self.value_at_type_text("smelt_async_output", *source_item, *target_item)?;
            let async_call = call_text
                .replace("_smelt_adapted_callback", "smelt_async_callback")
                .replace("smelt_callback", "smelt_async_callback");
            format!(
                "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = {async_call}.await?; Ok::<_, Box<dyn std::error::Error>>({awaited}) }})) }}"
            )
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            format!("{call_text}?")
        } else if source.may_throw && !source_returns_future {
            format!("{call_text}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
        } else {
            call_text
        };
        let converted_return_text = if source_returns_future
            && uses_adapted_callback
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Future(_))
            ) {
            // Both sides are promise values (`SmeltFuture<..>`): the future was
            // already rebuilt at the target output type when `call_value` was
            // constructed, so no further coercion is needed.
            call_value.clone()
        } else if source_returns_future {
            // The source returns a promise value but the target return is erased
            // (or otherwise not a future), so erase the `SmeltFuture<T>` to a
            // `SmeltUnknown::Promise` boundary value via the normal coercion.
            self.value_at_type_text(&call_value, source.return_ty, target_function.return_ty)?
        } else if source_is_erased {
            // An erased callable is invoked through `SmeltErasedFunction::call`,
            // which yields a bare `SmeltUnknown` at runtime regardless of the
            // source function's declared return type. Coerce the call result
            // from `Unknown` so the target coercion emits a checked adapter
            // (e.g. `SmeltUnknown` -> `Option<...>` via a Null/Undefined guard)
            // rather than treating the value as if it already had the source
            // return type and calling `Option` methods on a `SmeltUnknown`.
            let unknown_ty = self.type_id(Type::Unknown)?;
            self.value_at_type_text(&call_value, unknown_ty, target_function.return_ty)?
        } else {
            self.value_at_type_text(&call_value, source.return_ty, target_function.return_ty)?
        };
        let field_adjusted_return_text = if !source_returns_future
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && self.class_has_no_known_fields(source.return_ty)
        {
            call_value.clone()
        } else {
            converted_return_text
        };
        let default_adjusted_return_text = if field_adjusted_return_text == "Default::default()"
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) {
            self.null_value_text()
        } else {
            field_adjusted_return_text
        };
        let return_text = if self.mir.types.get(target_function.return_ty) == Some(&Type::None)
            && !source_returns_future
        {
            if target_function.may_throw {
                format!("{{ {call_value}; Ok::<(), Box<dyn std::error::Error>>(()) }}")
            } else {
                format!("{{ {call_value}; () }}")
            }
        } else if self.mir.types.get(target_function.return_ty) == Some(&Type::None)
            && source_returns_future
        {
            if target_function.may_throw {
                format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); Ok::<(), Box<dyn std::error::Error>>(()) }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                )
            } else {
                format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); () }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                )
            }
        } else if target_function.may_throw
            && source_returns_future
            && !source_async_output_may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(source.return_ty)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await?) }}))"
            )
        } else if target_function.may_throw
            && default_adjusted_return_text.contains("SmeltFuture::")
        {
            default_adjusted_return_text
        } else if target_function.may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(target_function.return_ty)
            && !(source.may_throw && source_returns_future)
        {
            let item_text = self.type_text_with_impl_trait(*item, false)?;
            format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, Box<dyn std::error::Error>>({default_adjusted_return_text}.await?) }}))"
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
            format!("::std::rc::Rc::new({closure})")
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

    /// If `operand` is a bare function-item-as-value wrapper, return its crate
    /// unique item cache key and the self-contained erased `SmeltUnknown::Function`
    /// accessor body for that item.
    ///
    /// The body forwards to the function item directly: the wrapper closure has
    /// no captures and references the free function by name, so the resulting
    /// expression captures no outer local and can be lifted verbatim into a
    /// module-level `__smelt_fn_value_<key>()` accessor that caches one shared
    /// erased value. Returns `None` for any operand that is not a bare function
    /// item value (e.g. user arrows), so those keep their fresh per-reference
    /// identity through the ordinary erase path.
    pub(super) fn function_item_erased_accessor(
        &self,
        operand: &Operand,
    ) -> Result<Option<(usize, String)>, EmitError> {
        let Some(local) = operand_local(operand) else {
            return Ok(None);
        };
        let Some(closure_id) = closure_definitions(self.function)?.get(&local).copied() else {
            return Ok(None);
        };
        let Some(closure) = self
            .mir
            .closures
            .get(id_index(closure_id.0, "closure index does not fit usize")?)
        else {
            return Ok(None);
        };
        let Some(key) = closure.function_item_key else {
            return Ok(None);
        };
        let source_ty = self.operand_ty(operand)?;
        let Some(Type::Function(_)) = self.mir.types.get(source_ty) else {
            return Ok(None);
        };
        // Self-contained typed wrapper that forwards to the function item by name
        // (no captures, no local reference): `::std::rc::Rc::new(move |..| func1(..))`.
        let wrapper_text = self.closure_text_for_type(closure_id, source_ty)?;
        // Erased forwarding closure using `smelt_callback` as the bound source.
        let inner = self.erased_rest_forwarding_closure_text(source_ty)?;
        let body =
            format!("SmeltUnknown::Function({{ let smelt_callback = {wrapper_text}; {inner} }})");
        Ok(Some((key, body)))
    }

    /// Build the erased rest-forwarding closure for a function type.
    ///
    /// Emits `::std::rc::Rc::new(move |smelt_args| { .. })` that forwards the
    /// erased `Vec<SmeltUnknown>` argument vector to a callback the caller has
    /// bound as `smelt_callback: Rc<dyn Fn..>`, then erases the result back into
    /// `SmeltUnknown`. The closure is self-contained: it references only
    /// `smelt_callback` and `smelt_args`, never the original operand local, so it
    /// can be reused both by `rest_vector_unknown_adapter_text` (owned callback
    /// path) and by the function-item value accessor.
    ///
    /// This mirrors the `needs_owned_callback` branch of
    /// `rest_vector_unknown_adapter_text`: the call is `smelt_callback.call(..)`
    /// when the source already exposes the erased rest ABI and cannot throw, and
    /// `(smelt_callback)(..)` otherwise. The `return_text` branches (None-return,
    /// fieldless class, may-throw, ordinary erase) match that function exactly.
    pub(super) fn erased_rest_forwarding_closure_text(
        &self,
        source_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(source_ty) else {
            return Err(EmitError::new(
                "erased rest forwarding closure requires a function type",
            ));
        };
        let args = self.function_args_from_smelt_args_text(source)?;
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            format!("smelt_callback.call({args})")
        } else {
            format!("(smelt_callback)({args})")
        };
        let null_text = self.null_value_text();
        let return_text = if self.mir.types.get(source.return_ty) == Some(&Type::None) {
            if source.may_throw {
                format!(
                    "{{ {call}?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>({null_text}) }}"
                )
            } else {
                format!("{{ {call}; Ok::<SmeltUnknown, Box<dyn std::error::Error>>({null_text}) }}")
            }
        } else if matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_))) {
            let value = self.erase_value_text(&call, source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        } else if self.class_has_no_known_fields(source.return_ty) {
            if source.may_throw {
                call
            } else {
                format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({call})")
            }
        } else if source.may_throw {
            let value = self.erase_value_text(&format!("{call}?"), source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        } else {
            let value = self.erase_value_text(&call, source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        };
        Ok(format!(
            "::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text})"
        ))
    }

    /// Adapts a concrete callback to the erased JavaScript callback surface.
    pub(super) fn rest_vector_unknown_adapter_text(
        &self,
        operand: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let source_ty = self.operand_ty(operand)?;
        let Some(Type::Function(source)) = self.mir.types.get(source_ty) else {
            return Ok(None);
        };
        let function_text = self.operand_text(operand)?;
        let needs_owned_callback = matches!(
            operand,
            Operand::Copy(place) | Operand::Move(place) if !self.is_function_parameter_place(place)?
        );
        if needs_owned_callback {
            // Bind the callback as `smelt_callback` and reuse the shared erased
            // forwarding closure builder. The bound name and call shapes match
            // the original owned-callback code, so the emitted text is identical.
            let inner = self.erased_rest_forwarding_closure_text(source_ty)?;
            return Ok(Some(format!(
                "{{ let smelt_callback = {function_text}.clone(); {inner} }}"
            )));
        }
        // Non-owned (function-parameter) path: invoke the callback by its operand
        // text directly, with no binding or extra parentheses. Kept inline so the
        // emitted text remains byte-identical to the previous implementation.
        let args = self.function_args_from_smelt_args_text(source)?;
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            format!("{function_text}.call({args})")
        } else {
            format!("{function_text}({args})")
        };
        let null_text = self.null_value_text();
        let return_text = if self.mir.types.get(source.return_ty) == Some(&Type::None) {
            if source.may_throw {
                format!(
                    "{{ {call}?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>({null_text}) }}"
                )
            } else {
                format!("{{ {call}; Ok::<SmeltUnknown, Box<dyn std::error::Error>>({null_text}) }}")
            }
        } else if matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_))) {
            let value = self.erase_value_text(&call, source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        } else if self.class_has_no_known_fields(source.return_ty) {
            if source.may_throw {
                call
            } else {
                format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({call})")
            }
        } else if source.may_throw {
            let value = self.erase_value_text(&format!("{call}?"), source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        } else {
            let value = self.erase_value_text(&call, source.return_ty)?;
            format!("Ok::<SmeltUnknown, Box<dyn std::error::Error>>({value})")
        };
        Ok(Some(format!(
            "::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text})"
        )))
    }

    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    /// Gets the type of an operand.
    pub(super) fn operand_ty(&self, operand: &Operand) -> Result<TypeId, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Const(Constant::None | Constant::Undefined) => Ok(self.none_ty),
            Operand::Const(Constant::Bool(_)) => self.type_id(Type::Bool),
            Operand::Const(Constant::Int(_)) => self.type_id(Type::Int),
            Operand::Const(Constant::Float(_)) => self.type_id(Type::Float),
            Operand::Const(Constant::String(_)) => self.type_id(Type::String),
            Operand::Const(Constant::Symbol(_)) => self.type_id(Type::Unknown),
        }
    }

    /// Returns the value type produced by awaiting `future`.
    ///
    /// An awaited operand's static type is a `Future<Item>`; the awaited value
    /// is that `Item`. When the operand's type is not spelled as a future (some
    /// promise-handle values flow through erased positions), fall back to the
    /// operand type itself so callers can still drive a coercion against it.
    pub(super) fn awaited_output_ty(&self, future: &Operand) -> Result<TypeId, EmitError> {
        let ty = self.operand_ty(future)?;
        match self.mir.types.get(ty) {
            Some(Type::Future(item)) => Ok(*item),
            _ => Ok(ty),
        }
    }

    /// Returns whether a type contains a non-cloneable function value.
    pub(super) fn type_contains_function(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Function(_)) => true,
            Some(
                Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item),
            ) => self.type_contains_function(*item),
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
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
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
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

    /// Returns whether a Rust `HashSet` is a correct backing for a set of this
    /// element type.
    ///
    /// `HashSet` is used only for value-equality primitives (`bool`, `i64`,
    /// `String`) where Rust `Eq + Hash` both exists and matches JavaScript
    /// SameValueZero. Every other element type — `f64` (no `Eq`), generated
    /// unions and generic type parameters (no `Eq + Hash` bound), and
    /// object-like values whose JS equality is by reference, not structure —
    /// routes through the `SmeltJsSet` runtime container instead, which
    /// projects each element through its `IntoSmeltUnknown` erasure for
    /// SameValueZero membership and preserves insertion order.
    pub(super) fn type_is_hash_set_key_safe(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::String) => true,
            Some(Type::Optional(inner) | Type::Future(inner)) => {
                self.type_is_hash_set_key_safe(*inner)
            }
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.type_is_hash_set_key_safe(*item)),
            _ => false,
        }
    }

    /// Returns whether a dictionary must use JavaScript `Map` key equality.
    ///
    /// Rust `HashMap` is correct for primitive key spaces, but JavaScript Map
    /// compares objects and functions by identity and treats `NaN` as equal to
    /// itself. Dictionaries keyed by erased, generic, or object-like values
    /// therefore use the generated linear `SmeltJsMap` runtime container.
    pub(super) fn dict_uses_js_key_map(&self, key_ty: TypeId) -> bool {
        match self.mir.types.get(key_ty) {
            Some(Type::Bool | Type::Int | Type::String) => false,
            Some(Type::Float) => true,
            Some(Type::Optional(inner) | Type::Future(inner)) => self.dict_uses_js_key_map(*inner),
            Some(Type::Union(items)) => items.iter().any(|item| self.dict_uses_js_key_map(*item)),
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Never
                | Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Tuple(_)
                | Type::Function(_)
                | Type::Class { .. },
            )
            | None => true,
            Some(Type::None) => false,
        }
    }

    /// Returns whether a string-keyed dictionary should carry object identity.
    ///
    /// TypeScript object literals lower to records internally. When one of
    /// those records is later boxed as `unknown`, JavaScript observes the
    /// original object identity for Map/Set keys. `SmeltRecord` preserves that
    /// identity while keeping structural equality for assertions.
    pub(super) fn dict_uses_smelt_record(&self, key_ty: TypeId) -> bool {
        crate::stdlib::needs_unknown_type(self.mir)
            && self.mir.types.get(key_ty) == Some(&Type::String)
    }

    /// Returns whether a Map/dict operation must use the `SmeltRecord` backing.
    ///
    /// Receiver-aware wrapper over [`Self::dict_uses_smelt_record`]. A source
    /// `Map` (`Type::JsMap`) *always* backs onto `SmeltJsMap` — even when
    /// string-keyed — so the `SmeltRecord`-specific emission (notably the
    /// `smelt_is_for_in_record_key` marker filter, which only type-checks over
    /// `SmeltRecord`) must never fire for it. Only a `Type::Dict` receiver can
    /// use the `SmeltRecord` backing, and only under the ordinary key rule.
    pub(super) fn map_op_uses_smelt_record(&self, receiver_ty: TypeId, key_ty: TypeId) -> bool {
        if matches!(self.mir.types.get(receiver_ty), Some(Type::JsMap(_, _))) {
            return false;
        }
        self.dict_uses_smelt_record(key_ty)
    }

    /// Returns whether a Map/dict operation uses the `SmeltJsMap` backing.
    ///
    /// Receiver-aware wrapper over [`Self::dict_uses_js_key_map`]. A source
    /// `Map` (`Type::JsMap`) always backs onto `SmeltJsMap`, so its projections
    /// take the same owned-key/value, symbol-only-filter emission as an
    /// object-keyed dict — regardless of key type. A `Type::Dict` receiver keeps
    /// the ordinary key-driven decision.
    pub(super) fn map_op_uses_js_key_map(&self, receiver_ty: TypeId, key_ty: TypeId) -> bool {
        if matches!(self.mir.types.get(receiver_ty), Some(Type::JsMap(_, _))) {
            return true;
        }
        self.dict_uses_js_key_map(key_ty)
    }

    /// Returns whether two map/dict types lower to different backing containers.
    ///
    /// `Dict` and `JsMap` can share a backing (`Dict` with an object-like key and
    /// any `JsMap` both use `SmeltJsMap`) or differ (a string-keyed `Dict` uses
    /// `SmeltRecord`, a plain-primitive-keyed `Dict` uses `HashMap`, and a
    /// `JsMap` always uses `SmeltJsMap`). A key/value-preserving conversion
    /// (`Object.fromEntries(map)`, an interchangeable `Dict`/`JsMap` assignment)
    /// still needs a real container rebuild when the backings differ, even though
    /// the key and value component types are identical — this reports that case.
    pub(super) fn map_backing_differs(&self, left: &Type, right: &Type) -> bool {
        self.map_backing_tag(left) != self.map_backing_tag(right)
    }

    /// Return a discriminant for a map/dict type's backing container.
    fn map_backing_tag(&self, ty: &Type) -> u8 {
        match ty {
            Type::JsMap(_, _) => 0,
            Type::Dict(key, _) if self.dict_uses_js_key_map(*key) => 0,
            Type::Dict(key, _) if self.dict_uses_smelt_record(*key) => 1,
            Type::Dict(_, _) => 2,
            _ => 3,
        }
    }

    /// Returns whether list membership should use JavaScript SameValueZero.
    ///
    /// `Array.prototype.includes`, `indexOf`, and `splice`-style removals do
    /// not use structural object equality. They compare objects/functions by
    /// reference and treat `NaN` as equal to itself.
    pub(super) fn list_item_uses_same_value_zero(&self, item_ty: TypeId) -> bool {
        match self.mir.types.get(item_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Float) => true,
            Some(Type::Class { .. }) => self.is_erased_class_type(item_ty),
            _ => false,
        }
    }

    /// Return the key/value types of a class's index-signature store field.
    ///
    /// A class declaring `[key: string]: T` (issue #84) carries a synthesized
    /// private `Dict` field (named [`smelt_hir::CLASS_INDEX_STORE_FIELD`]) that
    /// backs dynamic keyed reads/writes at runtime. When `ty` is such a class,
    /// this returns the store `Dict`'s `(key_ty, value_ty)` so keyed access can
    /// be routed to `base.__smelt_index_store` instead of erased to a stub.
    /// Named fields are unaffected; only genuinely dynamic keyed access uses it.
    pub(super) fn class_index_store_types(&self, ty: TypeId) -> Option<(TypeId, TypeId)> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        let class = self.mir.classes.iter().find(|class| class.name == *name)?;
        let store_field = crate::classes::effective_class_fields(self.mir, class)
            .into_iter()
            .find(|field| {
                self.symbol_source_name(field.name)
                    .is_ok_and(|source| source == smelt_hir::CLASS_INDEX_STORE_FIELD)
            })?;
        match self.mir.types.get(store_field.ty) {
            Some(Type::Dict(key_ty, value_ty)) => Some((*key_ty, *value_ty)),
            _ => None,
        }
    }

    /// Return whether a class type declares a named struct field.
    ///
    /// Used to distinguish a declared field access (`x.size`, concrete struct
    /// field) from an undeclared member access that must route to the class
    /// index-signature store (`x.dynamicName`). The synthesized store field
    /// itself is not treated as a named member here.
    pub(super) fn class_has_named_field(&self, ty: TypeId, field: Symbol) -> bool {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) else {
            return false;
        };
        crate::classes::effective_class_fields(self.mir, class)
            .iter()
            .any(|candidate| {
                candidate.name == field
                    && self
                        .symbol_source_name(candidate.name)
                        .is_ok_and(|source| source != smelt_hir::CLASS_INDEX_STORE_FIELD)
            })
    }

    /// Returns whether a type is a callable-interface struct.
    ///
    /// A callable interface (a TypeScript interface with one or more call
    /// signatures) lowers to a class record carrying a synthetic `__smelt_call`
    /// storage field for its underlying callable (see the frontend
    /// `add_interface_call_signature_field`). Detecting that field is how the
    /// emitter recognizes a value that is invoked like a function — for direct
    /// calls and for `.apply`/`.call`/`.bind`, which must operate on the erased
    /// callable rather than on a (non-existent) named struct field.
    pub(super) fn callable_interface_call_field_ty(&self, ty: TypeId) -> Option<TypeId> {
        // Interfaces live in `mir.interfaces` and classes in `mir.classes`;
        // `structural_record_fields` resolves the effective field list for
        // either, so the `__smelt_call` probe works for both record shapes. The
        // returned type is the (function-typed) storage slot for the underlying
        // callable, which the caller erases to invoke the value.
        let fields = self.structural_record_fields(ty)?;
        fields.iter().find_map(|candidate| {
            self.symbol_source_name(candidate.name)
                .ok()
                .filter(|source| *source == "__smelt_call")
                .map(|_| candidate.ty)
        })
    }

    /// Returns whether a place is a declared-field read on a reference class.
    ///
    /// Such a read is lowered as `base.0.borrow().field.clone()`, which is
    /// already an owned value, so callers must not wrap it in another `.clone()`.
    pub(super) fn place_is_reference_class_field(&self, place: &Place) -> bool {
        let Place::Field { base, field } = place else {
            return false;
        };
        let Ok(base_ty) = self.local_decl(*base).map(|decl| decl.ty) else {
            return false;
        };
        self.is_reference_class_type(base_ty) && self.class_has_named_field(base_ty, *field)
    }

    /// Returns whether a type is a reference class (handle newtype).
    ///
    /// Reference classes carry shared mutable identity through
    /// `Rc<RefCell<Inner>>`; field access goes through the cell and methods take
    /// `&self` uniformly. Non-class types and value classes answer `false`.
    pub(super) fn is_reference_class_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. }) if self.context.is_reference_class(*name)
        )
    }

    /// Returns whether `local` is the receiver of a reference-class method that
    /// is captured into an escaping closure.
    ///
    /// A reference-class `self` is already a shareable handle (`Rc<RefCell<
    /// Inner>>`), so an escaping closure captures a plain `self.clone()` rather
    /// than wrapping the receiver in a second `Rc<RefCell<_>>`. When this holds,
    /// the receiver is bound once as `let smelt_capture_self = self.clone();` and
    /// every reference renders as that handle (`smelt_capture_self`), with field
    /// access going through its cell (`smelt_capture_self.0.borrow()`). This is
    /// the escaping-`this` mechanism that clears the E0425 `smelt_capture_self`
    /// cluster.
    pub(super) fn is_reference_self_shared_capture(&self, local: LocalId) -> bool {
        self.method_owner_is_reference_class()
            && matches!(self.function.origin, HirOrigin::ClassMethod { .. })
            && self.function.params.first() == Some(&local)
            && self.local_uses_shared_capture_storage(local)
    }

    /// Returns whether the method currently being emitted belongs to a reference
    /// class, so its receiver is `&self` and its field access goes through the
    /// shared cell.
    pub(super) fn method_owner_is_reference_class(&self) -> bool {
        match self.function.origin {
            HirOrigin::ClassConstructor { class, .. }
            | HirOrigin::ClassMethod { class, .. }
            | HirOrigin::ClassStaticMethod { class, .. } => self.context.is_reference_class(class),
            HirOrigin::Body(_) => false,
        }
    }

    /// Returns whether a class symbol names the stdlib `RegExp` class.
    ///
    /// Several emitters dispatch RegExp-shaped class types to the regex
    /// runtime shim; the identity lookup lives in the shared `smelt-stdlib`
    /// registry instead of inline name comparisons.
    pub(super) fn is_regexp_class_symbol(&self, name: Symbol) -> Result<bool, EmitError> {
        Ok(
            smelt_stdlib::typescript_stdlib_class(self.symbol_name(name)?)
                == Some(smelt_stdlib::StdlibClass::RegExp),
        )
    }

    /// Returns the synthetic match-result class identity named by a symbol, if any.
    ///
    /// Both `__SmeltMatch` (the match value) and `__SmeltMatchGroups` (its
    /// named-group accessor) are backed by the generated concrete `SmeltMatch`
    /// Rust type. The two share a Rust representation but differ in how field and
    /// index reads are lowered, so callers inspect the returned identity to pick
    /// the right accessor.
    pub(super) fn match_class_kind(
        &self,
        name: Symbol,
    ) -> Result<Option<smelt_stdlib::StdlibClass>, EmitError> {
        Ok(
            match smelt_stdlib::typescript_stdlib_class(self.symbol_name(name)?) {
                class @ Some(
                    smelt_stdlib::StdlibClass::Match | smelt_stdlib::StdlibClass::MatchGroups,
                ) => class,
                _ => None,
            },
        )
    }

    /// Returns whether a class symbol names a synthetic RegExp match-result class.
    pub(super) fn is_match_class_symbol(&self, name: Symbol) -> Result<bool, EmitError> {
        Ok(self.match_class_kind(name)?.is_some())
    }

    /// Returns whether a class-shaped type is emitted as `SmeltUnknown`.
    pub(super) fn is_erased_class_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, .. }) => {
                // RegExp and the synthetic match-result classes have dedicated
                // Rust runtime types (`SmeltRegExp` / `SmeltMatch`). Other stdlib
                // classes may still be represented by primitive or collection
                // values and should keep the ordinary erased-class fallback.
                if self.symbol_name(*name).is_ok_and(|type_name| {
                    matches!(
                        smelt_stdlib::typescript_stdlib_class(type_name),
                        Some(
                            smelt_stdlib::StdlibClass::RegExp
                                | smelt_stdlib::StdlibClass::Match
                                | smelt_stdlib::StdlibClass::MatchGroups
                        )
                    )
                }) {
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

    /// Returns whether a target type can absorb an erased `SmeltUnknown::Function`.
    ///
    /// A function argument flowing into one of these targets should be erased
    /// via `value_at_type` (which yields `Some(SmeltUnknown::Function(..))` for
    /// optionals) rather than dropped to a default. Covers the dynamic surfaces
    /// (`Unknown`, type parameters, unions, erased classes), `Function` itself,
    /// and a single `Optional` layer wrapping any of those — e.g. purry
    /// data-last params typed `arg?: Data | Callback`, where the predicate must
    /// survive as a value so the runtime dispatcher can branch on `typeof`.
    pub(super) fn type_accepts_erased_function(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                true
            }
            Some(Type::Optional(inner)) => self.type_accepts_erased_function(*inner),
            _ => self.is_erased_class_type(ty),
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
            Some(Type::Class { name, .. }) if smelt_stdlib::typescript_stdlib_class(self.symbol_name(*name)?)
                == Some(smelt_stdlib::StdlibClass::MatchFnResult)
        ))
    }

    /// Return whether a type is the synthetic `MatchFnResult` class without inspecting generics.
    pub(super) fn is_match_fn_result_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        Ok(matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. }) if smelt_stdlib::typescript_stdlib_class(self.symbol_name(*name)?)
                == Some(smelt_stdlib::StdlibClass::MatchFnResult)
        ))
    }

    /// Return the generic payload type carried by a `MatchFnResult<T>` value.
    ///
    /// Older generated code erased this field through `SmeltUnknown`, but
    /// date-fns parser matches instantiate it with concrete payloads such as
    /// `String`. Preserving the payload type avoids emitting unknown-pattern
    /// casts against concrete Rust values.
    pub(super) fn match_fn_result_value_type(
        &self,
        ty: TypeId,
    ) -> Result<Option<TypeId>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(ty) else {
            return Ok(None);
        };
        if smelt_stdlib::typescript_stdlib_class(self.symbol_name(*name)?)
            != Some(smelt_stdlib::StdlibClass::MatchFnResult)
        {
            return Ok(None);
        }
        Ok(args.first().copied())
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
pub(super) fn operand_uses_local(operand: &Operand, local: LocalId) -> bool {
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
pub(super) fn assignment_place_reads_local(place: &Place, local: LocalId) -> bool {
    match place {
        Place::Local(_) => false,
        Place::Field { base, .. } => *base == local,
        Place::Index { base, index } => *base == local || operand_uses_local(index, local),
    }
}

/// Return whether an rvalue may read a local.
pub(super) fn rvalue_uses_local(value: &Rvalue, local: LocalId) -> bool {
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
        | Rvalue::NumericHypot { args: items }
        | Rvalue::AsyncOp { args: items, .. } => items
            .iter()
            .any(|operand| operand_uses_local(operand, local)),
        Rvalue::NumericExtrema { args, spread, .. } => {
            args.iter().any(|operand| operand_uses_local(operand, local))
                || spread
                    .as_ref()
                    .is_some_and(|operand| operand_uses_local(operand, local))
        }
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
            ..
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
        | Rvalue::TypeofValue { value: operand }
        | Rvalue::PrototypeSentinel { value: operand }
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
        Rvalue::FunctionTableLookup { key, cases } => {
            operand_uses_local(key, local)
                || cases
                    .iter()
                    .any(|(_, case)| operand_uses_local(case, local))
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
        Rvalue::Struct { fields, .. } => fields
            .iter()
            .any(|(_, field_value)| operand_uses_local(field_value, local)),
        Rvalue::CallableObjectAssign { callable, props } => {
            operand_uses_local(callable, local)
                || props
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
        // A host-global override write reads its stored value operand; the read
        // and presence probes take no operands. Missing this arm would let the
        // `_ => false` fallthrough elide a closure whose only use is the write.
        Rvalue::HostGlobalWrite { value: stored, .. } => operand_uses_local(stored, local),
        _ => false,
    }
}

/// Return whether a terminator may read a local.
pub(super) fn terminator_uses_local(terminator: &Terminator, local: LocalId) -> bool {
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

/// Tokens that mark a trial-rendered generic free-function body as needing the
/// erased runtime, so real generics would produce type errors.
///
/// These are the erased carrier types and the JavaScript-semantics methods /
/// conversions that are only implemented for `SmeltUnknown` (and primitives),
/// not for an arbitrary generic `T`. A body that mentions any of them inspects,
/// compares, hashes, keys, or erases a `T`-typed value — operations the generic
/// bounds (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown`) do not
/// support on a bare `T` (e.g. `js_strict_eq`, `same_js_key`) or that force a
/// `T`-vs-`SmeltUnknown` mismatch.
const ERASED_CARRIER_TOKENS: &[&str] = &[
    "SmeltUnknown",
    "SmeltObject",
    "SmeltArray",
    "SmeltRecord",
    "SmeltJsMap",
    "js_strict_eq",
    "same_js_key",
    "smelt_from_unknown",
    "into_smelt_unknown",
];

/// Return whether a trial-rendered generic free-function body still needs the
/// erased runtime carrier.
///
/// A generic free function only emits real Rust generics when its body keeps
/// every type parameter opaque. If the body — rendered with the type parameters
/// in scope — references any erased carrier type or JavaScript-semantics helper
/// (see [`ERASED_CARRIER_TOKENS`]), the body inspects, compares, or erases a
/// `T`-typed value (for example a null/undefined check, a deep-equality
/// comparison, or a map-keying operation). Emitting real generics for such a
/// body produces `T`-vs-`SmeltUnknown` mismatches or missing-trait errors, so
/// the caller falls back to the fully erased signature.
///
/// This is a deliberately conservative textual check: any listed token in the
/// trial body disqualifies generic emission. A pure passthrough body
/// (`return x;`, `return xs.get(..)..;`) contains none of them and keeps its
/// generics.
fn body_needs_erased_carrier(body: &str) -> bool {
    let stripped = strip_mut_list_adapter_blocks(body);
    ERASED_CARRIER_TOKENS
        .iter()
        .any(|token| stripped.contains(token))
}

/// Removes convert-in-place mutable-list adapter blocks from a trial body.
///
/// The adapter (see `call::static_call_mut_list_adapter_text`) deliberately
/// erases a generic element to `SmeltUnknown` and un-erases it again at a real
/// dynamic boundary: a generic caller forwarding a `&mut` list into an erased
/// callee. That controlled boundary conversion is not a `T` leak, so its erased
/// carrier tokens must not disqualify the caller from emitting real generics.
/// Each adapter block is delimited by its leading `smelt_mut_arg_` binding; this
/// removes the enclosing brace-balanced block so the cleanliness check sees only
/// the surrounding (opaque) body.
fn strip_mut_list_adapter_blocks(body: &str) -> String {
    const MARKER: &str = "let mut smelt_mut_arg_";
    let mut result = body.to_owned();
    while let Some(marker_pos) = result.find(MARKER) {
        // Find the `{` that opens the adapter block (the nearest one before the
        // marker binding).
        let Some(open) = result[..marker_pos].rfind('{') else {
            break;
        };
        // Brace-match forward from `open` to find the block's closing `}`.
        let bytes = result.as_bytes();
        let mut depth = 0_usize;
        let mut close_pos = None;
        for (offset, &byte) in bytes.iter().enumerate().skip(open) {
            match byte {
                b'{' => depth = depth.saturating_add(1),
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close_pos = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_pos) = close_pos else {
            break;
        };
        result.replace_range(open..=close_pos, "");
    }
    result
}

// Constant formatting continues in `literals.rs`.
