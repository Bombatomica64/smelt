//! Parameter and closure-capture ownership analysis: which parameters need mutable-reference ABI, which captured bindings must use shared (RefCell) storage, and the closure-body write detection behind those decisions.

use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
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
        self.parameter_needs_mutable_reference_in_seen(function, local, &mut Vec::new())
    }

    /// Returns whether a parameter type carries JavaScript object identity.
    ///
    /// Mutating fields or indexed entries through such a parameter must update
    /// the caller-visible value. Plain scalar parameters still remain owned
    /// Rust values because rebinding the parameter itself does not update the
    /// source caller's binding.
    pub(super) fn parameter_type_has_shared_mutation_semantics(&self, ty: TypeId) -> bool {
        // A reference-class parameter is a handle: passing it by value shares the
        // underlying cell, so mutations already reach the caller's object and the
        // `&mut` ABI is neither needed nor wanted (`&self` methods only). This is
        // also what fixes the `Mutex`→`Semaphore` throwaway-clone miscompile:
        // once `Semaphore` is a reference class, delegating to `self.semaphore`
        // no longer demands a `&mut` the `&self` caller cannot supply.
        if self.is_reference_class_type(ty) {
            return false;
        }
        self.mir.types.get(ty).is_some_and(|kind| {
            matches!(kind, Type::List(_) | Type::Set(_) | Type::Dict(_, _))
                || matches!(kind, Type::Class { .. })
        })
    }

    /// Returns whether a parameter needs mutable-reference ABI, tracking the
    /// active query stack so recursive forwarding does not loop forever.
    fn parameter_needs_mutable_reference_in_seen(
        &self,
        function: &MirFunction,
        local: LocalId,
        seen: &mut Vec<(usize, LocalId)>,
    ) -> bool {
        if !function.params.contains(&local) {
            return false;
        }
        if !self
            .function_local_decl(function, local)
            .is_ok_and(|decl| self.parameter_type_has_shared_mutation_semantics(decl.ty))
        {
            return false;
        }
        let key = (std::ptr::from_ref(function).addr(), local);
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        let needs_reference = function.blocks.iter().any(|block| {
            if block.statements.iter().any(|statement| match statement {
                // Only in-place mutation through the parameter (`param.field = …`
                // / `param[i] = …`) needs the shared `&mut` ABI so the write
                // reaches the caller's value. Rebinding the whole parameter
                // (`Place::Local`, e.g. `iteratees = iteratees[0]`) is a local
                // reassignment that JavaScript never propagates to the caller, so
                // it stays an owned `mut` binding — matching this function's
                // contract. Lumping `Place::Local` in here made owned rebinds emit
                // `&mut SmeltList<…>` and then fail to accept an owned assignment.
                Statement::AssignPlace {
                    place:
                        Place::Field {
                            base: candidate, ..
                        }
                        | Place::Index {
                            base: candidate, ..
                        },
                    ..
                } if *candidate == local => true,
                Statement::Assign { value, .. } => {
                    self.rvalue_mutates_local(value, local)
                        || self.rvalue_forwards_local_to_mutable_callback(function, value, local)
                }
                _ => false,
            }) {
                return true;
            }
            if let Some(Terminator::Call { callee, args, .. }) = &block.terminator {
                return match callee {
                    Callee::Static(function_id) => args.iter().enumerate().any(|(index, arg)| {
                        Self::operand_originates_from_local(function, arg, local, &mut Vec::new())
                            && self
                                .mir
                                .functions
                                .get(usize::try_from(function_id.0).unwrap_or(usize::MAX))
                                .and_then(|called_function| {
                                    called_function.params.get(index).map(|param| {
                                        self.parameter_needs_mutable_reference_in_seen(
                                            called_function,
                                            *param,
                                            seen,
                                        )
                                    })
                                })
                                .unwrap_or(false)
                    }),
                    Callee::Indirect(operand) => {
                        let callee_ty = match operand {
                            Operand::Copy(Place::Local(callee_local))
                            | Operand::Move(Place::Local(callee_local)) => self
                                .function_local_decl(function, *callee_local)
                                .ok()
                                .map(|decl| decl.ty),
                            Operand::Copy(Place::Field { base, field })
                            | Operand::Move(Place::Field { base, field }) => self
                                .function_local_decl(function, *base)
                                .ok()
                                .and_then(|decl| self.structural_record_fields(decl.ty))
                                .and_then(|fields| {
                                    fields
                                        .into_iter()
                                        .find(|candidate| candidate.name == *field)
                                        .map(|candidate| candidate.ty)
                                }),
                            _ => None,
                        };
                        let function_ty = callee_ty.and_then(|ty| self.mir.types.get(ty)).and_then(
                            |ty| match ty {
                                Type::Function(function_ty) => Some(function_ty),
                                _ => None,
                            },
                        );
                        function_ty.is_some_and(|callback_ty| {
                            args.iter().enumerate().any(|(index, arg)| {
                                Self::operand_originates_from_local(
                                    function,
                                    arg,
                                    local,
                                    &mut Vec::new(),
                                ) && callback_ty.mutable_params.contains(&index)
                            })
                        })
                    }
                    Callee::Builtin(_) => false,
                };
            }
            false
        });
        seen.pop();
        needs_reference
    }

    /// Return whether an rvalue forwards a parameter into a mutable callback slot.
    fn rvalue_forwards_local_to_mutable_callback(
        &self,
        function: &MirFunction,
        value: &Rvalue,
        local: LocalId,
    ) -> bool {
        let Rvalue::ClosureCall { callee, args } = value else {
            return false;
        };
        let callee_ty = match callee {
            Operand::Copy(Place::Local(callee_local))
            | Operand::Move(Place::Local(callee_local)) => self
                .function_local_decl(function, *callee_local)
                .ok()
                .map(|decl| decl.ty),
            Operand::Copy(Place::Field { base, field })
            | Operand::Move(Place::Field { base, field }) => self
                .function_local_decl(function, *base)
                .ok()
                .and_then(|decl| self.structural_record_fields(decl.ty))
                .and_then(|fields| {
                    fields
                        .into_iter()
                        .find(|candidate| candidate.name == *field)
                        .map(|candidate| candidate.ty)
                }),
            _ => None,
        };
        let Some(function_ty) = callee_ty
            .and_then(|ty| self.mir.types.get(ty))
            .and_then(|ty| match ty {
                Type::Function(function_ty) => Some(function_ty),
                _ => None,
            })
        else {
            return false;
        };
        args.iter().enumerate().any(|(index, arg)| {
            function_ty.mutable_params.contains(&index)
                && Self::operand_originates_from_local(function, arg, local, &mut Vec::new())
        })
    }

    /// Return whether an operand is the parameter itself or a temporary copy of it.
    ///
    /// MIR introduces copy temporaries before calls. Mutation-ABI analysis must
    /// follow those aliases so forwarding a shared object into a mutable
    /// callback still borrows the original caller-visible value.
    fn operand_originates_from_local(
        function: &MirFunction,
        operand: &Operand,
        origin: LocalId,
        seen: &mut Vec<LocalId>,
    ) -> bool {
        Self::operand_originates_from_local_in_blocks(&function.blocks, operand, origin, seen)
    }

    /// Like `operand_originates_from_local` but works on a raw block slice so it
    /// can also analyze closure bodies (which are not `MirFunction`s).
    fn operand_originates_from_local_in_blocks(
        blocks: &[BasicBlock],
        operand: &Operand,
        origin: LocalId,
        seen: &mut Vec<LocalId>,
    ) -> bool {
        let Some(local) = operand_local(operand) else {
            return false;
        };
        if local == origin {
            return true;
        }
        if seen.contains(&local) {
            return false;
        }
        seen.push(local);
        let result = blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                matches!(
                    statement,
                    Statement::Assign {
                        dest,
                        value: Rvalue::Use(source),
                    } if *dest == local
                        && Self::operand_originates_from_local_in_blocks(
                            blocks, source, origin, seen,
                        )
                )
            })
        });
        seen.pop();
        result
    }

    /// Renders an argument for a mutable-reference collection parameter.
    ///
    /// `callee_emission_scope` is the set of type parameters the callee actually
    /// emits as real Rust generics, supplied only for a *static* call where a
    /// callee `MirFunction` exists. It is needed because MIR type identity is
    /// not Rust type identity: [`smelt_hir::Symbol`] is name-interned, so a
    /// generic caller's `SmeltList<T>` local and an erased callee's declared
    /// `T[]` parameter are the SAME `TypeId` while rendering as
    /// `SmeltList<T>` and `SmeltList<SmeltUnknown>`. A `&mut` reference is
    /// invariant in its element, so passing the place straight through on
    /// TypeId equality alone is `E0308`.
    ///
    /// When the two renderings disagree the argument is rendered through
    /// [`FunctionEmitter::demoting_mutable_reference_text`], which is a
    /// *demotion signal* rather than a rendering the emitted crate keeps: it
    /// carries an `into_smelt_unknown` token, so the body-cleanliness trial
    /// (`crate::emitter::core::body_needs_erased_carrier`) rejects the caller's
    /// generic signature, and the re-render with both sides erased takes the
    /// pass-through branch again. Declining to emit generics is always sound;
    /// emitting an invariant `&mut` at the wrong element type is not.
    ///
    /// `None` means "no callee emission scope to compare against" — an indirect
    /// call through a function *value*, whose target type is rendered in the
    /// caller's own scope, so MIR identity already implies rendered identity.
    pub(super) fn mutable_reference_argument_text(
        &self,
        operand: &Operand,
        target: TypeId,
        callee_emission_scope: Option<&HashSet<Symbol>>,
    ) -> Result<String, EmitError> {
        let text = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if let Place::Local(local) = place
                    && self.function.id.0 != u32::MAX
                    && self.function.params.contains(local)
                    && self.parameter_needs_mutable_reference(*local)
                {
                    return self.place_text(place);
                }
                let place_ty = self.place_ty(place)?;
                if place_ty == target {
                    if self.mutable_reference_renders_alike(
                        place_ty,
                        target,
                        callee_emission_scope,
                    )? {
                        self.place_text(place)?
                    } else {
                        self.demoting_mutable_reference_text(place, place_ty)?
                    }
                } else {
                    self.value_at_type(operand, target)?
                }
            }
            Operand::Const(_) => self.value_at_type(operand, target)?,
        };
        Ok(format!("&mut {text}"))
    }

    /// Return whether the caller's rendering of `arg_ty` equals the callee's
    /// rendering of `target`, so a `&mut` borrow of the place is type-correct.
    ///
    /// Always `true` when there is no callee emission scope to compare against;
    /// see [`FunctionEmitter::mutable_reference_argument_text`].
    fn mutable_reference_renders_alike(
        &self,
        arg_ty: TypeId,
        target: TypeId,
        callee_emission_scope: Option<&HashSet<Symbol>>,
    ) -> Result<bool, EmitError> {
        let Some(scope) = callee_emission_scope else {
            return Ok(true);
        };
        // Only a *demotable* caller may report a mismatch. The disagreement is
        // reported by rendering text the body-cleanliness trial rejects, and
        // only a generic free function has that trial; a class method carries
        // its `impl<T>` parameters unconditionally and cannot fall back, so
        // reporting there would replace a compile error with a silently
        // mutation-losing temporary. Those keep their previous answer exactly.
        if !matches!(self.function.origin, HirOrigin::Body(_)) {
            return Ok(true);
        }
        let caller_scope = self.current_function_type_params();
        if caller_scope.is_empty() {
            return Ok(true);
        }
        let caller_text = self
            .rust_type(arg_ty, false, &TypeSubstitution::lexical(&caller_scope))?
            .into_string();
        let target_text = self
            .rust_type(target, false, &TypeSubstitution::callee_emission(scope))?
            .into_string();
        Ok(caller_text == target_text)
    }

    /// Render a `&mut` argument whose caller and callee renderings disagree.
    ///
    /// This exists to be *rejected*. `&mut` is invariant, so there is no inline
    /// expression that bridges a caller's `SmeltList<T>` to a callee's
    /// `SmeltList<SmeltUnknown>` while preserving the callee's write-back; the
    /// convert-in-place adapter (`call::static_call_mut_list_adapter_text`) is
    /// the mechanism that does, and it declines on the throwing-call path.
    ///
    /// So instead of emitting an unsound borrow, this emits the element-wise
    /// erasure of the place — a type-correct expression that *does* carry an
    /// `into_smelt_unknown` token. `body_needs_erased_carrier` sees the token in
    /// the caller's trial body, the caller demotes to full erasure, and the
    /// re-render finds both sides erased and passes the place through as before.
    /// The text below therefore never reaches the emitted crate; the guards in
    /// [`FunctionEmitter::mutable_reference_renders_alike`] are what make that
    /// true, and they must not be relaxed without replacing this mechanism.
    fn demoting_mutable_reference_text(
        &self,
        place: &Place,
        place_ty: TypeId,
    ) -> Result<String, EmitError> {
        let place_text = self.place_text(place)?;
        if matches!(self.mir.types.get(place_ty), Some(Type::List(_))) {
            return Ok(format!(
                "{place_text}.clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<SmeltList<_>>()"
            ));
        }
        Ok(format!("{place_text}.clone().into_smelt_unknown()"))
    }

    /// Returns whether a non-escaping capture must share outer storage.
    ///
    /// JavaScript closures observe the same binding as the outer scope. The
    /// generated Rust uses shared local storage when the closure body writes
    /// through that capture or when the binding is assigned after the closure
    /// is created; read-only captures of already-initialized bindings can remain
    /// cloned.
    pub(super) fn closure_capture_needs_shared_access(
        &self,
        closure: &MirClosure,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        if capture.mode == smelt_hir::CaptureMode::ByMut {
            return true;
        }
        if self.local_is_assigned_after_capture(capture.source_local) {
            return true;
        }
        if closure.escapes
            && !self
                .local_decl(capture.source_local)
                .is_ok_and(|local| matches!(self.mir.types.get(local.ty), Some(Type::Function(_))))
            && self.escaping_closure_capture_is_mutated(capture.source_local)
        {
            // A binding mutated inside an escaping closure needs a shared cell so
            // the closure (stored as an `Rc<dyn Fn>`, hence only `Fn`) can mutate
            // it through interior mutability instead of requiring `FnMut`. This
            // holds whether the binding is an outer `let` or a function parameter
            // (e.g. `after`/`before`, which decrement their captured `n`
            // parameter on each call), so parameters are not excluded here.
            return true;
        }
        if capture.mode == smelt_hir::CaptureMode::ByValue {
            return false;
        }
        self.closure_capture_body_writes(closure, capture)
    }

    /// Return whether closure creation precedes an assignment to a captured binding.
    ///
    /// This is the generated-Rust storage requirement for source such as
    /// `const recursive = factory(() => recursive())`: the callback is built
    /// before the factory result initializes `recursive`, but JavaScript reads
    /// the eventual binding value when the callback executes.
    fn local_is_assigned_after_capture(&self, local: LocalId) -> bool {
        let mut observed_capture = false;
        for block in &self.function.blocks {
            for statement in &block.statements {
                match statement {
                    Statement::Assign {
                        value: Rvalue::Closure { id, .. },
                        ..
                    } => {
                        observed_capture |= self
                            .mir
                            .closures
                            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                            .is_some_and(|closure| {
                                closure
                                    .captures
                                    .iter()
                                    .any(|capture| capture.source_local == local)
                            });
                    }
                    Statement::Assign { dest, .. } if observed_capture && *dest == local => {
                        return true;
                    }
                    Statement::AssignPlace {
                        place: Place::Local(candidate),
                        ..
                    } if observed_capture && *candidate == local => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// Return whether an outer binding needs storage shared with a closure.
    ///
    /// Mutating captures observe the same JavaScript binding as reads and
    /// assignments in their defining function, so all access must use one
    /// generated `RefCell`.
    pub(super) fn local_uses_shared_capture_storage(&self, local: LocalId) -> bool {
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
                            capture.source_local == local
                                && self.closure_capture_needs_shared_access(closure, capture)
                        })
                    })
            })
        })
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
    ///
    /// This is the sole gate for making a cloned collection capture `mut` now
    /// that the capture prelude no longer forces `mut` on every list/set/dict.
    /// It therefore has to recognize *every* way a closure body can write to the
    /// capture, and it stays deliberately conservative: any path it cannot prove
    /// to be read-only counts as a write. Covered write shapes:
    ///
    /// * direct rebinds (`Statement::Assign { dest == target }`) and in-place
    ///   place assignments (`x.field = …`, `x[i] = …`, `x = …`),
    /// * mutating collection rvalues (`push`, `set`, `sort`, …) via
    ///   `rvalue_mutates_local`,
    /// * callback rvalues that borrow the capture mutably
    ///   (`rvalue_borrows_local_mutably`),
    /// * forwarding the capture into a callee parameter or callback slot that
    ///   takes a mutable reference, both through `Rvalue::ClosureCall`
    ///   statements and through `Terminator::Call` (static and indirect callees),
    /// * `Terminator::Call` whose result destination is the capture itself.
    pub(super) fn closure_capture_body_writes(
        &self,
        closure: &MirClosure,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        let Some(target) = capture.target_local else {
            return false;
        };
        closure.blocks.iter().any(|block| {
            let statement_writes = block.statements.iter().any(|statement| match statement {
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
                Statement::Assign { value, .. } => {
                    self.rvalue_mutates_local(value, target)
                        || self.rvalue_borrows_local_mutably(value, target)
                        || self.closure_rvalue_forwards_local_to_mutable_callback(
                            closure, value, target,
                        )
                }
                _ => false,
            });
            statement_writes
                || self.closure_terminator_writes_local(closure, block.terminator.as_ref(), target)
        })
    }

    /// Returns whether a closure block terminator writes to `target`.
    ///
    /// Mirrors the mutable-reference analysis used for function parameters but
    /// operates on a closure's own blocks and local table. A terminator writes
    /// the capture when it stores its call result into it, forwards it into a
    /// static callee parameter that needs a mutable reference, or forwards it
    /// into an indirect callback slot declared mutable.
    fn closure_terminator_writes_local(
        &self,
        closure: &MirClosure,
        terminator: Option<&Terminator>,
        target: LocalId,
    ) -> bool {
        let Some(Terminator::Call {
            callee, args, dest, ..
        }) = terminator
        else {
            return false;
        };
        if *dest == target {
            return true;
        }
        match callee {
            Callee::Static(function_id) => args.iter().enumerate().any(|(index, arg)| {
                Self::operand_originates_from_local_in_blocks(
                    &closure.blocks,
                    arg,
                    target,
                    &mut Vec::new(),
                ) && self
                    .mir
                    .functions
                    .get(usize::try_from(function_id.0).unwrap_or(usize::MAX))
                    .and_then(|called_function| {
                        called_function.params.get(index).map(|param| {
                            self.parameter_needs_mutable_reference_in(called_function, *param)
                        })
                    })
                    .unwrap_or(false)
            }),
            Callee::Indirect(operand) => {
                let Some(function_ty) = self
                    .closure_operand_function_type(closure, operand)
                    .filter(|ty| !ty.mutable_params.is_empty())
                else {
                    return false;
                };
                args.iter().enumerate().any(|(index, arg)| {
                    function_ty.mutable_params.contains(&index)
                        && Self::operand_originates_from_local_in_blocks(
                            &closure.blocks,
                            arg,
                            target,
                            &mut Vec::new(),
                        )
                })
            }
            Callee::Builtin(_) => false,
        }
    }

    /// Returns whether a closure-body `ClosureCall` forwards `target` into a
    /// mutable callback parameter slot.
    fn closure_rvalue_forwards_local_to_mutable_callback(
        &self,
        closure: &MirClosure,
        value: &Rvalue,
        target: LocalId,
    ) -> bool {
        let Rvalue::ClosureCall { callee, args } = value else {
            return false;
        };
        let Some(function_ty) = self
            .closure_operand_function_type(closure, callee)
            .filter(|ty| !ty.mutable_params.is_empty())
        else {
            return false;
        };
        args.iter().enumerate().any(|(index, arg)| {
            function_ty.mutable_params.contains(&index)
                && Self::operand_originates_from_local_in_blocks(
                    &closure.blocks,
                    arg,
                    target,
                    &mut Vec::new(),
                )
        })
    }

    /// Resolves the `FunctionType` of a callee operand within a closure body.
    ///
    /// Looks the operand up in the closure's own local table (closure locals are
    /// scoped to the closure, not to `self.function`), following either a plain
    /// local or a structural record field.
    fn closure_operand_function_type(
        &self,
        closure: &MirClosure,
        operand: &Operand,
    ) -> Option<&FunctionType> {
        let callee_ty = match operand {
            Operand::Copy(Place::Local(callee_local))
            | Operand::Move(Place::Local(callee_local)) => closure
                .locals
                .get(id_index(callee_local.0, "closure local index does not fit usize").ok()?)
                .map(|decl| decl.ty),
            Operand::Copy(Place::Field { base, field })
            | Operand::Move(Place::Field { base, field }) => closure
                .locals
                .get(id_index(base.0, "closure local index does not fit usize").ok()?)
                .and_then(|decl| self.structural_record_fields(decl.ty))
                .and_then(|fields| {
                    fields
                        .into_iter()
                        .find(|candidate| candidate.name == *field)
                        .map(|candidate| candidate.ty)
                }),
            _ => None,
        }?;
        match self.mir.types.get(callee_ty) {
            Some(Type::Function(function_ty)) => Some(function_ty),
            _ => None,
        }
    }

}
