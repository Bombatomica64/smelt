//! The function emitter itself: how one is built for a function, closure or
//! generated item, and the MIR, local and symbol lookups every shard shares.

use crate::rust::RustType;
use crate::rust::RustExpr;
use super::*;
use crate::emitter::local_binding::{CellUpgrade, LocalBinding, SharedCell};
use crate::emitter::local_declaration::predeclared_locals_for_function;
use crate::emitter::typeof_str_locals;
use std::borrow::Cow;

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
        let bindings = Self::local_bindings(mir, function)?;
        let declared_locals = function.params.iter().copied().collect();
        let folded_throw_payloads = throw::folded_throw_payload_locals(mir, function);
        let predeclared_locals = predeclared_locals_for_function(mir, function)
            .difference(&folded_throw_payloads)
            .copied()
            .collect();
        Ok(Self {
            bindings,
            mutable_locals: assigned_locals(mir, context, function),
            declared_locals: RefCell::new(declared_locals),
            predeclared_locals,
            static_typeof_str_locals: typeof_str_locals::static_typeof_str_locals(mir, function),
            folded_throw_payloads,
            ..Self::bare(mir, context, function, none_ty, unknown_ty)
        })
    }

    /// The one place every [`FunctionEmitter`] field is initialized.
    ///
    /// Answers an emitter with no per-function analysis (no local names, no
    /// declared or mutable locals) in the [`emit_role::EmitRole::Own`] role.
    /// [`FunctionEmitter::new`] fills the analysis in on top of it; the
    /// synthetic type-rendering emitters ([`Self::type_renderer`]) use it as is.
    /// `unknown_ty` types the synthetic local answered for a missing local.
    fn bare(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
        none_ty: TypeId,
        unknown_ty: TypeId,
    ) -> Self {
        Self {
            mir,
            context,
            function,
            bindings: HashMap::new(),
            binding_uses: local_binding::SharedBindingUses::default(),
            mutable_locals: HashSet::new(),
            declared_locals: RefCell::new(HashSet::new()),
            predeclared_locals: HashSet::new(),
            deferred_capture_cells: RefCell::new(HashSet::new()),
            static_typeof_str_locals: HashSet::new(),
            folded_throw_payloads: HashSet::new(),
            borrowed_callback_names: HashSet::new(),
            by_reference_param_locals: HashSet::new(),
            caches: render_state::AnalysisCaches::default(),
            conversions: render_state::ConversionGuards::default(),
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
            role: emit_role::EmitRole::Own {
                generics: emit_role::OwnGenerics::Declared,
            },
            method_name: None,
            hoisted_module_item: std::cell::Cell::new(false),
            inherited_bound_suspended: std::cell::Cell::new(false),
            flow: flow_state::ControlFlowState::default(),
        }
    }

    /// A synthetic emitter used only to render types (and default values) at
    /// crate level, outside any real function.
    ///
    /// Its `function` is `mir.functions.first()` and both its `none_ty` and its
    /// missing-local type are `ty`, exactly as the three hand-written literals
    /// this replaced spelled them; see the TODO(plan-197) note in
    /// [`Self::type_text_for_with_context`] for why the foreign function is
    /// kept.
    pub(crate) fn type_renderer(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        ty: TypeId,
    ) -> Result<Self, EmitError> {
        let function = mir
            .functions
            .first()
            .ok_or_else(|| EmitError::new("MIR has no functions"))?;
        Ok(Self::bare(mir, context, function, ty, ty))
    }

    /// An emitter for a closure body rendered inline inside this emitter.
    ///
    /// `function` is the closure's synthetic [`MirFunction`] and `captures`
    /// pairs each enclosing local with the closure local receiving it. The child
    /// inherits, as one [`emit_role::ClosureScope`]: the type parameters in
    /// scope here (already gated by this emitter's erasure decision and
    /// hoisting state), the free function on generic trial, the inherited-copy
    /// substitution meant for closures, and the base-initializer receiver
    /// mapped through `captures`. Nothing else of this emitter's role carries
    /// over: the closure's own function declares no type parameters, so an own
    /// generic decision or a hoisting flag could only gate an empty set.
    pub(crate) fn child_for_closure<'c>(
        &'c self,
        function: &'c MirFunction,
        captures: &[smelt_mir::MirClosureCapture],
    ) -> Result<FunctionEmitter<'c>, EmitError>
    where
        'mir: 'c,
    {
        let base_initializer = self.role.base_initializer().map(|parent| {
            emit_role::BaseInitializerReceiver {
                owner: parent.owner,
                receivers: captures
                    .iter()
                    .filter(|capture| parent.receivers.contains(&capture.source_local))
                    .filter_map(|capture| capture.target_local)
                    .collect(),
            }
        });
        let scope = emit_role::ClosureScope {
            enclosing_type_params: self.current_function_type_params(),
            trial_function: self.trial_generic_function(),
            inherited_type_args: self.role.closure_inherited_type_args().clone(),
            base_initializer,
        };
        let mut child = FunctionEmitter::new(self.mir, self.context, function)?;
        child.role = emit_role::EmitRole::Closure(scope);
        child.binding_uses = std::rc::Rc::clone(&self.binding_uses);
        Ok(child)
    }

    /// An emitter for a generated module-level item that declares exactly
    /// `type_params` (a generic union enum `SmeltUnionN<T, ..>` and its impl
    /// blocks), rendering in that item's scope rather than `function`'s.
    ///
    /// `function` only supplies the emitter's MIR context; none of its own
    /// generics, locals or erasure decisions apply to the item. Every render
    /// inside the item (member types, `IntoSmeltUnknown` and
    /// `from_smelt_unknown` bodies) spells exactly `type_params`, which the
    /// item's `impl<..>` header declares, and erases any other parameter.
    /// The scope is carried by the [`emit_role::EmitRole::Closure`] role,
    /// whose `enclosing_type_params` is precisely "the parameters the
    /// enclosing Rust output declares".
    pub(crate) fn for_generated_item(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
        type_params: HashSet<Symbol>,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::new(mir, context, function)?;
        emitter.role = emit_role::EmitRole::Closure(emit_role::ClosureScope {
            enclosing_type_params: type_params,
            trial_function: None,
            inherited_type_args: HashMap::new(),
            base_initializer: None,
        });
        Ok(emitter)
    }

    /// Builds a stable Rust binding for every MIR local in a function.
    fn local_bindings(
        mir: &'mir Mir,
        function: &'mir MirFunction,
    ) -> Result<HashMap<LocalId, LocalBinding>, EmitError> {
        let mut bindings = HashMap::new();
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
                        let binding = if function.is_async || function.is_generator {
                            LocalBinding::SelfOwned
                        } else {
                            LocalBinding::Receiver
                        };
                        used.insert(binding.text().into_owned());
                        bindings.insert(local_id, binding);
                        continue;
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
            bindings.insert(local_id, LocalBinding::Plain(name));
        }

        Ok(bindings)
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
    pub(super) fn local_value(&self, local: LocalId) -> Result<RustExpr, EmitError> {
        match self.local_shared_cell(local)? {
            Some(cell) => Ok(cell.read_expr()),
            None => self.local_expr(local),
        }
    }

    /// The shared cell a local is read and written through, when it has one.
    ///
    /// A capture of an enclosing shared cell carries the cell in its binding; a
    /// local of this frame that lives in shared storage reaches its own cell once
    /// that cell is declared.
    pub(super) fn local_shared_cell(&self, local: LocalId) -> Result<Option<SharedCell>, EmitError> {
        if let Some(cell) = self.local_binding(local)?.shared_cell() {
            return Ok(Some(cell.clone()));
        }
        if self.local_uses_shared_capture_storage(local) && self.is_local_declared(local) {
            return Ok(Some(SharedCell::named(&self.local_name(local)?, CellUpgrade::Strong)));
        }
        Ok(None)
    }

    /// Renders a mutable receiver or assignment through shared closure storage.
    ///
    /// Shared captures live in `Rc<RefCell<T>>`, so a write/receiver expands to
    /// `(*smelt_capture_x.borrow_mut())`. The returned `RefMut` guard lives to
    /// the end of the FULL enclosing statement; if that statement also invoked
    /// a closure re-borrowing the same cell, the nested borrow would panic with
    /// "already borrowed". The same three-address invariant documented on
    /// [`Self::local_value`] prevents this: every call is a separate SSA
    /// temp statement, so an assignment target's `borrow_mut` guard only ever
    /// coexists with already-evaluated value operands (e.g.
    /// `(*smelt_capture_count.borrow_mut()) = _smelt_tmp_N;`), never with a live
    /// call. Callers must not construct a single statement that both takes this
    /// mutable borrow and evaluates a call.
    pub(super) fn local_mut_value(&self, local: LocalId) -> Result<RustExpr, EmitError> {
        match self.local_shared_cell(local)? {
            Some(cell) => {
                self.binding_uses.record_mut_borrow(&cell);
                Ok(cell.write_expr())
            }
            None => self.local_expr(local),
        }
    }

    /// Resolve a MIR function by stable function id.
    pub(super) fn function_by_id(&self, function_id: FuncId) -> Option<&MirFunction> {
        id_index(function_id.0, "function index does not fit usize")
            .ok()
            .and_then(|index| self.mir.functions.get(index))
    }

    /// The Rust type text of a local's shared capture cell contents.
    ///
    /// Every capture-cell declaration spells the local's type; for the receiver
    /// of a base initializer copy that is `Self` (see
    /// [`Self::is_base_initializer_receiver`]).
    pub(super) fn capture_cell_type(&self, local: LocalId, ty: TypeId) -> Result<RustType, EmitError> {
        if self.is_base_initializer_receiver(local) {
            return Ok(RustType::raw("Self"));
        }
        self.rendered_type(ty)
    }

    /// Gets the entry block of the function.
    pub(super) fn entry_block(&self) -> Result<&BasicBlock, EmitError> {
        self.block(self.function.entry)
    }

    /// Gets a basic block by ID.
    pub(super) fn block(&self, block: smelt_mir::BlockId) -> Result<&BasicBlock, EmitError> {
        self.function
            .blocks
            .get(id_index(block.0, "block index does not fit usize")?)
            .ok_or_else(|| EmitError::new("terminator references an unknown block"))
    }

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

    /// Open a nested Rust local-declaration scope.
    ///
    /// Generated Rust has lexical scopes for branches, loops, and closures, so
    /// a local declared inside one must not count as declared in a later
    /// sibling or outer scope. The returned guard remembers the declared set as
    /// it is now: [`DeclaredLocalsScope::reset`] restores it between sibling
    /// branches, [`DeclaredLocalsScope::close`] at the scope's end, and dropping
    /// the guard restores it too, so an early `?` (whose error a caller may
    /// catch and fall back from) cannot leak the branch's declarations.
    pub(super) fn declared_scope(&self) -> DeclaredLocalsScope<'_> {
        DeclaredLocalsScope {
            declared: &self.declared_locals,
            snapshot: Some(self.declared_locals.borrow().clone()),
        }
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

    /// How a local is bound in the generated Rust scope.
    pub(super) fn local_binding(&self, local: LocalId) -> Result<&LocalBinding, EmitError> {
        self.bindings
            .get(&local)
            .ok_or_else(|| EmitError::new(format!("local {} has no Rust binding", local.0)))
    }

    /// The Rust text of a local's binding, for output that names it.
    ///
    /// Rendering is what counts as a use (see [`BindingUses`]); a question about
    /// the binding asks [`Self::local_binding`] instead.
    ///
    /// [`BindingUses`]: local_binding::BindingUses
    pub(super) fn local_expr(&self, local: LocalId) -> Result<RustExpr, EmitError> {
        let binding = self.local_binding(local)?;
        self.binding_uses.record(binding);
        Ok(binding.expr())
    }

    /// The Rust name a local is bound to, recording the use.
    pub(super) fn local_name(&self, local: LocalId) -> Result<Cow<'_, str>, EmitError> {
        let binding = self.local_binding(local)?;
        self.binding_uses.record(binding);
        Ok(binding.text())
    }

    /// How the source language of the body being emitted spells an absent value.
    ///
    /// Decided during MIR lowering from the body's own file (`MirFunction::
    /// absent`), because a crate can mix TypeScript and Python modules and the
    /// two disagree: stringifying an `Optional` that holds nothing is
    /// `undefined` in JavaScript and `None` in Python. Every site that renders
    /// an absent or null value as text asks here rather than hard-coding a
    /// word.
    pub(super) fn absent_spelling(&self) -> AbsentSpelling {
        self.function.absent()
    }

    /// Gets the Rust-facing rendering of a symbol.
    ///
    /// A source name that is not a valid Rust spelling is case-folded when it is
    /// interned (`camelCase` -> `camel_case`), so this is the *generated* name,
    /// not the one the source wrote. Use [`Self::symbol_source_name`] whenever
    /// the answer is compared against a JavaScript key.
    pub(super) fn symbol_name(&self, symbol: Symbol) -> Result<&str, EmitError> {
        self.mir
            .symbols
            .get(symbol)
            .ok_or_else(|| EmitError::new("MIR references an unknown symbol"))
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
        Terminator::Return(_) | Terminator::Throw(_) => Vec::new(),
    }
}

