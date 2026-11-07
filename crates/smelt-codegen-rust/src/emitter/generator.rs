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
            "return Ok(value)"
        } else {
            "return value"
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
            format!(
                "{{ let smelt_forced_return = value; {cleanup_text} {final_return} }}"
            )
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
                    &RustExpr::path("error"),
                    unknown_ty,
                    exception_decl.ty,
                    &self.render_scope(),
                )?;
                catch_text.push_str(&format!(
                    "let {exception_name}: {} = {exception_value};\n",
                    self.rendered_type(exception_decl.ty)?
                ));
                self.mark_local_declared(exception_local);
            }
            self.emit_block(self.block(handler.catch_block)?, &mut catch_text)?;
            declared.close();
            format!("{{ {catch_text} }}")
        } else if cleanup.is_some() {
            format!("{{ {cleanup_text} smelt_panic_throw({}) }}", crate::thrown::throw_expr("error"))
        } else {
            format!("smelt_panic_throw({})", crate::thrown::throw_expr("error"))
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
            "{{ co.yield_({yield_text}).await; let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(value) => value, SmeltGeneratorCommand::Return(value) => {returned}, SmeltGeneratorCommand::Throw(error) => {thrown} }} }}"
        )))
    }

    /// Resume a generator with `next`, `return` or `throw`.
    pub(super) fn generator_next_expr(
        &self,
        generator: &Operand,
        value: Option<&Operand>,
        kind: smelt_hir::GeneratorResumeKind,
    ) -> Result<RustExpr, EmitError> {
        let raw_value = value
            .map(|value| self.operand_expr(value))
            .transpose()?
            .unwrap_or_else(|| RustExpr::atom("Default::default()"));
        let command = match kind {
            smelt_hir::GeneratorResumeKind::Next => {
                format!("SmeltGeneratorCommand::Next({raw_value})")
            }
            smelt_hir::GeneratorResumeKind::Return => {
                format!("SmeltGeneratorCommand::Return({raw_value})")
            }
            smelt_hir::GeneratorResumeKind::Throw => {
                let thrown = if let Some(value) = value {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    self.value_at_type(value, unknown_ty)?
                } else {
                    self.carrier_expr(&RustExpr::undefined())
                };
                format!("SmeltGeneratorCommand::Throw({thrown})")
            }
        };
        Ok({ let receiver = self.operand_expr(generator)?; RustExpr::postfix_on(&receiver, format!("{receiver}.resume({command})")) })
    }

    /// Whether a generator step result is the completed one.
    pub(super) fn generator_done_expr(&self, result: &Operand) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "matches!({}, SmeltGeneratorResult::Complete(_))",
            self.operand_expr(result)?
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
            return Err(EmitError::new("generator value read has non-result operand"));
        };
        let yielded = self.rendered_at_type(&RustExpr::path("value"), *yield_ty, dest_ty, &self.render_scope())?;
        let returned = self.rendered_at_type(&RustExpr::path("value"), *return_ty, dest_ty, &self.render_scope())?;
        Ok(RustExpr::atom(format!(
            "match {} {{ SmeltGeneratorResult::Yielded(value) => {yielded}, SmeltGeneratorResult::Complete(value) => {returned} }}",
            self.operand_expr(result)?
        )))
    }

    /// `yield*`: forward every step of the delegate, then evaluate to its return.
    pub(super) fn generator_delegate_expr(
        &self,
        generator: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
            "return Ok(value)"
        } else {
            "return value"
        };
        // `SmeltGeneratorCommand::Throw` carries the thrown VALUE (a
        // `SmeltUnknown`), not an error-channel box, so it enters the
        // panic route through `smelt_throw` -- which is also what keeps
        // the payload's class across the unwind.
        let thrown_error = crate::thrown::throw_expr("error");
        let consume_outer_sent = format!(
            "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(_) => {{}}, SmeltGeneratorCommand::Return(value) => {return_command}, SmeltGeneratorCommand::Throw(error) => smelt_panic_throw({thrown_error}) }} }}"
        );
        match self.mir.types.get(generator_ty) {
            Some(Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                next_ty,
            }) => {
                let forwarded =
                    self.rendered_at_type(&RustExpr::path("value"), *yield_ty, *outer_yield_ty, &self.render_scope())?;
                let completed = self.rendered_at_type(&RustExpr::path("value"), *return_ty, dest_ty, &self.render_scope())?;
                let sent = self.rendered_at_type(
                    &RustExpr::path("value"),
                    *outer_next_ty,
                    *next_ty,
                    &self.render_scope(),
                )?;
                let returned_command = self.rendered_at_type(
                    &RustExpr::path("value"),
                    *outer_return_ty,
                    *return_ty,
                    &self.render_scope(),
                )?;
                let delegate_command = format!(
                    "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(value) => SmeltGeneratorCommand::Next({sent}), SmeltGeneratorCommand::Return(value) => SmeltGeneratorCommand::Return({returned_command}), SmeltGeneratorCommand::Throw(error) => SmeltGeneratorCommand::Throw(error) }} }}"
                );
                let resume = if *is_async {
                    "smelt_delegate.resume(smelt_delegate_command).await?"
                } else {
                    "smelt_delegate.resume(smelt_delegate_command)"
                };
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_delegate = {operand}; let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); loop {{ match {resume} {{ SmeltGeneratorResult::Yielded(value) => {{ co.yield_({forwarded}).await; smelt_delegate_command = {delegate_command}; }}, SmeltGeneratorResult::Complete(value) => break {completed} }} }} }}"
                )))
            }
            Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                let forwarded =
                    self.rendered_at_type(&RustExpr::path("value"), *item_ty, *outer_yield_ty, &self.render_scope())?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_iterable = {operand}; for value in smelt_iterable.clone().into_iter() {{ co.yield_({forwarded}).await; {consume_outer_sent}; }} }}"
                )))
            }
            Some(Type::String | Type::PyStr) => {
                let forwarded =
                    self.rendered_at_type(&RustExpr::path("value"), generator_ty, *outer_yield_ty, &self.render_scope())?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_iterable = {operand}; for smelt_char in smelt_iterable.chars() {{ let value = smelt_char.to_string(); co.yield_({forwarded}).await; {consume_outer_sent}; }} }}"
                )))
            }
            Some(Type::Tuple(items)) => {
                let mut yields = String::new();
                for (index, item_ty) in items.iter().copied().enumerate() {
                    let tuple_value = RustExpr::chain(format!("smelt_iterable.{index}.clone()")).ending_in_clone();
                    let forwarded =
                        self.rendered_at_type(&tuple_value, item_ty, *outer_yield_ty, &self.render_scope())?;
                    yields.push_str(&format!("co.yield_({forwarded}).await; {consume_outer_sent}; "));
                }
                Ok(RustExpr::atom(format!("{{ let smelt_iterable = {operand}; {yields}}}")))
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
                                &RustExpr::path("value"),
                                *yield_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            let completed = self.rendered_at_type(
                                &RustExpr::path("value"),
                                *return_ty,
                                dest_ty,
                                &self.render_scope(),
                            )?;
                            let sent = self.rendered_at_type(
                                &RustExpr::path("value"),
                                *outer_next_ty,
                                *next_ty,
                                &self.render_scope(),
                            )?;
                            let returned_command = self.rendered_at_type(
                                &RustExpr::path("value"),
                                *outer_return_ty,
                                *return_ty,
                                &self.render_scope(),
                            )?;
                            let delegate_command = format!(
                                "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(value) => SmeltGeneratorCommand::Next({sent}), SmeltGeneratorCommand::Return(value) => SmeltGeneratorCommand::Return({returned_command}), SmeltGeneratorCommand::Throw(error) => SmeltGeneratorCommand::Throw(error) }} }}"
                            );
                            let resume = if *is_async {
                                "smelt_arm.resume(smelt_delegate_command).await?"
                            } else {
                                "smelt_arm.resume(smelt_delegate_command)"
                            };
                            format!(
                                "{{ let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); loop {{ match {resume} {{ SmeltGeneratorResult::Yielded(value) => {{ co.yield_({forwarded}).await; smelt_delegate_command = {delegate_command}; }}, SmeltGeneratorResult::Complete(value) => break {completed} }} }} }}"
                            )
                        }
                        Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                            let forwarded = self.rendered_at_type(
                                &RustExpr::path("value"),
                                *item_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            format!(
                                "{{ for value in smelt_arm.clone().into_iter() {{ co.yield_({forwarded}).await; {consume_outer_sent}; }} Default::default() }}"
                            )
                        }
                        Some(Type::String | Type::PyStr) => {
                            let forwarded = self.rendered_at_type(
                                &RustExpr::path("value"),
                                member_ty,
                                *outer_yield_ty,
                                &self.render_scope(),
                            )?;
                            format!(
                                "{{ for smelt_char in smelt_arm.chars() {{ let value = smelt_char.to_string(); co.yield_({forwarded}).await; {consume_outer_sent}; }} Default::default() }}"
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
                Ok(RustExpr::atom(format!("match {operand} {{ {} }}", arms.join(", "))))
            }
            _ => Err(EmitError::new("yield* has non-iterable operand")),
        }
    }
}
