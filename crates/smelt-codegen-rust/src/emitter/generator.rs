//! Generator operations: yielding, resuming, reading a result and `yield*`
//! delegation inside the emitted coroutine body.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Suspend the generator at a `yield` and resume on the command sent back.
    pub(super) fn generator_yield_expr(
        &self,
        value: &Operand,
        unwind: Option<&smelt_mir::ExceptionHandler>,
        cleanup: Option<&smelt_mir::GeneratorCleanup>,
    ) -> Result<RustExpr, EmitError> {
        self.binding_uses.record_early_return();
        let returned = if self.function.is_async || self.function.can_throw {
            "return Ok(smelt_value)"
        } else {
            "return smelt_value"
        };
        let cleanup_text = if let Some(cleanup) = cleanup {
            let declared = self.declared_scope();
            let mut cleanup_text = String::new();
            self.emit_block_until_goto(
                self.block(cleanup.block)?,
                cleanup.after,
                control_flow::RegionExit::Join,
                &mut cleanup_text,
            )?;
            declared.close();
            cleanup_text
        } else {
            String::new()
        };
        let returned = if cleanup.is_some() {
            self.binding_uses.record_early_return();
            let final_return = if self.function.is_async || self.function.can_throw {
                "return Ok(smelt_forced_return)"
            } else {
                "return smelt_forced_return"
            };
            format!("{{ let smelt_forced_return = smelt_value; {cleanup_text} {final_return} }}")
        } else {
            returned.to_owned()
        };
        let thrown = if let Some(handler) = unwind {
            let declared = self.declared_scope();
            let mut catch_text = String::new();
            if let Some(exception_local) = handler.exception_local {
                let exception_name = self.local_name(exception_local)?;
                let exception_decl = self.local_decl(exception_local)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                let exception_value = self.rendered_at_type(
                    &RustExpr::path("smelt_error"),
                    unknown_ty,
                    exception_decl.ty,
                    &self.render_scope(),
                )?;
                catch_text.push_str(&format!(
                    "let {exception_name}: {} = {exception_value};\n",
                    self.rendered_type(exception_decl.ty)?,
                    exception_value = exception_value.delimited()
                ));
                self.mark_local_declared(exception_local);
            }
            self.emit_block(self.block(handler.catch_block)?, &mut catch_text)?;
            declared.close();
            format!("{{ {catch_text} }}")
        } else if cleanup.is_some() {
            format!(
                "{{ {cleanup_text} smelt_panic_throw({}) }}",
                crate::thrown::throw_expr(&RustExpr::path("smelt_error")).delimited()
            )
        } else {
            format!(
                "smelt_panic_throw({})",
                crate::thrown::throw_expr(&RustExpr::path("smelt_error")).delimited()
            )
        };
        // A generator whose declared return type is erased (`unknown`)
        // pins every protocol channel to `SmeltUnknown` (see the
        // erased-slot construction in `closures.rs`), so its yielded
        // values must be erased at the suspension point too.
        let yield_text = if matches!(
            self.mir.types.get(self.function.return_ty),
            Some(Type::Generator { .. })
        ) {
            self.operand_expr(value)?
        } else {
            let unknown_ty = self.type_id(Type::Unknown)?;
            self.value_at_type(value, unknown_ty)?
        };
        Ok(RustExpr::atom(format!(
            "{{ smelt_co.yield_({yield_text}).await; let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(smelt_value) => smelt_value, SmeltGeneratorCommand::Return(smelt_value) => {returned}, SmeltGeneratorCommand::Throw(smelt_error) => {thrown} }} }}",
            yield_text = yield_text.delimited()
        )))
    }

    /// Resume a generator with `next`, `return` or `throw`, calling the
    /// handle's `method` (`resume` or `try_resume`).
    fn generator_next_expr(
        &self,
        generator: &Operand,
        value: Option<&Operand>,
        kind: smelt_hir::GeneratorResumeKind,
        method: &str,
    ) -> Result<RustExpr, EmitError> {
        let raw_value = value
            .map(|value| self.operand_expr(value))
            .transpose()?
            .unwrap_or_else(|| RustExpr::atom("Default::default()"));
        let command = match kind {
            smelt_hir::GeneratorResumeKind::Next => {
                format!(
                    "SmeltGeneratorCommand::Next({raw_value})",
                    raw_value = raw_value.delimited()
                )
            }
            smelt_hir::GeneratorResumeKind::Return => {
                format!(
                    "SmeltGeneratorCommand::Return({raw_value})",
                    raw_value = raw_value.delimited()
                )
            }
            smelt_hir::GeneratorResumeKind::Throw => {
                let thrown = if let Some(value) = value {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    self.value_at_type(value, unknown_ty)?
                } else {
                    self.carrier_expr(self.carrier()?, &RustExpr::undefined())
                };
                format!(
                    "SmeltGeneratorCommand::Throw({thrown})",
                    thrown = thrown.delimited()
                )
            }
        };
        Ok({
            let receiver = self.operand_expr(generator)?;
            receiver.postfix(format!(".{method}({command})"))
        })
    }

    /// A generator resume as the fallible call its terminator makes, with the
    /// result type it answers.
    ///
    /// A synchronous generator's `try_resume` returns its body's throw, so a
    /// `try` around `.next()` catches it. An asynchronous one returns a future
    /// whose `await` already carries the throw, so the resume itself cannot
    /// fail.
    pub(super) fn generator_next_call(
        &self,
        generator: &Operand,
        value: Option<&Operand>,
        kind: smelt_hir::GeneratorResumeKind,
    ) -> Result<(call::RenderedCall, TypeId), EmitError> {
        let Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            ..
        }) = self.mir.types.get(self.operand_ty(generator)?).cloned()
        else {
            return Err(EmitError::new("a generator resume needs a generator"));
        };
        let result_ty = self.type_id(Type::GeneratorResult {
            yield_ty,
            return_ty,
        })?;
        if is_async {
            let resume = self.generator_next_expr(generator, value, kind, "resume")?;
            let future_ty = self.type_id(Type::Future(result_ty))?;
            return Ok((call::RenderedCall::plain(resume), future_ty));
        }
        let resume = self.generator_next_expr(generator, value, kind, "try_resume")?;
        Ok((call::RenderedCall::propagating(resume), result_ty))
    }

    /// Whether a generator step result is the completed one.
    pub(super) fn generator_done_expr(&self, result: &Operand) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "matches!({}, SmeltGeneratorResult::Complete(_))",
            self.operand_expr(result)?.delimited()
        )))
    }

    /// The value a generator step result carries, at the destination type.
    pub(super) fn generator_value_expr(
        &self,
        result: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let result_ty = self.operand_ty(result)?;
        let Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) = self.mir.types.get(result_ty)
        else {
            return Err(EmitError::new(
                "generator value read has non-result operand",
            ));
        };
        let yielded = self.rendered_at_type(
            &RustExpr::path("smelt_value"),
            *yield_ty,
            dest_ty,
            &self.render_scope(),
        )?;
        let returned = self.rendered_at_type(
            &RustExpr::path("smelt_value"),
            *return_ty,
            dest_ty,
            &self.render_scope(),
        )?;
        Ok(RustExpr::atom(format!(
            "match {} {{ SmeltGeneratorResult::Yielded(smelt_value) => {yielded}, SmeltGeneratorResult::Complete(smelt_value) => {returned} }}",
            self.operand_expr(result)?.delimited(),
            yielded = yielded.delimited(),
            returned = returned.delimited()
        )))
    }

    /// `yield*`: forward every step of the delegate, then evaluate to its return.
    pub(super) fn generator_delegate_expr(
        &self,
        generator: &Operand,
        dest_ty: TypeId,
        result_valued: bool,
    ) -> Result<RustExpr, EmitError> {
        // In `result_valued` form the whole delegation is a `Result`: a throw
        // from a delegate generator ends the loop as `Err` instead of
        // unwinding, so a `try` around the `yield*` catches it (see
        // `generator_delegate_call`). Every other arm is `Ok` of its value.
        let ok = |text: String| {
            if result_valued {
                format!("Ok::<_, Box<dyn std::error::Error>>({text})")
            } else {
                text
            }
        };
        let delegate_loop = |receiver: &str,
                             is_async: bool,
                             forwarded: &RustExpr,
                             delegate_command: &str,
                             completed: &RustExpr| {
            if result_valued {
                let resume = if is_async {
                    format!("{receiver}.resume(smelt_delegate_command).await")
                } else {
                    format!("{receiver}.try_resume(smelt_delegate_command)")
                };
                format!(
                    "loop {{ match {resume} {{ Ok(SmeltGeneratorResult::Yielded(smelt_value)) => {{ smelt_co.yield_({forwarded}).await; smelt_delegate_command = {delegate_command}; }}, Ok(SmeltGeneratorResult::Complete(smelt_value)) => break Ok({completed}), Err(smelt_error) => break Err(smelt_error) }} }}",
                    forwarded = forwarded.delimited(),
                    completed = completed.delimited()
                )
            } else {
                let resume = if is_async {
                    format!("{receiver}.resume(smelt_delegate_command).await?")
                } else {
                    format!("{receiver}.resume(smelt_delegate_command)")
                };
                format!(
                    "loop {{ match {resume} {{ SmeltGeneratorResult::Yielded(smelt_value) => {{ smelt_co.yield_({forwarded}).await; smelt_delegate_command = {delegate_command}; }}, SmeltGeneratorResult::Complete(smelt_value) => break {completed} }} }}",
                    forwarded = forwarded.delimited(),
                    completed = completed.delimited()
                )
            }
        };
        let generator_ty = self.operand_ty(generator)?;
        let Some(Type::Generator {
            yield_ty: outer_yield_ty,
            next_ty: outer_next_ty,
            return_ty: outer_return_ty,
            ..
        }) = self.mir.types.get(self.function.return_ty)
        else {
            return Err(EmitError::new("yield* emitted outside generator body"));
        };
        let operand = self.operand_expr(generator)?;
        self.binding_uses.record_early_return();
        let return_command = if self.function.is_async || self.function.can_throw {
            "return Ok(smelt_value)"
        } else {
            "return smelt_value"
        };
        // `SmeltGeneratorCommand::Throw` carries the thrown VALUE (a
        // `SmeltUnknown`), not an error-channel box, so it enters the
        // panic route through `smelt_throw` -- which is also what keeps
        // the payload's class across the unwind.
        let thrown_error = crate::thrown::throw_expr(&RustExpr::path("smelt_error"));
        let consume_outer_sent = format!(
            "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(_) => {{}}, SmeltGeneratorCommand::Return(smelt_value) => {return_command}, SmeltGeneratorCommand::Throw(smelt_error) => smelt_panic_throw({thrown_error}) }} }}",
            thrown_error = thrown_error.delimited()
        );
        match self.mir.types.get(generator_ty) {
            Some(Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                next_ty,
            }) => {
                let forwarded = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *yield_ty,
                    *outer_yield_ty,
                    &self.render_scope(),
                )?;
                let completed = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *return_ty,
                    dest_ty,
                    &self.render_scope(),
                )?;
                let sent = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *outer_next_ty,
                    *next_ty,
                    &self.render_scope(),
                )?;
                let returned_command = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *outer_return_ty,
                    *return_ty,
                    &self.render_scope(),
                )?;
                let delegate_command = format!(
                    "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(smelt_value) => SmeltGeneratorCommand::Next({sent}), SmeltGeneratorCommand::Return(smelt_value) => SmeltGeneratorCommand::Return({returned_command}), SmeltGeneratorCommand::Throw(smelt_error) => SmeltGeneratorCommand::Throw(smelt_error) }} }}",
                    sent = sent.delimited(),
                    returned_command = returned_command.delimited()
                );
                let delegation = delegate_loop(
                    "smelt_delegate",
                    *is_async,
                    &forwarded,
                    &delegate_command,
                    &completed,
                );
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_delegate = {operand}; let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); {delegation} }}",
                    operand = operand.delimited()
                )))
            }
            Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                let forwarded = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *item_ty,
                    *outer_yield_ty,
                    &self.render_scope(),
                )?;
                Ok(RustExpr::atom(ok(format!(
                    "{{ let smelt_iterable = {operand}; for smelt_value in smelt_iterable.clone().into_iter() {{ smelt_co.yield_({forwarded}).await; {consume_outer_sent}; }} }}",
                    operand = operand.delimited(),
                    forwarded = forwarded.delimited()
                ))))
            }
            // A JavaScript string yields its code points, each as a string.
            Some(Type::String) => {
                let forwarded = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    generator_ty,
                    *outer_yield_ty,
                    &self.render_scope(),
                )?;
                Ok(RustExpr::atom(ok(format!(
                    "{{ let smelt_iterable = {operand}; for smelt_value in smelt_iterable.code_point_strings() {{ smelt_co.yield_({forwarded}).await; {consume_outer_sent}; }} }}",
                    operand = operand.delimited(),
                    forwarded = forwarded.delimited()
                ))))
            }
            Some(Type::PyStr) => {
                let forwarded = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    generator_ty,
                    *outer_yield_ty,
                    &self.render_scope(),
                )?;
                Ok(RustExpr::atom(ok(format!(
                    "{{ let smelt_iterable = {operand}; for smelt_char in smelt_iterable.chars() {{ let smelt_value = smelt_char.to_string(); smelt_co.yield_({forwarded}).await; {consume_outer_sent}; }} }}",
                    operand = operand.delimited(),
                    forwarded = forwarded.delimited()
                ))))
            }
            Some(Type::Tuple(items)) => {
                let mut yields = String::new();
                for (index, item_ty) in items.iter().copied().enumerate() {
                    let tuple_value =
                        RustExpr::path(format!("smelt_iterable.{index}")).cloned_value();
                    let forwarded = self.rendered_at_type(
                        &tuple_value,
                        item_ty,
                        *outer_yield_ty,
                        &self.render_scope(),
                    )?;
                    yields.push_str(&format!(
                        "smelt_co.yield_({forwarded}).await; {consume_outer_sent}; ",
                        forwarded = forwarded.delimited()
                    ));
                }
                Ok(RustExpr::atom(ok(format!(
                    "{{ let smelt_iterable = {operand}; {yields}}}",
                    operand = operand.delimited()
                ))))
            }
            Some(Type::Union(members)) => {
                let mut arms = Vec::with_capacity(members.len());
                for (index, member_ty) in members.iter().copied().enumerate() {
                    let body = match self.mir.types.get(member_ty) {
                        Some(Type::Generator {
                            is_async,
                            yield_ty,
                            return_ty,
                            next_ty,
                        }) => {
                            let forwarded = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                *yield_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            let completed = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                *return_ty,
                                dest_ty,
                                &self.render_scope(),
                            )?;
                            let sent = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                *outer_next_ty,
                                *next_ty,
                                &self.render_scope(),
                            )?;
                            let returned_command = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                *outer_return_ty,
                                *return_ty,
                                &self.render_scope(),
                            )?;
                            let delegate_command = format!(
                                "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(smelt_value) => SmeltGeneratorCommand::Next({sent}), SmeltGeneratorCommand::Return(smelt_value) => SmeltGeneratorCommand::Return({returned_command}), SmeltGeneratorCommand::Throw(smelt_error) => SmeltGeneratorCommand::Throw(smelt_error) }} }}",
                                sent = sent.delimited(),
                                returned_command = returned_command.delimited()
                            );
                            let delegation = delegate_loop(
                                "smelt_arm",
                                *is_async,
                                &forwarded,
                                &delegate_command,
                                &completed,
                            );
                            format!(
                                "{{ let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); {delegation} }}"
                            )
                        }
                        Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                            let forwarded = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                *item_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            format!(
                                "{{ for smelt_value in smelt_arm.clone().into_iter() {{ smelt_co.yield_({forwarded}).await; {consume_outer_sent}; }} {default} }}",
                                forwarded = forwarded.delimited(),
                                default = ok("Default::default()".to_owned())
                            )
                        }
                        Some(Type::String | Type::PyStr) => {
                            let forwarded = self.rendered_at_type(
                                &RustExpr::path("smelt_value"),
                                member_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            format!(
                                "{{ for smelt_char in smelt_arm.chars() {{ let smelt_value = smelt_char.to_string(); smelt_co.yield_({forwarded}).await; {consume_outer_sent}; }} {default} }}",
                                forwarded = forwarded.delimited(),
                                default = ok("Default::default()".to_owned())
                            )
                        }
                        _ => {
                            return Err(EmitError::new(
                                "yield* union contains a non-iterable member",
                            ));
                        }
                    };
                    arms.push(format!(
                        "{}::M{index}(smelt_arm) => {{ {body} }}",
                        union::union_name(generator_ty)
                    ));
                }
                Ok(RustExpr::atom(format!(
                    "match {operand} {{ {} }}",
                    arms.join(", "),
                    operand = operand.delimited()
                )))
            }
            _ => Err(EmitError::new("yield* has non-iterable operand")),
        }
    }

    /// `yield* operand` as the fallible call its terminator makes: the
    /// delegation loop as a `Result` (a delegate's throw is its `Err`),
    /// completing at `dest_ty`. Its text awaits, so a `try` matches it in place.
    pub(super) fn generator_delegate_call(
        &self,
        generator: &Operand,
        dest_ty: Option<TypeId>,
    ) -> Result<(call::RenderedCall, TypeId), EmitError> {
        let completion_ty = dest_ty
            .ok_or_else(|| EmitError::new("yield* needs the type its completion is bound at"))?;
        let delegation = self.generator_delegate_expr(generator, completion_ty, true)?;
        Ok((
            call::RenderedCall::propagating_suspending(delegation),
            completion_ty,
        ))
    }

    /// `[...gen]` / `Array.from(gen)` over a synchronous generator as the
    /// fallible call its terminator makes: the yields as a fresh list of the
    /// yield type (the destination's coercion converts the items), or the
    /// body's throw.
    pub(super) fn generator_collect_call(
        &self,
        generator: &Operand,
    ) -> Result<(call::RenderedCall, TypeId), EmitError> {
        let Some(Type::Generator {
            is_async: false,
            yield_ty,
            ..
        }) = self.mir.types.get(self.operand_ty(generator)?).cloned()
        else {
            return Err(EmitError::new(
                "draining a generator needs a synchronous generator",
            ));
        };
        let list_ty = self.type_id(Type::List(yield_ty))?;
        let receiver = self.operand_expr(generator)?;
        Ok((
            call::RenderedCall::propagating(
                receiver.postfix(".try_collect_yields().map(SmeltList::new)"),
            ),
            list_ty,
        ))
    }

    /// The resume closure a synchronous generator's `SmeltGenerator::new`
    /// takes, over the `genawaiter` producer `smelt_generator`.
    ///
    /// It answers the step, or the body's throw as `Err` when `can_throw`, so
    /// `try_resume` delivers the throw to a `try` around the `.next()` that
    /// resumed it.
    pub(super) fn sync_generator_resume_closure(can_throw: bool) -> &'static str {
        if can_throw {
            "move |smelt_value| { *smelt_generator_input.borrow_mut() = Some(smelt_value); match smelt_generator.resume() { genawaiter::GeneratorState::Yielded(smelt_value) => Ok(SmeltGeneratorResult::Yielded(smelt_value)), genawaiter::GeneratorState::Complete(smelt_value) => smelt_value.map(SmeltGeneratorResult::Complete) } }"
        } else {
            "move |smelt_value| { *smelt_generator_input.borrow_mut() = Some(smelt_value); match smelt_generator.resume() { genawaiter::GeneratorState::Yielded(smelt_value) => Ok(SmeltGeneratorResult::Yielded(smelt_value)), genawaiter::GeneratorState::Complete(smelt_value) => Ok(SmeltGeneratorResult::Complete(smelt_value)) } }"
        }
    }
}