/// A nested Rust local-declaration scope opened by
/// [`FunctionEmitter::declared_scope`].
///
/// Every success path ends it explicitly: [`Self::close`] restores the set it
/// captured, [`Self::keep`] leaves the current declarations in place (the
/// region continued in the same Rust scope, e.g. a `match` whose continuation
/// is emitted after it with no join). Dropping it without either — an early
/// `?` — restores the captured set, so an error a caller catches and falls
/// back from cannot leak the abandoned region's declarations.
pub(super) struct DeclaredLocalsScope<'e> {
    /// The emitter's declared-locals set.
    declared: &'e RefCell<HashSet<LocalId>>,
    /// The set as it was when the scope opened; `None` once kept.
    snapshot: Option<HashSet<LocalId>>,
}

impl DeclaredLocalsScope<'_> {
    /// Restore the captured set and keep the scope open (between siblings).
    pub(super) fn reset(&self) {
        if let Some(snapshot) = &self.snapshot {
            self.declared.borrow_mut().clone_from(snapshot);
        }
    }

    /// Restore the captured set and end the scope.
    pub(super) fn close(mut self) {
        if let Some(snapshot) = self.snapshot.take() {
            *self.declared.borrow_mut() = snapshot;
        }
    }

    /// End the scope keeping the declarations made inside it.
    pub(super) fn keep(mut self) {
        self.snapshot = None;
    }
}

impl Drop for DeclaredLocalsScope<'_> {
    /// Restore the captured set when the scope was abandoned by an early exit.
    fn drop(&mut self) {
        if let Some(snapshot) = self.snapshot.take() {
            *self.declared.borrow_mut() = snapshot;
        }
    }
}
