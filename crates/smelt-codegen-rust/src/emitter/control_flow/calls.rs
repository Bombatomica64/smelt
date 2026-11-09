//! Call and `await` terminators: the plain statement form, and the throwing form
//! that routes `Ok` and `Err` to their continuations.

use super::*;
use crate::rust::{RustExpr, RustType};

impl FunctionEmitter<'_> {
    /// Emits the assignment part of a call terminator without following it.
    pub(in crate::emitter) fn emit_call_terminator_statement(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest: LocalId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        let name = self.local_name(dest)?;
        // A `Function`-typed call result whose only consumer erases it back to
        // `SmeltUnknown` re-renders the call at the erase site (see
        // `coercion::erased_call_assignment_expr`). Emitting the typed-callback
        // binding here would be a dead store that also evaluates the call a
        // second time, double-moving its arguments. Suppress it so the single
        // re-inlined erase is the lone evaluation.
        if self.function_call_result_dead_when_erased(dest)? {
            self.mark_local_declared(dest);
            return Ok(());
        }
        if !self.local_has_uses(dest) && self.mir.types.get(local.ty) == Some(&Type::None) {
            // A discarded (void) call still needs the convert-in-place adapter
            // when it forwards a `&mut` list whose element type differs from the
            // callee's: the callee mutates the erased temporary and those
            // mutations must be written back through the reference even though
            // the returned value is dropped.
            if let Callee::Static(func) = callee
                && let Some(adapter) =
                    self.static_call_mut_list_adapter(*func, args, local.ty)?
            {
                out.push_str(&format!("    let _ = {adapter};\n", adapter = adapter.delimited()));
                self.mark_local_declared(dest);
                return Ok(());
            }
            let call_text = self.rendered_call(callee, args)?.into_expression();
            out.push_str(&format!("    let _ = {call_text};\n", call_text = call_text.delimited()));
            self.mark_local_declared(dest);
            return Ok(());
        }
        let call_text = self.call_expr_for_dest(callee, args, local.ty)?;
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        if matches!(
            self.mir.types.get(local.ty),
            Some(Type::Future(_) | Type::Function(_))
        ) {
            out.push_str(&format!("    let {mutability}{name} = {call_text};\n", call_text = call_text.delimited()));
        } else {
            out.push_str(&format!(
                "    let {mutability}{name}: {} = {call_text};\n",
                self.call_dest_type(callee, local.ty)?, call_text = call_text.delimited()
            ));
        }
        self.mark_local_declared(dest);
        Ok(())
    }

    /// The Rust type a call's destination binding is declared at.
    ///
    /// A base initializer answers the receiver it was handed, which is `Self`:
    /// the derived instance under construction. Its MIR type names the class
    /// whose constructor issued the `super(..)`, and inside an initializer copy
    /// re-emitted further down the chain (`Point3`'s constructor inside `impl
    /// Point4`) that class is NOT the impl's type.
    fn call_dest_type(&self, callee: &Callee, ty: TypeId) -> Result<RustType, EmitError> {
        if matches!(callee, Callee::BaseInit(_)) {
            return Ok(RustType::raw("Self"));
        }
        self.rendered_type(ty)
    }

    /// Emits the binding part of a non-throwing `await` without following it.
    ///
    /// Extracted from [`Self::emit_terminator`] so the loop-body emitters can
    /// bind the awaited value and then continue in loop context instead of
    /// delegating to `emit_terminator`, which would emit the successor region as
    /// a fresh top-level block and lose the surrounding loop's back edge.
    pub(super) fn emit_await_terminator_statement(
        &self,
        future: &Operand,
        dest: LocalId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        let name = self.local_name(dest)?;
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        let raw_value = RustExpr::loose(format!("{}.await?", self.await_operand_expr(future)?.tight()));
        let source_ty = self.awaited_output_ty(future)?;
        let value = self.rendered_at_type(&raw_value, source_ty, local.ty, &self.render_scope())?;
        if matches!(
            self.mir.types.get(local.ty),
            Some(Type::Future(_) | Type::Function(_))
        ) {
            out.push_str(&format!("    let {mutability}{name} = {value};\n", value = value.delimited()));
        } else {
            out.push_str(&format!(
                "    let {mutability}{name}: {} = {value};\n",
                self.rendered_type(local.ty)?, value = value.delimited()
            ));
        }
        self.mark_local_declared(dest);
        Ok(())
    }

    /// Emits a throwing call with explicit normal and exception continuations.
    pub(in crate::emitter) fn emit_throwing_call_terminator(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest: LocalId,
        target: smelt_mir::BlockId,
        handler: smelt_mir::ExceptionHandler,
        continuation: &Continuation<'_>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        // Rejoin the arms at their common tail when the region allows it (see
        // `throwing_join`): each arm stops there and the tail is emitted once.
        let declared_before_arms = self.declared_scope();
        let join = self.throwing_continuation_join(target, handler.catch_block, continuation);
        let fresh_visited = BlockIdSet::default();
        let joined_continuation = join.and_then(|join| match continuation {
            Continuation::Block => Some(Continuation::Region {
                stop: join,
                visited: &fresh_visited,
            }),
            Continuation::Region { visited, .. } => Some(Continuation::Region {
                stop: join,
                visited,
            }),
            Continuation::Closure { active, .. } => Some(Continuation::Closure {
                active,
                stop: Some(join),
                in_loop: true,
            }),
            Continuation::InLoop { .. } => None,
        });
        let arm_continuation = joined_continuation.as_ref().unwrap_or(continuation);
        let joined_tail = join.filter(|_| joined_continuation.is_some());
        let hoist_dest = joined_tail.is_some_and(|tail| !self.is_local_declared(dest) && self.join_tail_reads_local(tail, dest));
        let rendered = self.rendered_call(callee, args)?;
        if !rendered.propagates() {
            let call_text = rendered.into_call();
            let name = self.local_name(dest)?;
            let dest_type_text = if matches!(self.mir.types.get(local.ty), Some(Type::Function(_) | Type::Future(_))) {
                None
            } else {
                Some(self.call_dest_type(callee, local.ty)?)
            };
            if hoist_dest {
                out.push_str(&self.throwing_dest_predeclaration(dest, dest_type_text.as_deref())?);
                self.mark_local_declared(dest);
            }
            out.push_str(&format!(
                "    match {{ smelt_panic_catch_enter(); let smelt_caught = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {call_text})); smelt_panic_catch_leave(); smelt_caught }} {{\n", call_text = call_text.delimited()
            ));
            out.push_str("        Ok(__smelt_value) => {\n");
            let mutability = if self.local_binding_needs_mut(dest) {
                "mut "
            } else {
                ""
            };
            // A fully-erased callee returns its future/handle at the erased item
            // type (e.g. `SmeltFuture<SmeltUnknown>`) while a specialized call
            // site declares a concrete `Future<T>`. Coerce the caught value from
            // the call's actual emitted type to the destination so a later
            // `await` yields `T` directly; when they already match,
            // `rendered_at_type` returns the value unchanged. A function-typed
            // destination is left untouched (function handles are not coerced
            // through this path).
            let value_text = if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                RustExpr::path("__smelt_value")
            } else {
                let source_ty = self.call_emitted_source_ty(callee, args, local.ty)?;
                self.rendered_at_type(&RustExpr::path("__smelt_value"), source_ty, local.ty, &self.render_scope())?
            };
            if hoist_dest {
                out.push_str(&format!("            {name} = {value_text};\n", value_text = value_text.delimited()));
            } else if let Some(type_text) = &dest_type_text {
                out.push_str(&format!("            let {mutability}{name}: {type_text} = {value_text};\n", value_text = value_text.delimited()));
            } else {
                out.push_str(&format!("            let {mutability}{name} = {value_text};\n", value_text = value_text.delimited()));
            }
            self.mark_local_declared(dest);
            let __smelt_arm_declared = self.declared_scope();
            self.emit_continuation(target, arm_continuation, out)?;
            out.push_str("        }\n");
            // Each arm of the emitted `match` re-emits the SAME continuation,
            // and each is its own Rust lexical scope. Restoring the declaration
            // set before the next arm is what makes it declare its own copy of
            // a join temporary instead of assigning to the name the first arm
            // declared -- the E0425 in
            // `blocker-logs/top-level-try-await-tail.md`. The same
            // snapshot-per-arm rule the closure emitter already follows.
            __smelt_arm_declared.reset();
            out.push_str("        Err(__smelt_panic) => {\n");
            out.push_str(&format!(
                "            let __smelt_error = {};\n",
                crate::thrown::caught_panic_message_expr("__smelt_panic").delimited()
            ));
            if let Some(exception_local) = handler.exception_local {
                let exception_name = self.local_name(exception_local)?;
                let exception_decl = self.local_decl(exception_local)?;
                let value = match self.mir.types.get(exception_decl.ty) {
                    Some(Type::String) => RustExpr::chain("JsString::from(__smelt_error.as_str())"),
                    Some(Type::PyStr) => RustExpr::path("__smelt_error"),
                    // The recovered panic is presented through the shared
                    // exception-payload record, built by
                    // `caught_panic_error_value_expr`; see
                    // `runtime/thrown.rs` for why this is a real
                    // dynamic boundary rather than avoidable erasure.
                    // The class comes from the panic payload, not a hard-coded
                    // `Error`: a `URIError` routed through the panic channel
                    // must still answer `error.name === 'URIError'`. See
                    // `runtime/thrown.rs`.
                    Some(Type::Unknown) => {
                        crate::thrown::caught_panic_error_value_expr("__smelt_panic")
                    }
                    // A panic is an error the runtime raised without a Python class.
                    _ if self.is_py_exception_type(exception_decl.ty) => {
                        RustExpr::atom("SmeltPyException::from_message(__smelt_error)")
                    }
                    _ => self.default_value(exception_decl.ty)?,
                };
                out.push_str(self.exception_binding_expr(exception_local, &exception_name, &value).as_str());
                self.mark_local_declared(exception_local);
            }
            self.emit_continuation(handler.catch_block, arm_continuation, out)?;
            __smelt_arm_declared.close();
            out.push_str("        }\n");
            out.push_str("    }\n");
            if let Some(join) = join.filter(|_| joined_continuation.is_some()) {
                declared_before_arms.close();
                if hoist_dest {
                    self.mark_local_declared(dest);
                }
                self.emit_throwing_continuation_tail(join, continuation, out)?;
            } else {
                // No join: the continuation was emitted inside the arms, and the
                // declarations stay as the arms left them.
                declared_before_arms.keep();
            }
            return Ok(());
        }
        let raw_call = rendered.into_call();
        let source_ty = self.call_source_ty(callee)?;
        // Seam 3 (`emitter::seam_assertions`): `__smelt_value` below is the
        // value the EMITTED call produced, and it is coerced from the DECLARED
        // return type. The remaining site that still asks the declared question
        // about a call result; both answers are computable from what is in
        // scope, so compare them.
        #[cfg(debug_assertions)]
        self.debug_assert_declared_return_is_emitted_return(callee, args, local.ty, source_ty);
        let name = self.local_name(dest)?;
        let dest_type_text = if matches!(self.mir.types.get(local.ty), Some(Type::Future(_) | Type::Function(_))) {
            None
        } else {
            Some(self.call_dest_type(callee, local.ty)?)
        };
        if hoist_dest {
            out.push_str(&self.throwing_dest_predeclaration(dest, dest_type_text.as_deref())?);
            self.mark_local_declared(dest);
        }
        out.push_str(&format!(
            "    match {{ smelt_panic_catch_enter(); let smelt_caught = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {raw_call})); smelt_panic_catch_leave(); smelt_caught }} {{\n", raw_call = raw_call.delimited()
        ));
        out.push_str("        Ok(Ok(__smelt_value)) => {\n");
        let value_text = self.rendered_at_type(&RustExpr::path("__smelt_value"), source_ty, local.ty, &self.render_scope())?;
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        if hoist_dest {
            out.push_str(&format!("            {name} = {value_text};\n", value_text = value_text.delimited()));
        } else if let Some(type_text) = &dest_type_text {
            out.push_str(&format!("            let {mutability}{name}: {type_text} = {value_text};\n", value_text = value_text.delimited()));
        } else {
            out.push_str(&format!("            let {mutability}{name} = {value_text};\n", value_text = value_text.delimited()));
        }
        self.mark_local_declared(dest);
        // Each of the three arms re-emits the SAME continuation into its own
        // Rust lexical scope, so each needs its own declaration set — see the
        // note on the two-arm site above.
        let __smelt_arm_declared = self.declared_scope();
        self.emit_continuation(target, arm_continuation, out)?;
        out.push_str("        }\n");
        __smelt_arm_declared.reset();
        out.push_str("        Ok(Err(__smelt_error)) => {\n");
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            // This arm is the real Smelt error channel, so an erased catch
            // binding recovers the thrown payload rather than a rebuilt message
            // record. (The `Err(__smelt_panic)` arm below binds an `unknown`
            // catch variable through `smelt_panic_error_value`, which answers the
            // value a panic-routed throw parked in `SMELT_PANIC_VALUE`, and a
            // class-and-message record only for a panic with no parked value.)
            let value = self.caught_error_value(exception_decl.ty, &RustExpr::path("__smelt_error"))?;
            out.push_str(self.exception_binding_expr(exception_local, &exception_name, &value).as_str());
            self.mark_local_declared(exception_local);
        }
        self.emit_continuation(handler.catch_block, arm_continuation, out)?;
        out.push_str("        }\n");
        __smelt_arm_declared.reset();
        out.push_str("        Err(__smelt_panic) => {\n");
        out.push_str(&format!(
                "            let __smelt_error = {};\n",
                crate::thrown::caught_panic_message_expr("__smelt_panic").delimited()
            ));
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            let value = match self.mir.types.get(exception_decl.ty) {
                Some(Type::String) => RustExpr::chain("JsString::from(__smelt_error.as_str())"),
                Some(Type::PyStr) => RustExpr::path("__smelt_error"),
                // Same exception-payload record as the sibling call terminator;
                // see `runtime/thrown.rs`.
                // Same class-preserving recovery as the sibling call
                // terminator; see `runtime/thrown.rs`.
                Some(Type::Unknown) => {
                    crate::thrown::caught_panic_error_value_expr("__smelt_panic")
                }
                // A panic is an error the runtime raised without a Python class.
                _ if self.is_py_exception_type(exception_decl.ty) => {
                    RustExpr::atom("SmeltPyException::from_message(__smelt_error)")
                }
                _ => self.default_value(exception_decl.ty)?,
            };
            out.push_str(self.exception_binding_expr(exception_local, &exception_name, &value).as_str());
            self.mark_local_declared(exception_local);
        }
        self.emit_continuation(handler.catch_block, arm_continuation, out)?;
        __smelt_arm_declared.close();
        out.push_str("        }\n");
        out.push_str("    }\n");
        if let Some(join) = join.filter(|_| joined_continuation.is_some()) {
            declared_before_arms.close();
            if hoist_dest {
                self.mark_local_declared(dest);
            }
            self.emit_throwing_continuation_tail(join, continuation, out)?;
        } else {
            // No join: the continuation was emitted inside the arms, and the
            // declarations stay as the arms left them.
            declared_before_arms.keep();
        }
        Ok(())
    }

    /// Emits an awaited rejecting future with explicit normal and exception continuations.
    pub(in crate::emitter) fn emit_throwing_await_terminator(
        &self,
        future: &Operand,
        dest: LocalId,
        target: smelt_mir::BlockId,
        handler: smelt_mir::ExceptionHandler,
        continuation: &Continuation<'_>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        // Rejoin the arms at their common tail when the region allows it (see
        // `throwing_join`): each arm stops there and the tail is emitted once.
        let declared_before_arms = self.declared_scope();
        let join = self.throwing_continuation_join(target, handler.catch_block, continuation);
        let fresh_visited = BlockIdSet::default();
        let joined_continuation = join.and_then(|join| match continuation {
            Continuation::Block => Some(Continuation::Region {
                stop: join,
                visited: &fresh_visited,
            }),
            Continuation::Region { visited, .. } => Some(Continuation::Region {
                stop: join,
                visited,
            }),
            Continuation::Closure { active, .. } => Some(Continuation::Closure {
                active,
                stop: Some(join),
                in_loop: true,
            }),
            Continuation::InLoop { .. } => None,
        });
        let arm_continuation = joined_continuation.as_ref().unwrap_or(continuation);
        let joined_tail = join.filter(|_| joined_continuation.is_some());
        let hoist_dest = joined_tail.is_some_and(|tail| !self.is_local_declared(dest) && self.join_tail_reads_local(tail, dest));
        let future_text = self.await_operand_expr(future)?;
        let name = self.local_name(dest)?;
        let dest_type_text = if matches!(self.mir.types.get(local.ty), Some(Type::Future(_) | Type::Function(_))) {
            None
        } else if self.mir.types.get(local.ty) == Some(&Type::None) {
            Some("()".to_owned())
        } else {
            Some(self.rendered_type(local.ty)?.into_string())
        };
        if hoist_dest {
            out.push_str(&self.throwing_dest_predeclaration(dest, dest_type_text.as_deref())?);
            self.mark_local_declared(dest);
        }
        out.push_str(&format!("    match {future_text}.await {{\n", future_text = future_text.tight()));
        out.push_str("        Ok(__smelt_value) => {\n");
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        // The awaited value carries the future's real emitted output type,
        // which may differ from the destination local's type (an erased callee
        // yields `SmeltUnknown`). A `void`/`()` destination discards its result
        // entirely, so no coercion is computed for it.
        let source_ty = self.awaited_output_ty(future)?;
        let value_text = if self.mir.types.get(local.ty) == Some(&Type::None) {
            "{ let _ = __smelt_value; }".to_owned()
        } else {
            self.rendered_at_type(&RustExpr::path("__smelt_value"), source_ty, local.ty, &self.render_scope())?.into_string()
        };
        if hoist_dest {
            out.push_str(&format!("            {name} = {value_text};\n"));
        } else if let Some(type_text) = &dest_type_text {
            out.push_str(&format!("            let {mutability}{name}: {type_text} = {value_text};\n"));
        } else {
            out.push_str(&format!("            let {mutability}{name} = {value_text};\n"));
        }
        self.mark_local_declared(dest);
        let __smelt_arm_declared = self.declared_scope();
        self.emit_continuation(target, arm_continuation, out)?;
        out.push_str("        }\n");
        // Each arm of the emitted `match` re-emits the SAME continuation, and
        // each is its own Rust lexical scope. Restoring the declaration set
        // before the next arm is what makes it declare its own copy of a join
        // temporary instead of assigning to the name the first arm declared --
        // the E0425 in `blocker-logs/top-level-try-await-tail.md`. The same
        // snapshot-per-arm rule the closure emitter already follows.
        __smelt_arm_declared.reset();
        out.push_str("        Err(__smelt_error) => {\n");
        // A rejected future carries the same error channel as a throwing call, so
        // an erased catch binding recovers the rejection's payload here too
        // (`await` on a rejected promise is `throw` in JavaScript).
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            let value = self.caught_error_value(exception_decl.ty, &RustExpr::path("__smelt_error"))?;
            out.push_str(self.exception_binding_expr(exception_local, &exception_name, &value).as_str());
            self.mark_local_declared(exception_local);
        }
        self.emit_continuation(handler.catch_block, arm_continuation, out)?;
        __smelt_arm_declared.close();
        out.push_str("        }\n");
        out.push_str("    }\n");
        if let Some(join) = join.filter(|_| joined_continuation.is_some()) {
            declared_before_arms.close();
            if hoist_dest {
                self.mark_local_declared(dest);
            }
            self.emit_throwing_continuation_tail(join, continuation, out)?;
        } else {
            // No join: the continuation was emitted inside the arms, and the
            // declarations stay as the arms left them.
            declared_before_arms.keep();
        }
        Ok(())
    }

    /// The statement binding a `catch` parameter to the caught `value`.
    ///
    /// A catch parameter that is already declared — predeclared at function
    /// scope because the code after the `try` reads it — is ASSIGNED: the
    /// rejoined continuation (`throwing_join`) runs after the `match`, outside
    /// the arm, so a `let` in the arm would shadow it and the tail would read
    /// the unassigned outer binding. Otherwise it is a fresh `let`.
    fn exception_binding_expr(&self, local: LocalId, name: &str, value: &RustExpr) -> RustExpr {
        if self.is_local_declared(local) {
            RustExpr::loose(format!("            {name} = {value};\n", value = value.delimited()))
        } else {
            RustExpr::loose(format!("            let {name} = {value};\n", value = value.delimited()))
        }
    }
}
