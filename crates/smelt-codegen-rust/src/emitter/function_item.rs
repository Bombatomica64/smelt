//! Function and method definitions: the signature, the parameter and local
//! preludes, the body, and the fallthrough return of one emitted Rust item.

use super::*;
use crate::emitter::local_binding::SELF_OWNED_NAME;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// The closing text of an async `main`'s runtime scope.
    const ASYNC_MAIN_RUNTIME_EPILOGUE: &'static str = "})\n";

    /// The runtime an async `main` runs its body on.
    ///
    /// # Why not `#[tokio::main]`
    ///
    /// That attribute builds a MULTI-THREADED, work-stealing runtime, and
    /// everything Smelt generates is `Rc`-based: a closure's captured state, a
    /// modeled object's shared cell, a promise's result cell. None of it is
    /// `Send`, so nothing generated can be spawned onto such a runtime at all —
    /// a `node:http` request handler least of all, since it captures whatever
    /// the surrounding program had.
    ///
    /// A single-threaded loop is also what the source language actually has. A
    /// TypeScript program ported to Rust that silently gained parallel handler
    /// execution would be a different program: two requests could observe each
    /// other's half-written state through exactly the shared cells that model
    /// JavaScript's mutable objects. So the current-thread runtime is the
    /// faithful shape, not a workaround for a missing bound.
    ///
    /// The `LocalSet` is the other half: it is what makes `spawn_local`
    /// available, and `spawn_local` is how a listening server keeps accepting
    /// while the program's own body carries on. Both are emitted for EVERY
    /// async `main` rather than only for programs that serve — two runtime
    /// shapes for one language is the special case this codebase refuses.
    fn async_main_runtime_prologue(can_throw: bool) -> String {
        // A runtime that cannot be built is not a program error the source can
        // handle, but a throwing `main` can still report it in the ordinary
        // channel rather than panicking.
        let build = if can_throw {
            ".build()?"
        } else {
            ".build().expect(\"tokio runtime\")"
        };
        format!(
            "let smelt_runtime = tokio::runtime::Builder::new_current_thread().enable_all(){build};\nlet smelt_local = tokio::task::LocalSet::new();\nsmelt_local.block_on(&smelt_runtime, async move {{\n"
        )
    }

    /// Emits a free function definition, naming the site of any blocker.
    ///
    /// Every emitter blocker raised anywhere inside this function's emission
    /// gets the function's name and source span attached here, on the way out.
    /// That is one place instead of the couple of hundred `EmitError::new` call
    /// sites, and it covers the ones that have not been written yet — a blocker
    /// that reports only a shape (`list unshift item must match the list
    /// element type`) is unfindable in a corpus the size of Hono, which cost
    /// two rounds of bisecting a manifest's `exclude` list.
    pub(crate) fn emit(
        &mut self,
        out: &mut String,
        visibility: FreeFunctionVisibility,
    ) -> Result<(), EmitError> {
        self.apply_crate_generic_decision();
        let emitted = self.emit_free_function(out, visibility);
        emitted.map_err(|error| error.with_site(|| self.current_function_site()))
    }

    /// Emits a free function definition.
    fn emit_free_function(
        &mut self,
        out: &mut String,
        visibility: FreeFunctionVisibility,
    ) -> Result<(), EmitError> {
        let name = self.symbol_name(self.function.name)?;
        // A `#[tokio::test]` is found by the harness and never re-exported.
        let visibility = if self.function.is_test && self.function.is_async {
            ""
        } else {
            visibility.text()
        };
        if self.function.is_test {
            if self.function.is_async {
                out.push_str("#[tokio::test]\n");
            } else {
                out.push_str("#[test]\n");
            }
        }
        if !self.function.is_test && name == "main" && self.function.return_ty == self.none_ty {
            // An async module body ends by running the event loop until the
            // program may exit, and that drain is lowered as the body's last
            // statement rather than wrapped around it here -- see
            // `module_init`'s `append_module_exit_drain`. So this emission
            // supplies only the runtime the body runs on.
            // The signature depends only on whether the body can throw, and
            // the runtime scope only on whether it is async: an async `main`
            // builds its own runtime inside the body, so the two questions are
            // answered separately.
            if self.function.can_throw {
                out.push_str(&format!(
                    "{visibility}fn main() -> Result<(), {ERROR_BOX}> {{\n"
                ));
            } else {
                out.push_str(&format!("{visibility}fn main() {{\n"));
            }
            if self.function.is_async {
                out.push_str(&Self::async_main_runtime_prologue(self.function.can_throw));
            }
            // The entry's imports run first, each once, in start-up order
            // (`crate::startup`).
            out.push_str(&crate::startup::startup_calls(
                self.mir,
                self.context,
                Some(self.function.id),
                self.function.can_throw,
            )?);
            self.emit_body(out)?;
            if self.function.is_async {
                out.push_str(Self::ASYNC_MAIN_RUNTIME_EPILOGUE);
            }
            out.push_str("}\n");
            return Ok(());
        }

        // Whether this free function emits real Rust generics was decided once,
        // crate-wide, by `EmitContext::populate_generic_functions` (signature
        // safety + a body-cleanliness trial), and applied to this emitter's role
        // by `emit` (`emit_role::crate_generic_decision`). Erasing the type
        // parameters when the function is NOT in that set makes the signature,
        // body, and every call site agree on the erased shape.
        let mut body = String::new();
        self.emit_body(&mut body)?;

        let fn_params = self.param_decls_text(0)?;
        // A generic free function emits real Rust generics
        // (`fn identity<T: ..>(x: T) -> T`); the suffix is empty otherwise,
        // including when the body-cleanliness trial forced a fall back to
        // erasure (`OwnGenerics::Erased`). In that case the parameters and body
        // are already rendered as `SmeltUnknown`, so declaring `<T>` would leave
        // an unconstrained, uninferable type parameter on the signature.
        // Source type parameters first, then the generated
        // `F{n}: Fn(..) + ?Sized` callback bounds, in declaration order.
        let generics = if self.role.own_generics_erased() {
            String::new()
        } else {
            let mut parts = crate::generic_lifting::function_impl_generics_list(
                self.mir,
                self.function,
                &self.context.owned_callback_params,
                &self.context.bound_traits,
                &self.context.erased_arguments,
            )?;
            parts.extend(self.callback_generic_bounds_text()?);
            if parts.is_empty() {
                String::new()
            } else {
                format!("<{}>", parts.join(", "))
            }
        };
        out.push_str(&format!(
            "{visibility}{}fn {}{generics}({fn_params}) -> {} {{\n",
            self.async_keyword(),
            self.function_rust_name(self.function)?,
            self.return_type(self.function.return_ty)?
        ));
        if self.function.is_generator {
            self.emit_generator_wrapper(&body, out);
        } else {
            out.push_str(&body);
        }
        out.push_str("}\n");
        Ok(())
    }

    /// Emit a free function's body (parameter preludes, block, and fallthrough
    /// return) without the signature or closing brace.
    ///
    /// Split out from [`FunctionEmitter::emit`] so a generic free function can
    /// trial-render its body to decide whether real generics are safe (the body
    /// must keep each type parameter opaque).
    pub(super) fn emit_body(&self, out: &mut String) -> Result<(), EmitError> {
        // A generator takes its receiver when CALLED, in its wrapper, not on
        // its first resume (`emit_generator_wrapper`).
        if !self.function.is_generator
            && let Some(prologue) = self.this_frame_prologue()
        {
            out.push_str(&format!("    {prologue}\n"));
        }
        self.emit_shared_parameter_preludes(out)?;
        if self.function.is_test && self.context.test_runtime_resets.date_now {
            out.push_str("    SMELT_DATE_NOW.with(|value| value.set(None));\n");
        }
        if self.function.is_test && self.context.test_runtime_resets.timers {
            out.push_str(&format!(
                "    {reset_timers}();\n",
                reset_timers = smelt_stdlib::runtime_symbols::timers::RESET_TIMERS,
            ));
        }
        self.emit_mutable_local_preludes(out)?;
        let tail = self.emit_region_flow(self.entry_block()?, out)?;
        // `block_eventually_terminates` walks the MIR CFG, which can keep a
        // phantom fall-through edge that the structured emitter never renders
        // (e.g. a `match` whose every arm returns). The recorded `Flow`
        // reports whether the *rendered* tail already diverges, so consulting
        // it as well suppresses a trailing `return` that would otherwise be
        // dead `unreachable_code`.
        if !self.block_eventually_terminates(self.function.entry, &mut BlockIdSet::default())?
            && !tail.diverges()
        {
            self.emit_fallthrough_return(out)?;
        }
        Ok(())
    }

    /// Reset the per-emission scratch state so a function body can be rendered
    /// again (the generic body-cleanliness trial renders the body once to decide
    /// on generics, then the real emit renders it again).
    ///
    /// This is ALL the state a body render mutates; everything else on the
    /// emitter is either fixed at construction (names, the local analyses, the
    /// role) or a cache of a pure function of the MIR (termination, post
    /// dominators, loop exits, the local write index) that stays valid.
    ///
    /// - the declared-locals set starts over with only the parameters
    ///   pre-declared, matching [`FunctionEmitter::new`], or the re-rendered
    ///   body would omit declarations;
    /// - the deferred capture cells are re-recorded by the parameter preludes;
    /// - the structural expansion stacks and the scope flags are empty/off
    ///   between renders (their guards restore them; this also covers a render
    ///   abandoned by an error);
    /// - the control-flow state ([`flow_state::ControlFlowState`]) starts over.
    pub(super) fn reset_body_emission_state(&self) {
        {
            let mut declared = self.declared_locals.borrow_mut();
            declared.clear();
            declared.extend(self.function.params.iter().copied());
        }
        self.deferred_capture_cells.borrow_mut().clear();
        self.conversions
            .record_conversion_stack
            .borrow_mut()
            .clear();
        self.conversions.type_expansion_stack.borrow_mut().clear();
        self.hoisted_module_item.set(false);
        self.inherited_bound_suspended.set(false);
        self.flow.reset();
    }

    /// Wraps a generator's rendered `body` in the `genawaiter` producer and the
    /// `SmeltGenerator` or `SmeltAsyncGenerator` handle a call returns.
    fn emit_generator_wrapper(&self, body: &str, out: &mut String) {
        // The receiver is taken at the call, then moved into the producer.
        if let Some(prologue) = self.this_frame_prologue() {
            out.push_str(&format!("    {prologue}\n"));
        }
        out.push_str("    let smelt_generator_input = ::std::rc::Rc::new(::std::cell::RefCell::new(None));\n");
        out.push_str("    let smelt_generator_producer_input = smelt_generator_input.clone();\n");
        out.push_str("    let smelt_generator = genawaiter::rc::Gen::new(move |smelt_co| { let smelt_generator_input = smelt_generator_producer_input; async move {\n");
        out.push_str(body);
        out.push_str("    } });\n");
        if self.function.is_async {
            out.push_str("    let smelt_generator = ::std::rc::Rc::new(::std::cell::RefCell::new(smelt_generator));\n");
            out.push_str(&format!("    SmeltAsyncGenerator::new(move |smelt_value| {{ *smelt_generator_input.borrow_mut() = Some(smelt_value); let smelt_generator = smelt_generator.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_state = {{ let mut smelt_generator = smelt_generator.borrow_mut(); smelt_generator.async_resume().await }}; Ok::<_, {ERROR_BOX}>(match smelt_state {{ genawaiter::GeneratorState::Yielded(smelt_value) => SmeltGeneratorResult::Yielded(smelt_value), genawaiter::GeneratorState::Complete(smelt_value) => SmeltGeneratorResult::Complete(smelt_value?) }}) }})) }})\n"));
        } else {
            out.push_str("    let mut smelt_generator = smelt_generator;\n");
            let resume = FunctionEmitter::sync_generator_resume_closure(self.function.can_throw);
            out.push_str(&format!("    SmeltGenerator::new({resume})\n"));
        }
    }

    /// The declarations of this function's parameters after the first `skip`,
    /// comma separated.
    fn param_decls_text(&self, skip: usize) -> Result<String, EmitError> {
        Ok(self
            .function
            .params
            .iter()
            .skip(skip)
            .map(|param| {
                let mutability = if self.local_binding_needs_mut(*param) {
                    "mut "
                } else {
                    ""
                };
                Ok(format!(
                    "{mutability}{}: {}",
                    self.local_name(*param)?,
                    self.parameter_decl_type(*param)?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", "))
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
        self.flow.record(flow_state::Flow::Diverges);
        let return_ty = self.body_return_ty();
        if self.function.can_throw {
            self.binding_uses.record_early_return();
            out.push_str(&format!(
                "    return Ok({});\n",
                self.default_value(return_ty)?.delimited()
            ));
        } else if return_ty == self.none_ty {
            return Ok(());
        } else {
            out.push_str(&format!(
                "    return {};\n",
                self.default_value(return_ty)?.delimited()
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
        let mut slots = self
            .receiver_snapshot_slots()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        slots.sort_by_key(|slot| slot.0);
        for slot in slots {
            out.push_str(&format!(
                "    let mut {} = smelt_no_receiver();\n",
                self.receiver_snapshot_name(slot)?
            ));
        }
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
            if self.local_is_maybe_unbound(local)? {
                self.emit_binding_declaration(local, out)?;
                continue;
            }
            let decl = self.local_decl(local)?;
            if matches!(self.mir.types.get(decl.ty), Some(Type::Future(_))) {
                continue;
            }
            if matches!(self.mir.types.get(decl.ty), Some(Type::Class { .. }))
                && !self.is_erased_class_type(decl.ty)
                && !self.is_py_exception_type(decl.ty)
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
            if self.local_uses_shared_capture_storage(local)
                && self.capture_cell_created_at_assignment(local)?
            {
                out.push_str(&format!(
                    "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>>;\n",
                    self.capture_cell_type(local, decl.ty)?
                ));
                self.deferred_capture_cells.borrow_mut().insert(local);
            } else if self.local_uses_shared_capture_storage(local) {
                out.push_str(&format!(
                    "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>> = ::std::rc::Rc::new(::std::cell::RefCell::new({}));\n",
                    self.capture_cell_type(local, decl.ty)?,
                    (if self.is_base_initializer_receiver(local) {
                        RustExpr::atom("Default::default()")
                    } else {
                        self.default_value(decl.ty)?
                    }).delimited()
                ));
            } else if self.predeclared_local_needs_default(local)?
                || self.local_may_be_used_before_assignment(local)?
            {
                out.push_str(&format!(
                    "    let {mutability}{}: {} = {};\n",
                    name,
                    self.local_declaration_type(local, decl.ty)?,
                    self.local_default_value(local, decl.ty)?.delimited()
                ));
            } else {
                out.push_str(&format!(
                    "    let {mutability}{}: {};\n",
                    name,
                    self.local_declaration_type(local, decl.ty)?
                ));
            }
            self.mark_local_declared(local);
        }
        Ok(())
    }

    /// Return the emitted Rust name of a MIR function, from the crate's name
    /// plan (see [`crate::names`]): a free function's crate name, or a class
    /// member's name inside its `impl` (`new` for a constructor).
    pub(super) fn function_rust_name(&self, function: &MirFunction) -> Result<String, EmitError> {
        let names = self.context.names();
        let planned = match function.origin {
            HirOrigin::Body(_) => names.function(function.id),
            HirOrigin::ClassMethod { method, .. } => names.method(method),
            HirOrigin::ClassStaticMethod { .. } => names.static_method(function.id),
            HirOrigin::ClassConstructor { .. } => return Ok("new".to_owned()),
        };
        planned
            .map(|name| name.as_str().to_owned())
            .ok_or_else(|| EmitError::new("function has no planned Rust name"))
    }

    /// The emitted-signature key for `function` (see [`SignatureKey`]).
    pub(super) fn emitted_signature_key(
        &self,
        function: &MirFunction,
    ) -> Result<SignatureKey, EmitError> {
        Ok(SignatureKey::of(function))
    }

    /// Returns the parameter types of a generated function by its identity.
    ///
    /// Function values can carry an instantiated generic call type even though
    /// this backend emits one erased Rust function. When a closure call points
    /// back at a generated function symbol, the emitted function signature is
    /// the ABI that call arguments must satisfy.
    pub(super) fn emitted_function_param_types(
        &self,
        key: &SignatureKey,
    ) -> Result<Option<Vec<TypeId>>, EmitError> {
        Ok(self.context.function_param_types.get(key).cloned())
    }

    /// Returns the emitted return type of a generated function by its identity.
    ///
    /// Cross-module calls can reference an imported overload signature in the
    /// caller's MIR while resolving to a concrete Rust function emitted from the
    /// source module. This is the return-side counterpart to emitted parameter
    /// lookup, and lets call sites adapt from the actual Rust ABI.
    pub(super) fn emitted_function_return_type(&self, key: &SignatureKey) -> Option<TypeId> {
        self.context.function_return_types.get(key).copied()
    }

    /// The Rust name of the declared class or interface `symbol` (see
    /// [`crate::names`]).
    pub(super) fn type_rust_name(&self, symbol: Symbol) -> Result<String, EmitError> {
        Ok(self.context.names().type_name(symbol)?.as_str().to_owned())
    }

    /// The Rust name of the instance method `method` (see [`crate::names`]).
    ///
    /// A method no class of this crate declares is a runtime or host type's
    /// own method, spelled as that type spells it.
    pub(super) fn method_rust_name(&self, method: Symbol) -> Result<String, EmitError> {
        Ok(match self.context.names().method(method) {
            Some(name) => name.as_str().to_owned(),
            None => crate::names::host_member(self.symbol_name(method)?).into_string(),
        })
    }

    /// Whether this method's declared return type is its own declaring class and
    /// every `return` in it answers the receiver.
    ///
    /// That is JavaScript's polymorphic `this` written in TypeScript's nominal
    /// spelling, and it is the only shape whose inherited copy may be re-typed
    /// to `Self`: a method that returns a FRESHLY BUILT base instance (a
    /// `clone()`-style factory) genuinely answers the base type and keeps it.
    pub(super) fn returns_the_receiver_at_its_own_class(&self) -> bool {
        crate::classes::returns_the_receiver_at_its_own_class(self.mir, self.function)
    }

    /// Emits a method or constructor definition, naming the site of any
    /// blocker (see [`Self::emit`]).
    pub(crate) fn emit_method(&mut self, out: &mut String) -> Result<(), EmitError> {
        self.apply_crate_generic_decision();
        let emitted = self.emit_method_definition(out);
        emitted.map_err(|error| error.with_site(|| self.current_function_site()))
    }

    /// Emits a method or constructor definition.
    fn emit_method_definition(&mut self, out: &mut String) -> Result<(), EmitError> {
        match self.function.origin {
            HirOrigin::ClassConstructor { .. } => {
                let method_params = self.param_decls_text(0)?;
                let return_text = self.self_return_text();
                if self.is_base_initializer_copy() {
                    // `fn __smelt_init_Base(smelt_receiver: Self, ..) -> Self`:
                    // the same body, handed the instance instead of building it.
                    let class =
                        crate::base_init::constructor_class(self.function).ok_or_else(|| {
                            EmitError::new("base initializer copy is not a constructor")
                        })?;
                    let name = crate::base_init::base_initializer_name(self.context, class)?;
                    let receiver = crate::base_init::BASE_INIT_RECEIVER;
                    let params = if method_params.is_empty() {
                        format!("{receiver}: Self")
                    } else {
                        format!("{receiver}: Self, {method_params}")
                    };
                    out.push_str(&format!("    fn {name}({params}) -> {return_text} {{\n"));
                } else {
                    out.push_str(&format!(
                        "    fn new({method_params}) -> {return_text} {{\n"
                    ));
                }
            }
            HirOrigin::ClassMethod { method, .. } => {
                let name = match &self.method_name {
                    Some(name) => name.clone(),
                    None => self.method_rust_name(method)?,
                };
                let method_params = self.param_decls_text(1)?;
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
                if self.function.is_async && !self.function.is_generator {
                    // Async-method owned-self transform: emit an ordinary
                    // `fn(&self, ..) -> SmeltFuture<T>` that clones `self` into an
                    // owned handle and runs the awaited body inside a moved
                    // `async` block. The returned future therefore owns its state
                    // and is `'static`, so specs can spawn `receiver.method()` as
                    // a detached task without the future borrowing the local
                    // receiver (E0597). Reference-class receivers clone as a cheap
                    // `Rc` handle preserving identity; value classes derive `Clone`.
                    let inner_ret = self.rendered_type(self.function.return_ty)?;
                    out.push_str(&format!(
                        "    fn {name}({rendered_params}) -> SmeltFuture<{inner_ret}> {{\n"
                    ));
                    return self.emit_async_method_owned_self_body(&inner_ret, out);
                }
                let return_text = if self.role.is_inherited_copy()
                    && self.returns_the_receiver_at_its_own_class()
                {
                    self.self_return_text().to_owned()
                } else {
                    self.return_type(self.function.return_ty)?.into_string()
                };
                out.push_str(&format!(
                    "    fn {name}({rendered_params}) -> {return_text} {{\n"
                ));
            }
            HirOrigin::ClassStaticMethod { .. } => {
                // A static method takes no receiver: emit every parameter and no
                // `self`, producing an associated function `Class::name(..)`.
                let name = static_method_rust_name(self.context, self.function.id)?;
                let method_params = self.param_decls_text(0)?;
                out.push_str(&format!(
                    "    {}fn {name}({method_params}) -> {} {{\n",
                    self.async_keyword(),
                    self.return_type(self.function.return_ty)?
                ));
            }
            HirOrigin::Body(_) => return self.emit(out, FreeFunctionVisibility::Private),
        }
        // Reference-class methods may capture `self` into an escaping closure;
        // that receiver must be bound once as a cloned handle before the body.
        // Value-class methods emit no prelude here, keeping their output
        // byte-identical to the pre-reference-class emitter.
        let is_generator = self.function.is_generator;
        if is_generator {
            let mut body = String::new();
            self.binding_uses.reset_self_owned();
            if self.method_owner_is_reference_class() {
                self.emit_shared_parameter_preludes(&mut body)?;
            }
            self.emit_block(self.entry_block()?, &mut body)?;
            if self.binding_uses.self_owned_used() {
                out.push_str(&format!("    let {SELF_OWNED_NAME} = self.clone();\n"));
            }
            self.emit_generator_wrapper(&body, out);
        } else {
            if self.method_owner_is_reference_class() {
                self.emit_shared_parameter_preludes(out)?;
            }
            // A method body needs the same function-scope declarations a free
            // function body gets. MIR locals are function-scoped while generated
            // Rust branch bodies are lexically scoped, so a temporary first
            // assigned inside one `if` arm and assigned again in the sibling arm
            // (or read after the branch) has to be declared OUTSIDE the branch.
            // Methods skipped this and emitted an inline `let mut` in the first
            // arm instead, so the sibling assignment referred to a name that was
            // out of scope: E0425 by the thousand in a branchy method (Hono's
            // routers), with no diagnostic anywhere before rustc.
            if let Some(prologue) = self.this_frame_prologue() {
                out.push_str(&format!("    {prologue}\n"));
            }
            self.emit_mutable_local_preludes(out)?;
            self.emit_block(self.entry_block()?, out)?;
        }
        out.push_str("    }\n");
        Ok(())
    }

    /// The keyword in front of `fn`: a generator's body is its own coroutine, so
    /// only a plain async function is declared `async`.
    const fn async_keyword(&self) -> &'static str {
        if self.function.is_async && !self.function.is_generator {
            "async "
        } else {
            ""
        }
    }

    /// The return type of a function that answers its own class.
    const fn self_return_text(&self) -> &'static str {
        if self.function.can_throw {
            crate::thrown::RESULT_SELF
        } else {
            "Self"
        }
    }

    /// Emit Rust's typed `Add<Rhs>` adapter for a Python `__add__` protocol.
    ///
    /// The source method remains the canonical implementation. Rust operator
    /// syntax consumes cloned operands at generated call sites, while the
    /// inherent Python method continues to borrow `self`; the adapter bridges
    /// those ownership conventions without dynamic dispatch or erasure.
    pub(crate) fn emit_python_add_impl(
        &mut self,
        out: &mut String,
        class_name: &str,
        impl_generics: &str,
        type_args: &str,
    ) -> Result<(), EmitError> {
        self.apply_crate_generic_decision();
        let emitted = self.emit_python_add_impl_body(out, class_name, impl_generics, type_args);
        emitted.map_err(|error| error.with_site(|| self.current_function_site()))
    }

    /// Emits the `std::ops::Add` impl body for a Python `__add__` method.
    fn emit_python_add_impl_body(
        &mut self,
        out: &mut String,
        class_name: &str,
        impl_generics: &str,
        type_args: &str,
    ) -> Result<(), EmitError> {
        let [_, rhs, ..] = self.function.params.as_slice() else {
            return Err(EmitError::new(
                "Python __add__ protocol method is missing its operand parameter",
            ));
        };
        let HirOrigin::ClassMethod { method, .. } = self.function.origin else {
            return Err(EmitError::new(
                "Python __add__ protocol target is not an instance method",
            ));
        };
        let rhs_ty = self.parameter_decl_type(*rhs)?;
        let output_ty = self.return_type(self.function.return_ty)?;
        let method_name = self.method_rust_name(method)?;
        out.push_str(&format!(
            "\nimpl{impl_generics} ::std::ops::Add<{rhs_ty}> for {class_name}{type_args} {{\n    type Output = {output_ty};\n    fn add(self, rhs: {rhs_ty}) -> Self::Output {{ self.{method_name}(rhs) }}\n}}\n"
        ));
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
    pub(super) fn emit_async_method_owned_self_body(
        &self,
        inner_ret: &str,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let mut body = String::new();
        self.binding_uses.reset_self_owned();
        if self.method_owner_is_reference_class() {
            self.emit_shared_parameter_preludes(&mut body)?;
        }
        self.emit_block(self.entry_block()?, &mut body)?;
        if self.binding_uses.self_owned_used() {
            out.push_str(&format!("    let {SELF_OWNED_NAME} = self.clone();\n"));
        }
        // A genuine async-method body: prime it so its synchronous prefix runs
        // at call time (JS eager-async-prefix semantics; see `from_future_primed`).
        out.push_str(&format!(
            "    SmeltFuture::<{inner_ret}>::from_future_primed(Box::pin(async move {{\n"
        ));
        out.push_str(&body);
        out.push_str("    }))\n    }\n");
        Ok(())
    }
}

/// The key the emitted-signature maps use for one generated Rust function.
///
/// A method's Rust name is unique only INSIDE its `impl` block: two classes may
/// both emit `fn bump`. Keying the emitted parameter/return types by the bare
/// name therefore let one class's method answer for another's — and since those
/// maps decide the type a call site converts FROM, a call to
/// `Second::bump(): string` was converted from `First::bump()`'s `()`, which
/// renders a constant and drops the call itself (H63: `second.bump('!')`
/// answered `""`).
///
/// The key is the emitted item's identity, never its rendering: the function
/// id for a free function (whose name the plan made unique), and the owning
/// class, the member's symbol and its kind for a constructor, method or static
/// method. Overload implementations that share ONE emitted member share
/// one key, so the priority rule picks between their signatures; a static and
/// an instance method of one name have different keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SignatureKey {
    /// A free function.
    Free(FuncId),
    /// An associated item of a class `impl`.
    Member {
        /// The class whose `impl` declares it.
        class: Symbol,
        /// The member's name symbol.
        name: Symbol,
        /// Which kind of associated item.
        kind: MemberKind,
    },
}

/// The kind of a class's associated item, which shares the class's one `impl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MemberKind {
    /// The constructor `new`.
    Constructor,
    /// An instance method.
    Instance,
    /// A static (associated) method.
    Static,
}

impl SignatureKey {
    /// The key of `function`.
    pub(crate) const fn of(function: &MirFunction) -> Self {
        match function.origin {
            HirOrigin::Body(_) => Self::Free(function.id),
            HirOrigin::ClassConstructor { class, .. } => Self::Member {
                class,
                name: function.name,
                kind: MemberKind::Constructor,
            },
            HirOrigin::ClassMethod { class, method, .. } => Self::Member {
                class,
                name: method,
                kind: MemberKind::Instance,
            },
            HirOrigin::ClassStaticMethod { class, method, .. } => Self::Member {
                class,
                name: method,
                kind: MemberKind::Static,
            },
        }
    }
}

/// The Rust name of the static method `function`, planned inside its class's
/// `impl` block (see [`crate::names::RustNames::static_method`]).
pub(crate) fn static_method_rust_name(
    context: &EmitContext,
    function: FuncId,
) -> Result<String, EmitError> {
    context
        .names()
        .static_method(function)
        .map(|name| name.as_str().to_owned())
        .ok_or_else(|| EmitError::new("static method has no planned Rust name"))
}
