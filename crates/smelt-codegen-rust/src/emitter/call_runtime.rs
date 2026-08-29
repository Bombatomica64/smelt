//! Call Runtime emission helpers.

use super::*;
use crate::emitter::rendered_text_rewrite::cloned_value_text;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {

    /// Render a call whose callee is a callable-object record, through the
    /// synthetic `__smelt_call` slot that holds its underlying callable.
    ///
    /// A callable object (a TypeScript interface with a call signature, or the
    /// `type`/intersection/type-literal spellings that lower to the same
    /// interface) is emitted as a struct, so it is not a Rust callable and the
    /// ordinary indirect-call ladders do not apply to it. The frontend leaves
    /// such a call as a plain `closure_call` on the record — the MIR-level
    /// instruction for "invoke this callable object" — and this is where that
    /// instruction is implemented: read the slot and invoke it with the slot's
    /// own ABI, which is `SmeltErasedFunction::call(vec![..])` for the erased
    /// rest shape and a direct invocation for a concrete signature.
    ///
    /// Answers `None` for any callee that is not a callable-object record, so
    /// callers keep their existing behaviour for every other shape.
    pub(super) fn callable_object_slot_call_text(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let callee_ty = self.operand_ty(callee)?;
        let Some(call_ty) = self.callable_interface_call_field_ty(callee_ty) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(call_ty).cloned() else {
            return Ok(None);
        };
        let slot_text = format!("{}.__smelt_call.clone()", self.operand_text(callee)?);
        let (call_text, source_ty) =
            if self.is_erased_unknown_rest_function(&function) && !function.may_throw {
                // The slot is a `SmeltErasedFunction`: its ABI takes the argument
                // vector and always answers a bare `SmeltUnknown`.
                let rendered = args
                    .iter()
                    .map(|arg| self.erase(arg))
                    .collect::<Result<Vec<_>, EmitError>>()?;
                (
                    format!("{slot_text}.call(vec![{}])", rendered.join(", ")),
                    self.type_id(Type::Unknown)?,
                )
            } else {
                let rendered = args
                    .iter()
                    .zip(function.params.iter())
                    .map(|(arg, param)| self.value_at_type(arg, *param))
                    .collect::<Result<Vec<_>, EmitError>>()?;
                let call = format!("({slot_text})({})", rendered.join(", "));
                let call = if function.may_throw {
                    if self.body_can_propagate_error() {
                        format!("{call}?")
                    } else {
                        format!("{call}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
                    }
                } else {
                    call
                };
                (call, function.return_ty)
            };
        if self.mir.types.get(dest_ty) == Some(&Type::None) {
            return Ok(Some(format!("{{ {call_text}; () }}")));
        }
        Ok(Some(self.value_at_type_text(&call_text, source_ty, dest_ty)?))
    }
    /// Render array-literal elements by value without consuming local operands.
    ///
    /// JavaScript array literals copy references/primitive values into the new
    /// array and do not invalidate the source expression. MIR may mark an input
    /// as `Move` when the temporary is otherwise dead, but generated Rust can
    /// still need the same closure parameter later in the literal expression, so
    /// list emission treats local moves as cloneable copies.
    fn list_literal_operand(&self, operand: &Operand) -> Operand {
        match operand {
            Operand::Move(place @ Place::Local(_)) => Operand::Copy(place.clone()),
            _ => operand.clone(),
        }
    }

    /// Render the runtime dispatch snippet for invoking a dynamically-typed
    /// callable (`SmeltUnknown`).
    ///
    /// When the callee is erased to `SmeltUnknown` (source `unknown`, a union, or
    /// an erased class), the concrete callable shape is only known at runtime: it
    /// may be a `SmeltUnknown::Function`, or an object that exposes a
    /// `__smelt_call` function member (a callable object). This snippet clones the
    /// callee value, coerces the argument expression into a `Vec<SmeltUnknown>`,
    /// extracts whichever callable form is present, invokes it (propagating a
    /// thrown error via `panic!`), and falls back to `SmeltUnknown::Null` when the
    /// value is not callable.
    ///
    /// `callee_text` is the expression producing the callee value. `args_expr` is
    /// the expression fed to `Into::into` to build the argument vector; callers
    /// differ only in how they materialize that expression (an explicit
    /// `vec![...]` for `ClosureCall`, a pre-flattened list for
    /// `ClosureCallSpread`), so it is the single parameterized hole in the shared
    /// snippet.
    fn dynamic_callable_dispatch_text(&self, callee_text: &str, args_expr: &str) -> String {
        // `callee_text` is usually already an owned temporary (an operand render
        // clones the local it reads), so take an owned copy rather than
        // deep-copying it a second time.
        let callee_text = &cloned_value_text(callee_text);
        format!(
            "{{ let smelt_function_value = {callee_text}; let smelt_call_args: Vec<SmeltUnknown> = Into::into({args_expr}); let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (smelt_function)(smelt_call_args).unwrap_or_else(|error| panic!(\"{{}}\", error)) }} else {{ SmeltUnknown::Null }} }}"
        )
    }

    /// Render a dictionary literal as a generated record when the destination
    /// type is a known class/interface storage shape.
    ///
    /// TypeScript object literals are structural, but generated Rust records
    /// are nominal. This path uses the destination type already known at the
    /// assignment/call site to construct the nominal Rust record directly
    /// instead of first materializing an unrelated `HashMap`.
    fn record_literal_text_for_dest(
        &self,
        entries: &[(Operand, Operand)],
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        if !self.is_interface_record_type(dest_ty)
            && self.mir.classes.iter().all(|class| class.name != *name)
        {
            return Ok(None);
        }
        let Some(fields) = self.structural_record_fields(dest_ty) else {
            return Ok(None);
        };
        if fields.is_empty() {
            return Ok(None);
        }

        let mut literal_entries = HashMap::new();
        for (entry_key, value) in entries {
            let Operand::Const(Constant::String(key_text)) = entry_key else {
                return Ok(None);
            };
            literal_entries.insert(sanitize_ident(key_text), value);
        }

        let mut field_text = Vec::new();
        for field in fields {
            let field_name = sanitize_ident(self.symbol_name(field.name)?);
            let value = if let Some(entry_value) = literal_entries.get(&field_name) {
                self.value_at_type(entry_value, field.ty)?
            } else if matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                self.default_value(field.ty)?
            } else {
                return Ok(None);
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }

        let type_name = sanitize_ident(self.symbol_name(*name)?);
        Ok(Some(format!("{type_name} {{ {} }}", field_text.join(", "))))
    }

    /// Converts an rvalue to its Rust text representation.
    pub(super) fn rvalue_text(&self, value: &Rvalue) -> Result<String, EmitError> {
        self.rvalue_text_for_dest(value, self.none_ty)
    }

    /// Converts an rvalue to Rust text using the destination type when it affects emission.
    /// Converts an rvalue to Rust text using the destination type when it affects emission.
    pub(super) fn rvalue_text_for_dest(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let text = self.rvalue_text_for_dest_inner(value, dest_ty)?;
        // An rvalue that emits a bare `SmeltUnknown` constructor (`SmeltUnknown::
        // Number(..)` from arithmetic, a boxed literal) but is assigned to a
        // concrete-union destination is reconstructed into the tagged union.
        // Guarding on the literal `SmeltUnknown::` prefix keeps values that
        // already produced the tagged union (`SmeltUnion…::M0(..)`, a `Use` of a
        // union local) untouched, avoiding a double wrap.
        if text.starts_with("SmeltUnknown::") && self.concrete_union_members(dest_ty).is_some() {
            return Ok(format!(
                "{}::from_smelt_unknown({text})",
                union::union_name(dest_ty)
            ));
        }
        // List-producing operations (concat/slice/flat/map/copy/…) emit a bare
        // `Vec`, but `Type::List` now lowers to the identity-bearing `SmeltList`.
        // Coerce through `Into` at this single choke point: it is a no-op when the
        // rvalue already produced a `SmeltList` (the blanket `From<T> for T`) and
        // wraps a fresh-identity list otherwise (`From<Vec<T>>`).
        if matches!(self.mir.types.get(dest_ty), Some(Type::List(_))) {
            // `Default::default()` is an ambiguous `Into` source (both Vec and
            // SmeltList satisfy it); emit the SmeltList default directly.
            if text == "Default::default()" {
                return Ok("SmeltList::default()".to_owned());
            }
            return Ok(format!("Into::<SmeltList<_>>::into({text})"));
        }
        Ok(text)
    }

    /// Render an rvalue at a destination type, before list-identity coercion.
    ///
    /// This is the dispatch body behind [`Self::rvalue_text_for_dest`]: it
    /// matches every `Rvalue` variant and produces the raw Rust expression for
    /// it. The public wrapper then applies the single `SmeltList` `Into`
    /// choke-point coercion for list-typed destinations, so variants here may
    /// freely emit bare `Vec` expressions.
    fn rvalue_text_for_dest_inner(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match value {
            Rvalue::Use(operand)
                if matches!(
                    self.mir.types.get(self.operand_ty(operand)?),
                    Some(Type::Optional(inner))
                        if *inner == dest_ty
                            && !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
                ) =>
            {
                Ok(format!(
                    "{}.clone().expect(\"optional value was absent after narrowing\")",
                    self.operand_text(operand)?
                ))
            }
            Rvalue::Use(operand) => self.value_at_type(operand, dest_ty),
            Rvalue::GeneratorYield {
                value,
                unwind,
                cleanup,
            } => {
                let returned = if self.function.is_async || self.function.can_throw {
                    "return Ok(value)"
                } else {
                    "return value"
                };
                let cleanup_text = if let Some(cleanup) = cleanup {
                    let declared = self.declared_locals_snapshot();
                    let mut cleanup_text = String::new();
                    self.emit_block_until_goto(
                        self.block(cleanup.block)?,
                        cleanup.after,
                        control_flow::RegionExit::Join,
                        &mut cleanup_text,
                    )?;
                    self.restore_declared_locals(declared);
                    cleanup_text
                } else {
                    String::new()
                };
                let returned = if cleanup.is_some() {
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
                    let declared = self.declared_locals_snapshot();
                    let mut catch_text = String::new();
                    if let Some(exception_local) = handler.exception_local {
                        let exception_name = self.local_name(exception_local)?;
                        let exception_decl = self.local_decl(exception_local)?;
                        let unknown_ty = self.type_id(Type::Unknown)?;
                        let exception_value = self.value_at_type_text(
                            "error",
                            unknown_ty,
                            exception_decl.ty,
                        )?;
                        catch_text.push_str(&format!(
                            "let {exception_name}: {} = {exception_value};\n",
                            self.type_text_with_impl_trait(exception_decl.ty, false)?
                        ));
                        self.mark_local_declared(exception_local);
                    }
                    self.emit_block(self.block(handler.catch_block)?, &mut catch_text)?;
                    self.restore_declared_locals(declared);
                    format!("{{ {catch_text} }}")
                } else if cleanup.is_some() {
                    format!("{{ {cleanup_text} panic!(\"{{}}\", error) }}")
                } else {
                    "panic!(\"{}\", error)".to_owned()
                };
                // A generator whose declared return type is erased (`unknown`)
                // pins every protocol channel to `SmeltUnknown` (see the
                // erased-slot construction in `closures.rs`), so its yielded
                // values must be erased at the suspension point too.
                let yield_text = if matches!(
                    self.mir.types.get(self.function.return_ty),
                    Some(Type::Generator { .. })
                ) {
                    self.operand_text(value)?
                } else {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    self.value_at_type(value, unknown_ty)?
                };
                Ok(format!(
                    "{{ co.yield_({yield_text}).await; let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(value) => value, SmeltGeneratorCommand::Return(value) => {returned}, SmeltGeneratorCommand::Throw(error) => {thrown} }} }}"
                ))
            }
            Rvalue::GeneratorNext {
                generator,
                value,
                kind,
            } => {
                let raw_value = value
                    .as_ref()
                    .map(|value| self.operand_text(value))
                    .transpose()?
                    .unwrap_or_else(|| "Default::default()".to_owned());
                let command = match kind {
                    smelt_mir::GeneratorResumeKind::Next => {
                        format!("SmeltGeneratorCommand::Next({raw_value})")
                    }
                    smelt_mir::GeneratorResumeKind::Return => {
                        format!("SmeltGeneratorCommand::Return({raw_value})")
                    }
                    smelt_mir::GeneratorResumeKind::Throw => {
                        let thrown = if let Some(value) = value {
                            let unknown_ty = self.type_id(Type::Unknown)?;
                            self.value_at_type(value, unknown_ty)?
                        } else {
                            "SmeltUnknown::Undefined".to_owned()
                        };
                        format!("SmeltGeneratorCommand::Throw({thrown})")
                    }
                };
                Ok(format!("{}.resume({command})", self.operand_text(generator)?))
            }
            Rvalue::GeneratorDone { result } => Ok(format!(
                "matches!({}, SmeltGeneratorResult::Complete(_))",
                self.operand_text(result)?
            )),
            Rvalue::GeneratorValue { result } => {
                let result_ty = self.operand_ty(result)?;
                let Some(Type::GeneratorResult {
                    yield_ty,
                    return_ty,
                }) = self.mir.types.get(result_ty)
                else {
                    return Err(EmitError::new("generator value read has non-result operand"));
                };
                let yielded = self.value_at_type_text("value", *yield_ty, dest_ty)?;
                let returned = self.value_at_type_text("value", *return_ty, dest_ty)?;
                Ok(format!(
                    "match {} {{ SmeltGeneratorResult::Yielded(value) => {yielded}, SmeltGeneratorResult::Complete(value) => {returned} }}",
                    self.operand_text(result)?
                ))
            }
            Rvalue::GeneratorDelegate { generator } => {
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
                let operand = self.operand_text(generator)?;
                let return_command = if self.function.is_async || self.function.can_throw {
                    "return Ok(value)"
                } else {
                    "return value"
                };
                let consume_outer_sent = format!(
                    "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(_) => {{}}, SmeltGeneratorCommand::Return(value) => {return_command}, SmeltGeneratorCommand::Throw(error) => panic!(\"{{}}\", error) }} }}"
                );
                match self.mir.types.get(generator_ty) {
                    Some(Type::Generator {
                        is_async,
                        yield_ty,
                        return_ty,
                        next_ty,
                    }) => {
                        let forwarded =
                            self.value_at_type_text("value", *yield_ty, *outer_yield_ty)?;
                        let completed = self.value_at_type_text("value", *return_ty, dest_ty)?;
                        let sent = self.value_at_type_text(
                            "value",
                            *outer_next_ty,
                            *next_ty,
                        )?;
                        let returned_command = self.value_at_type_text(
                            "value",
                            *outer_return_ty,
                            *return_ty,
                        )?;
                        let delegate_command = format!(
                            "{{ let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command {{ SmeltGeneratorCommand::Next(value) => SmeltGeneratorCommand::Next({sent}), SmeltGeneratorCommand::Return(value) => SmeltGeneratorCommand::Return({returned_command}), SmeltGeneratorCommand::Throw(error) => SmeltGeneratorCommand::Throw(error) }} }}"
                        );
                        let resume = if *is_async {
                            "smelt_delegate.resume(smelt_delegate_command).await?"
                        } else {
                            "smelt_delegate.resume(smelt_delegate_command)"
                        };
                        Ok(format!(
                            "{{ let smelt_delegate = {operand}; let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); loop {{ match {resume} {{ SmeltGeneratorResult::Yielded(value) => {{ co.yield_({forwarded}).await; smelt_delegate_command = {delegate_command}; }}, SmeltGeneratorResult::Complete(value) => break {completed} }} }} }}"
                        ))
                    }
                    Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                        let forwarded =
                            self.value_at_type_text("value", *item_ty, *outer_yield_ty)?;
                        Ok(format!(
                            "{{ let smelt_iterable = {operand}; for value in smelt_iterable.clone().into_iter() {{ co.yield_({forwarded}).await; {consume_outer_sent}; }} }}"
                        ))
                    }
                    Some(Type::String) => {
                        let forwarded =
                            self.value_at_type_text("value", generator_ty, *outer_yield_ty)?;
                        Ok(format!(
                            "{{ let smelt_iterable = {operand}; for smelt_char in smelt_iterable.chars() {{ let value = smelt_char.to_string(); co.yield_({forwarded}).await; {consume_outer_sent}; }} }}"
                        ))
                    }
                    Some(Type::Tuple(items)) => {
                        let mut yields = String::new();
                        for (index, item_ty) in items.iter().copied().enumerate() {
                            let tuple_value = format!("smelt_iterable.{index}.clone()");
                            let forwarded =
                                self.value_at_type_text(&tuple_value, item_ty, *outer_yield_ty)?;
                            yields.push_str(&format!("co.yield_({forwarded}).await; {consume_outer_sent}; "));
                        }
                        Ok(format!("{{ let smelt_iterable = {operand}; {yields}}}"))
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
                                    let forwarded = self.value_at_type_text(
                                        "value",
                                        *yield_ty,
                                        *outer_yield_ty,
                                    )?;
                                    let completed = self.value_at_type_text(
                                        "value",
                                        *return_ty,
                                        dest_ty,
                                    )?;
                                    let sent = self.value_at_type_text(
                                        "value",
                                        *outer_next_ty,
                                        *next_ty,
                                    )?;
                                    let returned_command = self.value_at_type_text(
                                        "value",
                                        *outer_return_ty,
                                        *return_ty,
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
                                    let forwarded = self.value_at_type_text(
                                        "value",
                                        *item_ty,
                                        *outer_yield_ty,
                                    )?;
                                    format!(
                                        "{{ for value in smelt_arm.clone().into_iter() {{ co.yield_({forwarded}).await; {consume_outer_sent}; }} Default::default() }}"
                                    )
                                }
                                Some(Type::String) => {
                                    let forwarded = self.value_at_type_text(
                                        "value",
                                        member_ty,
                                        *outer_yield_ty,
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
                        Ok(format!("match {operand} {{ {} }}", arms.join(", ")))
                    }
                    _ => Err(EmitError::new("yield* has non-iterable operand")),
                }
            }
            Rvalue::List(items) => {
                // A contextual tuple assertion keeps the literal as a `List`
                // rvalue while changing its destination storage to a Rust tuple.
                // Render its heterogeneous elements directly into that tuple.
                if let Some(Type::Tuple(target_items)) = self.mir.types.get(dest_ty)
                    && target_items.len() == items.len()
                {
                    let items_text = items
                        .iter()
                        .zip(target_items)
                        .map(|(item, target_ty)| {
                            self.value_at_type(&self.list_literal_operand(item), *target_ty)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return if target_items.len() == 1 {
                        Ok(format!("({items_text},)"))
                    } else {
                        Ok(format!("({items_text})"))
                    };
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
                    if matches!(
                        self.mir.types.get(*inner),
                        Some(
                            Type::List(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
                        )
                    ) || self.is_erased_class_type(*inner)
                    {
                        let inner_text = self.rvalue_text_for_dest(value, *inner)?;
                        return Ok(format!("Some({inner_text})"));
                    }
                }
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let items_text = items
                        .iter()
                        .map(|item| self.erase(&self.list_literal_operand(item)))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(self.erase_array_text(&items_text));
                }
                if let Some(&Type::List(item_ty)) = self.mir.types.get(dest_ty) {
                    let items_text = items
                        .iter()
                        .map(|item| self.value_at_type(&self.list_literal_operand(item), item_ty))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    // Annotate the backing Vec with the element type so heterogeneous
                    // closures coerce to the shared `Rc<dyn Fn>` element (the dest
                    // annotation used to drive this before `Type::List` became SmeltList).
                    let elem_text = self.type_text_with_impl_trait(item_ty, false)?;
                    return Ok(format!(
                        "SmeltList::from({{ let smelt_list_items: Vec<{elem_text}> = vec![{items_text}]; smelt_list_items }})"
                    ));
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(&self.list_literal_operand(item)))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            Rvalue::Set(items) => {
                let set_uses_js_set = matches!(self.mir.types.get(dest_ty), Some(Type::Set(item)) if !self.type_is_hash_set_key_safe(*item));
                if items.is_empty() {
                    return Ok(if set_uses_js_set {
                        "SmeltJsSet::new()".to_owned()
                    } else {
                        "::std::collections::HashSet::new()".to_owned()
                    });
                }
                let item_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::Set(item_ty)) => Some(*item_ty),
                    _ => None,
                };
                let items_text = items
                    .iter()
                    .map(|item| {
                        if let Some(set_item_ty) = item_ty {
                            self.value_at_type(item, set_item_ty)
                        } else {
                            self.operand_text(item)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if set_uses_js_set {
                    return Ok(format!("SmeltJsSet::from([{items_text}])"));
                }
                Ok(format!("::std::collections::HashSet::from([{items_text}])"))
            }
            Rvalue::Dict(entries) => {
                if let Some(record_text) = self.record_literal_text_for_dest(entries, dest_ty)? {
                    return Ok(record_text);
                }
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let entries_text = entries
                        .iter()
                        .map(|(key, entry_value)| {
                            Ok(format!(
                                "({}, {})",
                                self.value_at_type(key, self.type_id(Type::String)?)?,
                                self.erase(entry_value)?
                            ))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    return Ok(self.erase_object_text(&entries_text));
                }
                // A source-spelled `Map` dest and a plain `Dict`/record dest share
                // the same `Rvalue::Dict` literal; `dest_is_js_map` selects the
                // `SmeltJsMap` backing (whose erasure stamps `__smelt_map`) for
                // the former, even when string-keyed.
                let dest_is_js_map = matches!(self.mir.types.get(dest_ty), Some(Type::JsMap(_, _)));
                let dict_types = match self.mir.types.get(dest_ty) {
                    Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) => {
                        Some((*key_ty, *value_ty))
                    }
                    _ => None,
                };
                let entries_text = entries
                    .iter()
                    .map(|(key, entry_value)| {
                        let key_text = if let Some((key_ty, _)) = dict_types {
                            if self.mir.types.get(key_ty) == Some(&Type::String)
                                && self.mir.types.get(self.operand_ty(key)?) != Some(&Type::String)
                            {
                                self.property_key_to_string_text(
                                    &self.operand_text(key)?,
                                    self.operand_ty(key)?,
                                )?
                            } else {
                                self.value_at_type(key, key_ty)?
                            }
                        } else {
                            self.operand_text(key)?
                        };
                        let value_text = if let Some((_, value_ty)) = dict_types {
                            if matches!(self.mir.types.get(value_ty), Some(Type::Function(_))) {
                                format!(
                                    "{{ let smelt_fn: {} = {}; smelt_fn }}",
                                    self.type_text_with_impl_trait(value_ty, false)?,
                                    self.value_at_type(entry_value, value_ty)?
                                )
                            } else {
                                self.value_at_type(entry_value, value_ty)?
                            }
                        } else {
                            self.operand_text(entry_value)?
                        };
                        Ok(format!("({key_text}, {value_text})"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                if dest_is_js_map {
                    return Ok(format!("SmeltJsMap::from([{entries_text}])"));
                }
                if let Some((key_ty, _)) = dict_types {
                    if self.dict_uses_smelt_record(key_ty) {
                        return Ok(format!("SmeltRecord::from([{entries_text}])"));
                    }
                    if self.dict_uses_js_key_map(key_ty) {
                        return Ok(format!("SmeltJsMap::from([{entries_text}])"));
                    }
                }
                Ok(format!(
                    "::std::collections::HashMap::from([{entries_text}])"
                ))
            }
            Rvalue::Tuple(items) => {
                if let Some(Type::Tuple(target_items)) = self.mir.types.get(dest_ty) {
                    let offset = items.len().saturating_sub(target_items.len());
                    let items_text = target_items
                        .iter()
                        .enumerate()
                        .map(|(index, target_item)| {
                            let item_index = index
                                .checked_add(offset)
                                .ok_or_else(|| EmitError::new("tuple item index overflowed"))?;
                            let item = items.get(item_index).ok_or_else(|| {
                                EmitError::new("tuple destination has more items than literal")
                            })?;
                            self.value_at_type(item, *target_item)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return if target_items.len() == 1 {
                        Ok(format!("({items_text},)"))
                    } else {
                        Ok(format!("({items_text})"))
                    };
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if items.is_empty() {
                    Ok("()".to_owned())
                } else if items.len() == 1 {
                    Ok(format!("({items_text},)"))
                } else {
                    Ok(format!("({items_text})"))
                }
            }
            Rvalue::Binary { op, lhs, rhs } => {
                if let Some(text) = self.optional_binary_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if let Some(text) = self.unknown_binary_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.strict_identity_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if *op == smelt_hir::BinOp::Add
                    && matches!(self.mir.types.get(dest_ty), Some(Type::String))
                {
                    let lhs_text = self.string_like_operand_text(lhs, "string addition")?;
                    let rhs_text = self.string_like_operand_text(rhs, "string addition")?;
                    return Ok(format!("{lhs_text} + &{rhs_text}"));
                }
                if let Some(text) = self.erased_arithmetic_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if let Some(text) = self.function_equality_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.nested_float_equality_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.heterogeneous_equality_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.mixed_string_number_relational_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.bare_erased_relational_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.numeric_comparison_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if matches!(
                    *op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(self.mir.types.get(dest_ty), Some(Type::Int | Type::Float))
                {
                    return Ok(format!(
                        "{} {} {}",
                        self.value_at_type(lhs, dest_ty)?,
                        smelt_hir::bin_op_text(*op),
                        self.value_at_type(rhs, dest_ty)?
                    ));
                }
                if matches!(*op, smelt_hir::BinOp::And | smelt_hir::BinOp::Or)
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Bool))
                {
                    return Ok(format!(
                        "{} {} {}",
                        self.truthy_operand_text(lhs)?,
                        smelt_hir::bin_op_text(*op),
                        self.truthy_operand_text(rhs)?
                    ));
                }
                if *op == smelt_hir::BinOp::UShr {
                    let lhs_text = self.operand_text(lhs)?;
                    let rhs_text = self.operand_text(rhs)?;
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    return Ok(format!(
                        "{{ let smelt_shift_value = {lhs_trunc_text}; let smelt_shift_value = if smelt_shift_value.is_finite() {{ smelt_shift_value.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; let smelt_shift_count = {rhs_trunc_text}; let smelt_shift_count = if smelt_shift_count.is_finite() {{ smelt_shift_count.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; (smelt_shift_value >> (smelt_shift_count & 31)) as f64 }}"
                    ));
                }
                if matches!(*op, smelt_hir::BinOp::Shl | smelt_hir::BinOp::Shr) {
                    let lhs_text = self.operand_text(lhs)?;
                    let rhs_text = self.operand_text(rhs)?;
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    let op_text = smelt_hir::bin_op_text(*op);
                    let result_cast = if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                        "i64"
                    } else {
                        "f64"
                    };
                    return Ok(format!(
                        "(({lhs_trunc_text} as i128) {op_text} (({rhs_trunc_text} as u32).min(127))) as {result_cast}"
                    ));
                }
                if matches!(
                    *op,
                    smelt_hir::BinOp::BitAnd | smelt_hir::BinOp::BitOr | smelt_hir::BinOp::BitXor
                ) {
                    // JavaScript bitwise `&`, `|`, `^` coerce both operands with
                    // `ToInt32` (truncate to integer, take modulo 2^32 as a signed
                    // 32-bit value), operate, and yield a signed 32-bit number.
                    let lhs_text = self.operand_text(lhs)?;
                    let rhs_text = self.operand_text(rhs)?;
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    let op_text = smelt_hir::bin_op_text(*op);
                    let result_cast = if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                        "i64"
                    } else {
                        "f64"
                    };
                    return Ok(format!(
                        "({{ let smelt_bit_lhs = {{ let smelt_bit_v = {lhs_trunc_text}; if smelt_bit_v.is_finite() {{ smelt_bit_v.trunc().rem_euclid(4294967296.0) as u32 as i32 }} else {{ 0_i32 }} }}; let smelt_bit_rhs = {{ let smelt_bit_v = {rhs_trunc_text}; if smelt_bit_v.is_finite() {{ smelt_bit_v.trunc().rem_euclid(4294967296.0) as u32 as i32 }} else {{ 0_i32 }} }}; (smelt_bit_lhs {op_text} smelt_bit_rhs) as {result_cast} }})"
                    ));
                }
                if *op == smelt_hir::BinOp::Add
                    && matches!(
                        self.mir.types.get(self.operand_ty(lhs)?),
                        Some(Type::String)
                    )
                {
                    let rhs_text = self.operand_text(rhs)?;
                    let rhs_expr = match self.mir.types.get(self.operand_ty(rhs)?) {
                        Some(Type::String) => format!("&{rhs_text}"),
                        Some(Type::Optional(inner))
                            if matches!(
                                self.mir.types.get(*inner),
                                Some(Type::Bool | Type::Int | Type::Float | Type::String)
                            ) =>
                        {
                            format!("&{rhs_text}.unwrap_or_default().to_string()")
                        }
                        _ => format!("&{rhs_text}.to_string()"),
                    };
                    return Ok(format!("{} + {rhs_expr}", self.operand_text(lhs)?));
                }
                if matches!(*op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
                    let lhs_ty = self.operand_ty(lhs)?;
                    let rhs_ty = self.operand_ty(rhs)?;
                    if lhs_ty != rhs_ty
                        || self.type_contains_unknown(lhs_ty)
                        || self.type_contains_unknown(rhs_ty)
                    {
                        let comparison = format!("{} == {}", self.erase(lhs)?, self.erase(rhs)?);
                        return Ok(if *op == smelt_hir::BinOp::NotEq {
                            format!("!({comparison})")
                        } else {
                            comparison
                        });
                    }
                }
                Ok(format!(
                    "{} {} {}",
                    self.operand_text(lhs)?,
                    smelt_hir::bin_op_text(*op),
                    self.operand_text(rhs)?
                ))
            }
            Rvalue::Unary { op, operand } => match op {
                smelt_hir::UnaryOp::Not => Ok(format!("!({})", self.truthy_operand_text(operand)?)),
                smelt_hir::UnaryOp::Neg => {
                    let operand_ty = self.operand_ty(operand)?;
                    let numeric_text = if matches!(
                        self.mir.types.get(operand_ty),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                    ) {
                        let float_ty = self.type_id(Type::Float)?;
                        self.primitive_cast_text(
                            smelt_hir::PrimitiveCastOp::ToFloat,
                            operand,
                            float_ty,
                        )?
                    } else {
                        self.operand_text(operand)?
                    };
                    if self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
                        self.erase_value_text(
                            &format!("-({numeric_text})"),
                            self.type_id(Type::Float)?,
                        )
                    } else {
                        Ok(format!("-{numeric_text}"))
                    }
                }
            },
            Rvalue::Conditional {
                cond,
                then_operand,
                else_operand,
            } => Ok(format!(
                "if {} {{ {} }} else {{ {} }}",
                self.operand_text(cond)?,
                self.value_at_type(then_operand, dest_ty)?,
                self.value_at_type(else_operand, dest_ty)?
            )),
            Rvalue::FunctionTableLookup { key, cases } => {
                self.function_table_lookup_text(key, cases, dest_ty)
            }
            Rvalue::OptionalField { receiver, field } => {
                self.optional_field_text_for_dest(receiver, *field, dest_ty)
            }
            Rvalue::OptionalIndex { receiver, index } => {
                self.optional_index_text(receiver, index, dest_ty)
            }
            Rvalue::OptionalMethod {
                receiver,
                method,
                args,
            } => self.optional_method_text(receiver, *method, args, dest_ty),
            Rvalue::UnionMethod {
                receiver,
                method,
                args,
            } => self.union_method_text(receiver, *method, args, dest_ty),
            Rvalue::OptionalCoalesce { optional, fallback } => {
                self.optional_coalesce_text(optional, fallback, dest_ty)
            }
            Rvalue::InstanceOf {
                value: operand,
                class,
            } => self.instance_of_text(operand, *class),
            Rvalue::UnknownIs {
                value: unknown_value,
                kind,
            } => self.tag_check(unknown_value, *kind),
            Rvalue::TypeofValue {
                value: unknown_value,
            } => self.typeof_value_text(unknown_value),
            Rvalue::PrototypeSentinel {
                value: unknown_value,
            } => self.prototype_sentinel_text(unknown_value),
            Rvalue::BoxPrimitive { value } => self.box_primitive_text(value),
            Rvalue::ObjectFromPrototype { prototype } => {
                self.object_from_prototype_text(prototype)
            }
            Rvalue::UnknownCast {
                value: unknown_value,
                target,
            } => {
                let source = self.operand_ty(unknown_value)?;
                let value_text = self.operand_text(unknown_value)?;
                if let Some(projected) =
                    self.project_union_value_text(&value_text, source, *target)?
                {
                    return self.value_at_type_text(&projected, *target, dest_ty);
                }
                let target_rust_ty = self.type_text_with_impl_trait(*target, false)?;
                let cast_text = if target_rust_ty == "SmeltUnknown" {
                    self.erase(unknown_value)?
                } else {
                    self.extract(unknown_value, *target)?
                };
                self.value_at_type_text(&cast_text, *target, dest_ty)
            }
            Rvalue::Struct { class, fields } => {
                let class_name = sanitize_ident(self.symbol_name(*class)?);
                let mir_class = self
                    .mir
                    .classes
                    .iter()
                    .find(|item| item.name == *class)
                    .ok_or_else(|| EmitError::new("struct rvalue references an unknown class"))?;
                let effective_fields = self
                    .structural_record_fields(dest_ty)
                    .unwrap_or_else(|| crate::classes::effective_class_fields(self.mir, mir_class));
                let scoped_type_params = mir_class
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect::<HashSet<_>>();
                let mut parts = Vec::new();
                for field in effective_fields {
                    let name = sanitize_ident(self.symbol_name(field.name)?);
                    if let Some((_, field_value)) = fields
                        .iter()
                        .find(|(field_name, _)| *field_name == field.name)
                    {
                        parts.push(format!("{name}: {}", self.operand_text(field_value)?));
                    } else {
                        parts.push(format!(
                            "{name}: {}",
                            self.default_value_with_scoped_type_params(
                                field.ty,
                                &TypeSubstitution::lexical(&scoped_type_params),
                            )?
                        ));
                    }
                }
                if !mir_class.type_params.is_empty() {
                    parts.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
                }
                // A reference class wraps its inner record in a fresh shared cell.
                // Identity lives in the `Rc`, so the constructor mints one cell;
                // aliasing later clones the handle (`Rc::clone`), sharing it.
                if self.context.is_reference_class(*class) {
                    return Ok(format!(
                        "{class_name}(::std::rc::Rc::new(::std::cell::RefCell::new({class_name}Inner {{ {} }})))",
                        parts.join(", ")
                    ));
                }
                Ok(format!("{class_name} {{ {} }}", parts.join(", ")))
            }
            Rvalue::ExternalClassInstance { class, args } => {
                let text = self.external_class_instance_text(*class, args)?;
                if self.is_regexp_class_symbol(*class)?
                    && matches!(self.mir.types.get(dest_ty), Some(Type::String))
                {
                    Ok(Self::regexp_literal_text(&text))
                } else {
                    Ok(text)
                }
            }
            Rvalue::Len(operand) => self.len_text(operand, dest_ty),
            Rvalue::NumericAbs(operand) => self.numeric_abs_text(operand),
            Rvalue::NumericRound { op, operand } => self.numeric_round_text(*op, operand, dest_ty),
            Rvalue::NumericExtrema { op, args, spread } => {
                self.numeric_extrema_text(*op, args, spread.as_ref(), dest_ty)
            }
            Rvalue::NumericHypot { args } => self.numeric_hypot_text(args),
            Rvalue::NumericPredicate { op, operand } => self.numeric_predicate_text(*op, operand),
            Rvalue::NumericUnaryFunc { op, operand } => self.numeric_unary_func_text(*op, operand),
            Rvalue::NumericPow { base, exponent } => self.numeric_pow_text(base, exponent),
            Rvalue::NumericAtan2 { y, x } => self.numeric_atan2_text(y, x),
            Rvalue::NumericRandom => Ok("rand::random::<f64>()".to_owned()),
            Rvalue::NumericRandomInt { start, end } => self.numeric_random_int_text(start, end),
            Rvalue::NumericToStringRadix { operand, radix } => {
                self.numeric_to_string_radix_text(operand, radix)
            }
            Rvalue::NumericToFixed { operand, digits } => {
                self.numeric_to_fixed_text(operand, digits)
            }
            Rvalue::ParseIntRadix { operand, radix } => self.parse_int_radix_text(operand, radix),
            Rvalue::PrimitiveCast { op, operand } => {
                self.primitive_cast_text(*op, operand, dest_ty)
            }
            Rvalue::StringCase { op, operand } => self.string_case_text(*op, operand, dest_ty),
            Rvalue::StringNormalize { form, operand } => self.string_normalize_text(*form, operand),
            Rvalue::UriEncode { operand } => self.uri_encode_text(operand),
            Rvalue::StringLocaleCompare { left, right } => {
                self.string_locale_compare_text(left, right)
            }
            Rvalue::ObjectToStringTag { operand } => self.object_to_string_tag_text(operand),
            Rvalue::StructuredClone { operand } => self.structured_clone_text(operand),
            Rvalue::StringTrim { side, operand } => self.string_trim_text(*side, operand),
            Rvalue::StringAffix {
                op,
                haystack,
                needle,
            } => self.string_affix_text(*op, haystack, needle),
            Rvalue::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            } => self.string_search_text(*op, haystack, needle, from_index.as_ref(), dest_ty),
            Rvalue::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => self.string_replace_text(*op, haystack, pattern, replacement),
            Rvalue::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => self.string_remove_affix_text(*op, haystack, affix),
            Rvalue::StringRepeat { operand, count } => self.string_repeat_text(operand, count),
            Rvalue::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => self.string_pad_text(*op, operand, target_len, pad),
            Rvalue::StringPredicate { op, operand } => self.string_predicate_text(*op, operand),
            Rvalue::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => self.regex_is_match_text(*op, pattern, haystack),
            Rvalue::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => self.regex_replace_text(*op, pattern, haystack, replacement),
            Rvalue::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
            } => self.regex_replace_callback_text(*op, pattern, haystack, callback),
            Rvalue::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                self.regex_replace_first_match_uppercase_text(pattern, haystack)
            }
            Rvalue::RegexSplit { pattern, haystack } => self.regex_split_text(pattern, haystack),
            Rvalue::RegexFind { pattern, haystack } => {
                let text = self.regex_find_text(pattern, haystack)?;
                let string_ty = self.type_id(Type::String)?;
                let list_ty = self.type_id(Type::List(string_ty))?;
                let source_ty = self.type_id(Type::Optional(list_ty))?;
                self.value_at_type_text(&text, source_ty, dest_ty)
            }
            Rvalue::RegexExec { regex, haystack } => {
                self.regex_exec_text(regex, haystack, dest_ty)
            }
            Rvalue::RegexMatchAll { regex, haystack } => {
                self.regex_match_all_text(regex, haystack, dest_ty)
            }
            Rvalue::StringCharAt { operand, index } => self.string_char_at_text(operand, index),
            Rvalue::StringCharCodeAt { operand, index } => {
                self.string_char_code_at_text(operand, index)
            }
            Rvalue::StringContains {
                haystack,
                needle,
                from_index,
            } => self.string_contains_text(haystack, needle, from_index.as_ref()),
            Rvalue::StringSlice {
                operand,
                start,
                end,
            } => self.string_slice_text(operand, start.as_ref(), end.as_ref(), dest_ty),
            Rvalue::ListContains { list, item } => {
                let text = self.list_contains_text(list, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetContains { set, item } => {
                let text = self.set_contains_text(set, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetDisjoint { left, right } => {
                let text = self.set_disjoint_text(left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetRelation { op, left, right } => {
                let text = self.set_relation_text(*op, left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetAdd { set, item } => self.set_add_text(set, item, dest_ty),
            Rvalue::SetRemove { op, set, item } => self.set_remove_text(*op, set, item, dest_ty),
            Rvalue::SetClear { set } => self.collection_clear_text(set, dest_ty, "set"),
            Rvalue::SetCopy { set } => self.set_copy_text(set, dest_ty),
            Rvalue::ListToSet { list } => self.list_to_set_text(list, dest_ty),
            Rvalue::ListPairsToDict { list } => self.list_pairs_to_dict_text(list, dest_ty),
            Rvalue::SetBinary { op, left, right } => {
                self.set_binary_text(*op, left, right, dest_ty)
            }
            Rvalue::SetProjection { op, set } => self.set_projection_text(*op, set, dest_ty),
            Rvalue::ListConcat { left, right } => {
                let text = self.list_concat_text(left, right)?;
                let source_ty = self.concat_result_list_ty(left, right)?;
                self.value_at_type_text(&text, source_ty, dest_ty)
            }
            // One erased `concat` argument, normalized by JavaScript's
            // `IsConcatSpreadable` rule. The argument's static type does not say
            // whether it is an array, so the array-vs-scalar choice happens at
            // runtime in the prelude helper rather than being guessed here.
            Rvalue::ConcatSpread { value } => {
                let value_ty = self.operand_ty(value)?;
                let value_text = self.erase_value_text(&self.operand_text(value)?, value_ty)?;
                Ok(format!("smelt_concat_spread({value_text})"))
            }
            Rvalue::ListSearch {
                op,
                list,
                item,
                from_index,
            } => {
                let text = self.list_search_text(*op, list, item, from_index.as_ref())?;
                let float_ty = self.type_id(Type::Float)?;
                self.value_at_type_text(&text, float_ty, dest_ty)
            }
            Rvalue::Closure { id, .. } => {
                if !matches!(
                    self.mir.types.get(dest_ty),
                    Some(
                        Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
                    )
                ) && !self.is_erased_class_type(dest_ty)
                {
                    return self.default_value(dest_ty);
                }
                if let Some(closure) = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    && closure.captures.iter().any(|capture| {
                        self.capture_is_borrowed_callback_param(capture.source_local)
                            .unwrap_or(false)
                            || self
                                .capture_symbol_is_borrowed_callback_param(
                                    capture.symbol,
                                    capture.ty,
                                )
                                .unwrap_or(false)
                    })
                    && (closure.escapes
                        || !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))))
                {
                    return self.default_value(dest_ty);
                }
                // Emit the closure value directly at its destination type.
                //
                // A bare function-item-as-value wrapper still carries a stable
                // `function_item_key`, but reference identity is now preserved at
                // the ERASE site (see `coercion::erase`), which routes erased
                // function-item references through a per-item accessor. Here, in
                // typed context, identity does not matter, so this arm emits the
                // plain fresh closure regardless of `function_item_key`.
                Ok(self.closure_text_for_type(*id, dest_ty)?)
            }
            Rvalue::ClosureCall { callee, args } => {
                let callee_ty = self.operand_ty(callee)?;
                if matches!(
                    self.mir.types.get(callee_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(callee_ty)
                {
                    let callee_text = self.operand_text(callee)?;
                    let rendered_args =
                        args.iter()
                            .map(|arg| self.erase(arg))
                            .collect::<Result<Vec<_>, EmitError>>()?;
                    // A `ClosureCall` carries the exact argument list; spread calls
                    // are represented separately as `ClosureCallSpread`, which packs
                    // and flattens the runtime vector. Pass these arguments verbatim
                    // so a single array argument is delivered as one value instead of
                    // being mistaken for a spread and flattened into its elements.
                    let smelt_call_args = format!("vec![{}]", rendered_args.join(", "));
                    let call_text =
                        self.dynamic_callable_dispatch_text(&callee_text, &smelt_call_args);
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                        return Ok(call_text);
                    }
                    return self.value_at_type_text(&call_text, unknown_ty, dest_ty);
                }
                // An optional call `f?.(args)` whose callee is an absent-able
                // `Option<Rc<dyn Fn(..)>>` must short-circuit to `None`
                // (JavaScript `undefined`) when the callee is missing, rather
                // than substitute a null-returning default callback and call it
                // unconditionally. Render it as `callee.map(|f| f(args))` so an
                // absent callee yields `None` and a present one yields
                // `Some(result)`. The non-throwing case is handled here; a
                // throwing inner function is routed through the terminator call
                // path (see MIR `ClosureCall` lowering) where
                // `optional_indirect_call_text_for_dest` applies the same rule.
                if let Some(Type::Optional(inner_callee_ty)) = self.mir.types.get(callee_ty)
                    && let Some(Type::Function(function)) =
                        self.mir.types.get(*inner_callee_ty).cloned()
                {
                    let dest_is_optional =
                        matches!(self.mir.types.get(dest_ty), Some(Type::Optional(_)));
                    let inner_dest_ty = match self.mir.types.get(dest_ty) {
                        Some(Type::Optional(inner)) => *inner,
                        _ => dest_ty,
                    };
                    let callee_text = self.operand_text(callee)?;
                    let rendered_args = self.indirect_call_args_text(&function, args)?;
                    let raw_call = if function.may_throw {
                        format!(
                            "(smelt_function)({rendered_args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                        )
                    } else {
                        format!("(smelt_function)({rendered_args})")
                    };
                    let coerced_call =
                        self.value_at_type_text(&raw_call, function.return_ty, inner_dest_ty)?;
                    let map_expr =
                        format!("{callee_text}.clone().map(|smelt_function| {coerced_call})");
                    if dest_is_optional {
                        return Ok(map_expr);
                    }
                    // The destination is not an `Option` (an erased
                    // `SmeltUnknown` seam): an absent callee lowers to
                    // `undefined` at the destination type.
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    let undefined_text =
                        self.value_at_type_text("SmeltUnknown::Undefined", unknown_ty, dest_ty)?;
                    return Ok(format!("{map_expr}.unwrap_or({undefined_text})"));
                }
                // A callable-object record IS callable: its synthetic
                // `__smelt_call` slot holds the underlying function. Falling
                // through to `default_value` below silently replaced the whole
                // call with `null` — es-toolkit's `memoize` (whose result type is
                // `F & { cache }`) lost every `memoized(value)` inside a callback
                // that way, with no diagnostic. Route the call through the slot.
                if let Some(call_text) = self.callable_object_slot_call_text(callee, args, dest_ty)?
                {
                    return Ok(call_text);
                }
                if !matches!(self.mir.types.get(callee_ty), Some(Type::Function(_))) {
                    return self.default_value(dest_ty);
                }
                let callee_text = self.operand_text(callee)?;
                if callee_text == "()" {
                    return self.default_value(dest_ty);
                }
                let emitted_params = self.emitted_function_param_types(&callee_text)?;
                let local_function = match callee {
                    Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                        match self.mir.types.get(self.local_decl(*local)?.ty) {
                            Some(Type::Function(function)) => Some(function),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let local_params = local_function.map(|function| function.params.as_slice());
                let inferred_function = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => Some(function),
                    _ => None,
                };
                let inferred_params = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => function.params.as_slice(),
                    _ => &[],
                };
                let params = local_params
                    .or(emitted_params.as_deref())
                    .unwrap_or(inferred_params);
                let rest_function = local_function.or(inferred_function);
                // The by-reference ABI is the callee's, and a local that received
                // the result of a generic call holds a value whose ABI was decided
                // by the callee's DECLARED signature, not by the instantiated type
                // MIR gave the local. See `emitted_call_result_function_type`.
                let abi_function = match callee {
                    Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                        self.emitted_call_result_function_type(*local)
                    }
                    _ => None,
                };
                let mut rendered_args = if let Some(rest_args) =
                    self.rest_vector_call_args_text(args, rest_function)?
                {
                    rest_args
                } else {
                    let mut rendered_args = args
                        .iter()
                        .zip(params.iter())
                        .enumerate()
                        .map(|(index, (arg, param))| {
                            let text = if rest_function.is_some_and(|function| {
                                function.mutable_params.contains(&index)
                            }) {
                                self.mutable_reference_argument_text(arg, *param, None)?
                            } else if abi_function.as_ref().map_or_else(
                                || {
                                    rest_function.is_some_and(|function| {
                                        self.callback_param_is_shared_reference(
                                            function, index, *param,
                                        )
                                    })
                                },
                                |function| {
                                    function.params.get(index).is_some_and(|declared| {
                                        self.callback_param_is_shared_reference(
                                            function, index, *declared,
                                        )
                                    })
                                },
                            ) {
                                // The parameter is `&T`, so pass a reference. Borrowing
                                // the place is the point: this is the per-element
                                // whole-list copy that made array callbacks quadratic.
                                self.shared_reference_argument_text(arg, *param)?
                            } else {
                                self.value_at_type(arg, *param)?
                            };
                            if self.type_text(*param)? == "Vec<SmeltUnknown>"
                                && text
                                    .contains(".into_iter().map(|value| value).collect::<Vec<_>>()")
                            {
                                Ok(text.replace(
                                    ".into_iter().map(|value| value).collect::<Vec<_>>()",
                                    ".into_iter().map(|value| value.into_smelt_unknown()).collect::<Vec<_>>()",
                                ))
                            } else {
                                Ok(text)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    // A padded argument is a fresh temporary, so a
                    // by-shared-reference parameter borrows it in place. Without
                    // this the padding is the one argument in the ladder that
                    // ignores the callee's ABI (E0308) — es-toolkit's
                    // `isEqualWith` calls a six-parameter comparator with two
                    // required arguments and four padded ones.
                    for (index, param) in params.iter().enumerate().skip(args.len()) {
                        let default_text = self.default_value(*param)?;
                        rendered_args.push(match (abi_function.as_ref(), rest_function) {
                            (Some(function), _) => match function.params.get(index) {
                                Some(declared) => self.callback_call_arg_text(
                                    function,
                                    index,
                                    *declared,
                                    default_text,
                                ),
                                None => default_text,
                            },
                            (None, Some(function)) => {
                                self.callback_call_arg_text(function, index, *param, default_text)
                            }
                            (None, None) => default_text,
                        });
                    }
                    rendered_args
                };
                if args.is_empty()
                    && rendered_args.as_slice() == ["Vec::new()"]
                    && matches!(
                        self.mir.types.get(callee_ty),
                        Some(Type::Function(function)) if function.params.is_empty()
                    )
                {
                    rendered_args.clear();
                }
                let args_text = rendered_args.join(", ");
                let callee_is_erased_rest = matches!(
                    self.mir.types.get(callee_ty),
                    Some(Type::Function(function))
                        if self.is_erased_unknown_rest_function(function) && !function.may_throw
                );
                // The choice between the erased `.call(..)` ABI and a direct
                // invocation is answered once, by
                // `callee_uses_erased_call_method` (see its docs for the
                // precedence and why it matters); `call_text`'s
                // `Callee::Indirect` arm consults the same helper.
                let call_text = if self.callee_uses_erased_call_method(callee)? {
                    format!("{callee_text}.call({args_text})")
                } else if self.callee_is_borrowed_function_handle(callee)? {
                    format!("{callee_text}({args_text})")
                } else {
                    format!("({callee_text})({args_text})")
                };
                let (source_ty, rendered_call_text) = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => {
                        let returns_future = matches!(
                            self.mir.types.get(function.return_ty),
                            Some(Type::Future(_))
                        );
                        // A throwing callee invoked in statement position
                        // propagates its error with `?` when the enclosing
                        // emitted body itself returns a `Result` — a recoverable
                        // JavaScript exception must stay recoverable. The
                        // `panic!` is kept only where the surrounding Rust
                        // signature genuinely cannot carry an error (a
                        // non-throwing body, or a generator whose `?` would
                        // target the wrong output type).
                        let throwing_call_text = if function.may_throw && !returns_future {
                            if self.body_can_propagate_error() {
                                format!("{call_text}?")
                            } else {
                                format!(
                                    "{call_text}.unwrap_or_else(|error| panic!(\"{{}}\", error))"
                                )
                            }
                        } else {
                            call_text
                        };
                        // The fully-erased `SmeltErasedFunction::call` ABI always
                        // yields a bare `SmeltUnknown`, regardless of the callee's
                        // declared return type. Coercing the call result from the
                        // declared `return_ty` (e.g. `T | undefined`, which lowers
                        // to `Option<SmeltUnknown>`) would suppress the wrap the
                        // destination needs, so treat the erased-rest call's source
                        // type as `Unknown` and let `value_at_type_text` inject the
                        // correct `Some(..)`/extraction at the assignment seam.
                        let source_ty = if callee_is_erased_rest {
                            self.type_id(Type::Unknown)?
                        } else {
                            function.return_ty
                        };
                        (source_ty, throwing_call_text)
                    }
                    _ => (dest_ty, call_text),
                };
                if matches!(self.mir.types.get(source_ty), Some(Type::Unknown))
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
                {
                    return Ok(rendered_call_text);
                }
                if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
                    return Ok(format!("{{ {rendered_call_text}; () }}"));
                }
                self.value_at_type_text(&rendered_call_text, source_ty, dest_ty)
            }
            Rvalue::ClosureCallSpread { callee, args } => {
                let callee_text = self.operand_text(callee)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                let args_ty = self.type_id(Type::List(unknown_ty))?;
                let args_text = self
                    .inline_list_concat_operand_text(args)?
                    .unwrap_or(self.value_at_type(args, args_ty)?);
                if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callee)?)
                {
                    let callee_is_erased_rest =
                        self.is_erased_unknown_rest_function(function) && !function.may_throw;
                    // A typed callee with leading positional parameters before its
                    // rest cannot receive the packed spread list as a single
                    // argument; redistribute the list into `(positionals…,
                    // rest_list)` read from an in-scope `smelt_spread_args`.
                    let split_args = if callee_is_erased_rest {
                        None
                    } else {
                        self.spread_leading_positional_call_args_text(function)?
                    };
                    // The argument list handed to the callee: either the split
                    // positional+rest text (reads `smelt_spread_args`) or the
                    // packed list verbatim.
                    let inner_args = split_args.as_deref().unwrap_or(&args_text);
                    // See the `ClosureCall` arm: the ABI question is answered by
                    // the shared `callee_uses_erased_call_method` helper.
                    let uses_erased_call_method = self.callee_uses_erased_call_method(callee)?;
                    let inner_call = if uses_erased_call_method {
                        format!("{callee_text}.call({inner_args})")
                    } else if self.callee_is_borrowed_function_handle(callee)? {
                        format!("{callee_text}({inner_args})")
                    } else {
                        format!("({callee_text})({inner_args})")
                    };
                    // When the arguments were split, bind the packed list once so
                    // the positional/rest reads above resolve; otherwise the call
                    // stands alone.
                    let call_text = if split_args.is_some() {
                        format!("{{ let smelt_spread_args = {args_text}; {inner_call} }}")
                    } else {
                        inner_call
                    };
                    if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
                        return Ok(format!("{{ {call_text}; () }}"));
                    }
                    // Same rule as the `ClosureCall` arm above, for the same
                    // reason: the erased ABI -- `SmeltErasedFunction::call`, or the
                    // borrowed `dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown`
                    // handle it is emitted as -- always yields a bare
                    // `SmeltUnknown`, whatever the callee's declared return type
                    // says. Coercing from the declared `return_ty` emits an
                    // identity conversion and leaves a `SmeltUnknown` assigned to,
                    // say, a `bool` destination: an E0308 the moment a spread call
                    // reaches such a callee with a non-`unknown` return, as
                    // es-toolkit's `cond` does with `predicate.apply(this, args)`
                    // on a `(...args: any[]) => boolean`.
                    //
                    // The condition keys on the ABI actually chosen for
                    // `inner_call` above, not just on the erased-rest shape, so the
                    // two cannot drift: ANY call rendered as `.call(..)` returns the
                    // erased carrier and needs the same correction.
                    let source_ty = if uses_erased_call_method || callee_is_erased_rest {
                        unknown_ty
                    } else {
                        function.return_ty
                    };
                    return self.value_at_type_text(&call_text, source_ty, dest_ty);
                }
                // The runtime dispatch snippet matches the callee over
                // `SmeltUnknown` discriminants, so the callee value must be the
                // bare erased carrier. A callee typed as `Optional<..>` or a
                // concrete union renders as `Option<SmeltUnknown>` /
                // `SmeltUnion..`, not `SmeltUnknown`; erase it to the runtime
                // carrier first (flow can narrow an optional callable to a
                // definitely-present function, and this unwraps it) so the match
                // sees the shape it expects instead of failing to type-check
                // (E0308). An already-`Unknown` callee coerces to itself.
                let erased_callee =
                    self.value_at_type_text(&callee_text, self.operand_ty(callee)?, unknown_ty)?;
                let call_text = self.dynamic_callable_dispatch_text(&erased_callee, &args_text);
                if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                    return Ok(call_text);
                }
                self.value_at_type_text(&call_text, unknown_ty, dest_ty)
            }
            Rvalue::ListCallback { op, list, callback } => {
                self.list_callback_text(*op, list, callback, dest_ty)
            }
            Rvalue::ListFromLength { length } => self.list_from_length_text(length, dest_ty),
            Rvalue::ListRepeat {
                value: repeat_value,
                count,
            } => self.list_repeat_text(repeat_value, count, dest_ty),
            Rvalue::ListFromLengthMap { length, callback } => {
                self.list_from_length_map_text(length, callback, dest_ty)
            }
            Rvalue::ListReduce {
                list,
                initial,
                callback,
            } => self.list_reduce_text(list, initial.as_ref(), callback, dest_ty),
            Rvalue::ListSlice { list, start, end } => {
                self.list_slice_text(list, start.as_ref(), end.as_ref(), dest_ty)
            }
            Rvalue::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => self.list_splice_text(list, start, delete_count.as_ref(), items, *mutate, dest_ty),
            Rvalue::ListFill {
                list,
                value: fill_value,
                start,
                end,
            } => self.list_fill_text(list, fill_value, start.as_ref(), end.as_ref(), dest_ty),
            Rvalue::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => self.list_copy_within_text(list, target, start, end.as_ref(), dest_ty),
            Rvalue::ListWith {
                list,
                index,
                value: replacement,
            } => self.list_with_text(list, index, replacement, dest_ty),
            Rvalue::ListFlat { list, depth } => self.list_flat_text(list, depth.as_ref(), dest_ty),
            Rvalue::ListProjection { op, list } => self.list_projection_text(*op, list, dest_ty),
            Rvalue::ListPush { list, item } => self.list_push_text(list, item, dest_ty),
            Rvalue::ListExtend { list, other } => self.list_extend_text(list, other, dest_ty),
            Rvalue::ListInsert { list, index, item } => {
                self.list_insert_text(list, index, item, dest_ty)
            }
            Rvalue::ListUnshift { list, items } => self.list_unshift_text(list, items, dest_ty),
            Rvalue::ListReverse { list } => self.list_reverse_text(list, dest_ty),
            Rvalue::ListClear { list } => self.collection_clear_text(list, dest_ty, "list"),
            Rvalue::ListCopy { list } => self.list_copy_text(list, dest_ty),
            Rvalue::TupleToList { tuple } => self.tuple_to_list_text(tuple, dest_ty),
            Rvalue::ListToTuple { list } => self.list_to_tuple_text(list, dest_ty),
            Rvalue::TupleToSet { tuple } => self.tuple_to_set_text(tuple, dest_ty),
            Rvalue::ListCount { list, item } => self.list_count_text(list, item, dest_ty),
            Rvalue::ListSum { list } => self.list_sum_text(list, dest_ty),
            Rvalue::ListBoolFold { op, list } => self.list_bool_fold_text(*op, list),
            Rvalue::ListSorted { list, key, reverse } => {
                self.list_sorted_text(list, key.as_ref(), *reverse, dest_ty)
            }
            Rvalue::ListReversed { list } => self.list_reversed_text(list, dest_ty),
            Rvalue::ListEnumerate { list } => self.list_enumerate_text(list, dest_ty),
            Rvalue::ListZip { left, right } => self.list_zip_text(left, right, dest_ty),
            Rvalue::ListRange { start, end, step } => {
                self.list_range_text(start, end, step, dest_ty)
            }
            Rvalue::ListRandomChoice { list } => self.list_random_choice_text(list, dest_ty),
            Rvalue::ListIndex { list, item } => self.list_index_text(list, item, dest_ty),
            Rvalue::ListRemove { list, item } => self.list_remove_text(list, item, dest_ty),
            Rvalue::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => self.list_sort_text(list, comparator.as_ref(), key.as_ref(), *reverse, dest_ty),
            Rvalue::ListPop { list } => self.list_pop_text(list, dest_ty),
            Rvalue::ListShift { list } => self.list_shift_text(list, dest_ty),
            Rvalue::ListNext { list } => self.list_next_text(list, dest_ty),
            Rvalue::IteratorDone { result } => {
                Ok(format!("{}.is_none()", self.operand_text(result)?))
            }
            Rvalue::IteratorValue { result } => {
                let text = self.operand_text(result)?;
                if self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
                    self.erase_value_text(&text, self.operand_ty(result)?)
                } else {
                    self.value_at_type(result, dest_ty)
                }
            }
            Rvalue::TupleContains { tuple, item } => self.tuple_contains_text(tuple, item),
            Rvalue::TupleIndex { tuple, index } => self.tuple_index_text(tuple, *index, dest_ty),
            Rvalue::TupleSlice { tuple, start, end } => {
                self.tuple_slice_text(tuple, *start, *end, dest_ty)
            }
            Rvalue::DictContainsKey { dict, key } => self.dict_contains_key_text(dict, key),
            Rvalue::DictSet {
                dict,
                key,
                value: dict_value,
            } => self.dict_set_text(dict, key, dict_value, dest_ty),
            Rvalue::DictRemoveKey { dict, key } => self.dict_remove_key_text(dict, key, dest_ty),
            Rvalue::DictGet { dict, key, default } => {
                self.dict_get_text(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictSetDefault { dict, key, default } => {
                self.dict_setdefault_text(dict, key, default, dest_ty)
            }
            Rvalue::DictClear { dict } => self.collection_clear_text(dict, dest_ty, "dict"),
            Rvalue::DictPop { dict, key, default } => {
                self.dict_pop_text(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictUpdate { dict, other } => self.dict_update_text(dict, other, dest_ty),
            Rvalue::DictAssign { target, sources } => {
                self.dict_assign_text(target, sources, dest_ty)
            }
            Rvalue::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => self.callable_object_assign_text(callable, props, spreads, dest_ty),
            Rvalue::DictCopy { dict } => self.dict_copy_text(dict, dest_ty),
            Rvalue::DictProjection { op, dict } => self.dict_projection_text(*op, dict),
            Rvalue::StringSplit {
                haystack,
                separator,
                limit,
            } => self.string_split_text(haystack, separator, limit.as_ref(), dest_ty),
            Rvalue::StringChars { haystack } => self.string_chars_text(haystack, dest_ty),
            Rvalue::StringJoin { items, separator } => self.string_join_text(items, separator),
            Rvalue::JsonStringify { value: json_value } => {
                self.json_stringify_text(json_value, dest_ty)
            }
            Rvalue::JsonParse { text } => self.json_parse_text(text, dest_ty),
            Rvalue::HttpGetText { url } => self.http_get_text(url),
            Rvalue::GlobalGet { global } => self.global_get_text(*global),
            Rvalue::GlobalSet { global, value: stored } => {
                self.global_set_text(*global, stored)
            }
            Rvalue::DateNow => {
                // `Date.now()` shares the timer timeline: real wall time plus the
                // virtual fast-forward accumulated by `sleep`/timer draining
                // (`SMELT_VIRTUAL_MS`, emitted alongside the date runtime). This
                // keeps elapsed-time measurements consistent with `setTimeout`
                // deadlines under the deterministic virtual clock. An explicit
                // `vi.setSystemTime(...)` override (`SMELT_DATE_NOW`) still wins.
                let text = "SMELT_DATE_NOW.with(::std::cell::Cell::get).unwrap_or_else(|| chrono::Utc::now().timestamp_millis().saturating_add(SMELT_VIRTUAL_MS.with(::std::cell::Cell::get) as i64))";
                self.date_timestamp_result_text(text, dest_ty)
            }
            Rvalue::DateSetNow { timestamp } => Ok(format!(
                "{{ SMELT_DATE_NOW.with(|value| value.set(Some(({}) as i64))); {} }}",
                self.date_timestamp_text(timestamp)?,
                self.default_value(dest_ty)?
            )),
            Rvalue::DateResetNow => Ok(format!(
                "{{ SMELT_DATE_NOW.with(|value| value.set(None)); {} }}",
                self.default_value(dest_ty)?
            )),
            Rvalue::DateTimezoneOffset => {
                Ok("SMELT_DATE_TIMEZONE_OFFSET.with(::std::cell::Cell::get)".to_owned())
            }
            Rvalue::DateSetTimezoneOffset { offset } => Ok(format!(
                "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set({})); {} }}",
                self.value_at_type(offset, self.type_id(Type::Float)?)?,
                self.default_value(dest_ty)?
            )),
            Rvalue::DateResetTimezoneOffset => Ok(format!(
                "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set(0.0)); {} }}",
                self.default_value(dest_ty)?
            )),
            // Vitest mock rvalues: the mock and its recorded calls live behind
            // the erased ABI (a genuine dynamic boundary — see the prelude
            // comment on `smelt_vitest_mock_new`), so every operand is erased
            // to `SmeltUnknown` before crossing into the runtime helpers.
            Rvalue::VitestMockFn { implementation } => Ok(match implementation {
                Some(implementation) => format!(
                    "smelt_vitest_mock_new(Some({}))",
                    self.value_at_type(implementation, self.type_id(Type::Unknown)?)?
                ),
                None => "smelt_vitest_mock_new(None)".to_owned(),
            }),
            Rvalue::VitestMockCalledTimes { mock, count } => Ok(format!(
                "smelt_vitest_mock_called_times(&({}), ({}) as f64)",
                self.value_at_type(mock, self.type_id(Type::Unknown)?)?,
                self.value_at_type(count, self.type_id(Type::Float)?)?
            )),
            Rvalue::VitestMockCalledWith { mock, args, last } => Ok(format!(
                "smelt_vitest_mock_called_with(&({}), vec![{}], {})",
                self.value_at_type(mock, self.type_id(Type::Unknown)?)?,
                args.iter()
                    .map(|arg| self.value_at_type(arg, self.type_id(Type::Unknown)?))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", "),
                last
            )),
            Rvalue::VitestMockLastResolvedWith { mock, expected } => Ok(format!(
                "smelt_vitest_mock_last_resolved_with(&({}), {})",
                self.value_at_type(mock, self.type_id(Type::Unknown)?)?,
                self.value_at_type(expected, self.type_id(Type::Unknown)?)?
            )),
            Rvalue::DateTimezoneContext { timezone } => Ok(format!(
                "{{ let smelt_timezone_name = {}; let smelt_timezone: chrono_tz::Tz = smelt_timezone_name.parse().expect(\"invalid IANA time zone\"); ::std::rc::Rc::new(move |value: SmeltUnknown| -> SmeltUnknown {{ let timestamp_ms = match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => chrono::DateTime::parse_from_rfc3339(&value).map(|date| date.timestamp_millis() as f64).unwrap_or_else(|_| value.parse::<f64>().unwrap_or(f64::NAN)), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}; let local_timestamp_ms = if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map_or(f64::NAN, |date| date.with_timezone(&smelt_timezone).naive_local().and_utc().timestamp_millis() as f64) }} else {{ f64::NAN }}; SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(local_timestamp_ms)), (\"__smelt_timezone\".to_owned(), SmeltUnknown::String(smelt_timezone_name.clone().into()))]))) }}) }}",
                self.operand_text(timezone)?
            )),
            Rvalue::DateToIsoString { timestamp_ms } => self.date_to_iso_string_text(timestamp_ms),
            Rvalue::DateToString { timestamp_ms } => self.date_to_string_text(timestamp_ms),
            Rvalue::DateFromParts { parts } => {
                let text = self.date_from_parts_text(parts)?;
                self.date_timestamp_result_text(&text, dest_ty)
            }
            Rvalue::DateFromValue { value: date_value } => {
                let text = self.date_timestamp_text(date_value)?;
                self.date_timestamp_result_preserving_receiver_text(&text, dest_ty, date_value)
            }
            Rvalue::DateGetPart { part, timestamp_ms } => {
                self.date_get_part_text(*part, timestamp_ms)
            }
            Rvalue::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => {
                let text = self.date_set_part_text(*part, timestamp_ms, values)?;
                self.date_timestamp_result_preserving_receiver_text(&text, dest_ty, timestamp_ms)
            }
            Rvalue::UrlField { field, url } => self.url_field_text(*field, url),
            Rvalue::FileReadText { path } => self.file_read_text(path),
            Rvalue::FileWriteText { path, text } => self.file_write_text(path, text),
            Rvalue::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => {
                let parts_text = self.operand_text(parts)?;
                let type_text = self.operand_text(blob_type)?;
                let name_text = match name {
                    Some(name_operand) => {
                        format!("Some(({}).clone())", self.operand_text(name_operand)?)
                    }
                    None => "None".to_owned(),
                };
                let last_modified_text = match last_modified {
                    Some(last_modified_operand) => {
                        format!(
                            "Some(({}) as f64)",
                            self.operand_text(last_modified_operand)?
                        )
                    }
                    None => "None".to_owned(),
                };
                Ok(format!(
                    "{blob_record_from_parts}(({parts_text}).clone(), ({type_text}).clone(), {name_text}, {last_modified_text})",
                    blob_record_from_parts =
                        smelt_stdlib::runtime_symbols::host::BLOB_RECORD_FROM_PARTS,
                ))
            }
            Rvalue::HostConstruct { class_name, args } => {
                self.host_construct_text(class_name, args, dest_ty)
            }
            Rvalue::BuiltinNamespace { name } => self.builtin_namespace_text(name, dest_ty),
            Rvalue::ArgumentsObject { fixed, rest } => {
                self.arguments_object_text(fixed, rest.as_ref(), dest_ty)
            }
            Rvalue::HostGlobalRead { class } => self.host_global_read_text(*class, dest_ty),
            Rvalue::HostGlobalWrite {
                class,
                value: stored,
            } => self.host_global_write_text(*class, stored, dest_ty),
            Rvalue::HostGlobalPresent { class } => self.host_global_present_text(*class),
            Rvalue::Await(operand) => {
                if self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::None) {
                    return self.default_value(dest_ty);
                }
                Ok(format!("{}.await?", self.await_operand_text(operand)?))
            }
            Rvalue::AsyncOp { op, args } => {
                let text = self.async_op_text(*op, args, dest_ty)?;
                if matches!(op, smelt_hir::AsyncOp::Sleep)
                    && let Some(Type::Future(item)) = self.mir.types.get(dest_ty)
                    && self.mir.types.get(*item) != Some(&Type::None)
                {
                    return Ok(format!(
                        "SmeltFuture::from_future(Box::pin(async move {{ {text}.await?; Ok::<_, Box<dyn std::error::Error>>({}) }}))",
                        self.default_value(*item)?,
                    ));
                }
                Ok(text)
            }
        }
    }





    /// Converts a runtime-backed async operation to Rust.
    /// Validates two same-typed set operands.
    pub(super) fn validate_set_pair_operands(
        &self,
        left: &Operand,
        right: &Operand,
        context: &str,
    ) -> Result<TypeId, EmitError> {
        let left_ty = self.operand_ty(left)?;
        if self.operand_ty(right)? != left_ty {
            return Err(EmitError::new(format!(
                "{context} operands must have the same set type"
            )));
        }
        if !matches!(self.mir.types.get(left_ty), Some(Type::Set(_))) {
            return Err(EmitError::new(format!("{context} operands must be sets")));
        }
        Ok(left_ty)
    }





    /// Converts a function call to its Rust text representation.
    /// Converts an awaited future operand without cloning it.
    pub(super) fn await_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place),
            Operand::Const(_) => self.operand_text(operand),
        }
    }





















    /// Emits Rust for selecting a function value from a static string-keyed table.
    fn function_table_lookup_text(
        &self,
        key: &Operand,
        cases: &[(String, Operand)],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let key_text = self.operand_text(key)?;
        let cases_text = cases
            .iter()
            .map(|(case_key, case)| {
                let case_text = self.value_at_type(case, dest_ty)?;
                Ok(format!("{case_key:?} => {case_text}"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        Ok(format!(
            "{{ let __smelt_function_key = {key_text}; match __smelt_function_key.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", __smelt_function_key) }} }}"
        ))
    }


    /// Returns the static type produced by a field read helper.
    pub(super) fn field_access_type(&self, receiver_ty: TypeId, field: Symbol) -> Result<TypeId, EmitError> {
        if let Some(Type::Dict(_, value)) = self.mir.types.get(receiver_ty) {
            return Ok(*value);
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(receiver_ty)
        {
            return self.type_id(Type::Unknown);
        }
        // A `String` receiver's builtin fields have concrete Rust types; keep
        // this in sync with `string_field_text` so a caller coercing the field
        // read does not treat the concrete result as an erased `SmeltUnknown`
        // (e.g. `.length` is an `i64`, not a `SmeltUnknown::Number`).
        if matches!(self.mir.types.get(receiver_ty), Some(Type::String)) {
            return match self.symbol_name(field)? {
                "source" => self.type_id(Type::String),
                "global" | "ignoreCase" | "ignore_case" | "multiline" => {
                    self.type_id(Type::Bool)
                }
                "length" => self.type_id(Type::Int),
                _ => self.type_id(Type::Unknown),
            };
        }
        // Likewise for a concrete `RegExp` receiver (see `regexp_field_text`).
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return match self.symbol_name(field)? {
                "source" | "flags" => self.type_id(Type::String),
                "global" | "ignoreCase" | "ignore_case" | "multiline" | "sticky" | "unicode"
                | "dotAll" | "dot_all" => self.type_id(Type::Bool),
                "lastIndex" | "last_index" => self.type_id(Type::Float),
                _ => self.type_id(Type::Unknown),
            };
        }
        // A concrete match-result receiver reached through an optional chain
        // types its fields the same way the direct accessor does (see
        // `match_field_ty`): named groups are `Option<String>`.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && let Some(kind) = self.match_class_kind(*name)?
        {
            return self.match_field_ty(kind, field);
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return self.type_id(Type::Unknown);
        };
        if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
            return Ok(crate::classes::effective_class_fields(self.mir, class)
                .into_iter()
                .find(|class_field| class_field.name == field)
                .map_or_else(
                    || self.type_id(Type::Unknown),
                    |class_field| Ok(class_field.ty),
                )?);
        }
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return Ok(interface
                .fields
                .iter()
                .find(|item| item.name == field)
                .map_or_else(|| self.type_id(Type::Unknown), |item| Ok(item.ty))?);
        }
        self.type_id(Type::Unknown)
    }








    /// Packs scalar callback call arguments for an erased rest-vector callback ABI.
    pub(super) fn rest_vector_call_args_text(
        &self,
        args: &[Operand],
        function: Option<&FunctionType>,
    ) -> Result<Option<Vec<String>>, EmitError> {
        let Some(function_ty) = function else {
            return Ok(None);
        };
        let Some(0) = function_ty.rest else {
            return Ok(None);
        };
        let [param] = function_ty.params.as_slice() else {
            return Ok(None);
        };
        let Some(Type::List(item)) = self.mir.types.get(*param) else {
            return Ok(None);
        };
        if self.mir.types.get(*item) != Some(&Type::Unknown) {
            return Ok(None);
        }
        if args.is_empty() {
            return Ok(None);
        }
        if let [single_arg] = args
            && self.operand_ty(single_arg)? == *param
        {
            return Ok(None);
        }
        let items = args
            .iter()
            .map(|arg| self.value_at_type(arg, *item))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(Some(vec![format!("SmeltList::from(vec![{items}])")]))
    }

    /// Render call-argument text for a spread across a leading-positional + rest
    /// signature, binding the packed list to `smelt_spread_args`.
    ///
    /// A JavaScript call like `callee(data, ...extraArgs)` lowers to a
    /// `ClosureCallSpread` whose runtime argument list is the concatenation
    /// `[data, ...extraArgs]`. When the typed callee declares leading positional
    /// parameters before its rest (`(data: T, ...rest: U[]) => …`, i.e. `rest`
    /// starts after index 0), the packed list must be redistributed: the first
    /// `rest` elements fill the positional parameters and the remainder becomes
    /// the rest `SmeltList`. Returns `None` when the callee has no leading
    /// positional before the rest (index-0 rest is handled by the plain spread
    /// path) or when the shape does not match `[positionals…, rest_list]`.
    ///
    /// The returned string is the comma-separated argument list that reads from
    /// an in-scope `smelt_spread_args` binding; the caller wraps the whole call
    /// in a block that first binds `smelt_spread_args` to the packed list.
    fn spread_leading_positional_call_args_text(
        &self,
        function: &FunctionType,
    ) -> Result<Option<String>, EmitError> {
        let Some(rest_index) = function.rest else {
            return Ok(None);
        };
        if rest_index == 0 {
            return Ok(None);
        }
        // The rest parameter is the last declared parameter; everything before it
        // is a leading positional the packed list must supply by index.
        let Some(expected_params_len) = rest_index.checked_add(1) else {
            return Ok(None);
        };
        if function.params.len() != expected_params_len {
            return Ok(None);
        }
        let Some((rest_param, positional_params)) = function.params.split_last() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered_capacity = positional_params
            .len()
            .checked_add(1)
            .ok_or_else(|| EmitError::new("spread call argument count overflowed usize"))?;
        let mut rendered = Vec::with_capacity(rendered_capacity);
        for (index, param) in positional_params.iter().enumerate() {
            // Each positional reads the erased element at its index (absent
            // arguments become `undefined`, matching JS) and coerces to the
            // declared parameter type.
            let element = format!(
                "smelt_spread_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
            );
            let coerced = self.value_at_type_text(&element, unknown_ty, *param)?;
            rendered.push(self.callback_call_arg_text(function, index, *param, coerced));
        }
        // The rest parameter collects the remaining elements as a fresh
        // `SmeltList`; coerce each element to the rest item type when needed.
        let rest_text = if self.mir.types.get(*rest_item) == Some(&Type::Unknown) {
            format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().collect::<Vec<_>>())"
            )
        } else {
            let item_text = self.value_at_type_text("value", unknown_ty, *rest_item)?;
            format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().map(|value| {item_text}).collect::<Vec<_>>())"
            )
        };
        rendered.push(rest_text);
        Ok(Some(rendered.join(", ")))
    }

    /// Emits `Object.assign` when the target is a callable JavaScript value.
    ///
    /// JavaScript functions can carry own properties, but Rust closures cannot.
    /// The current Rust representation keeps the callable value as the runtime
    /// value and relies on MIR operands to preserve evaluation of the property
    /// expressions before this rvalue is emitted. This gives the frontend and
    /// MIR a distinct operation for callable object assignment without making
    /// closure calls or function returns stop compiling.
    fn callable_object_assign_text(
        &self,
        callable: &Operand,
        props: &[(Symbol, Operand)],
        spreads: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(dest_ty)
        {
            let callable_text = self.erase(callable)?;
            let mut entries = vec![format!(
                "smelt_object.push((\"__smelt_call\".to_owned(), {callable_text}));"
            )];
            for (key, value) in props {
                let key_text = self.symbol_source_name(*key)?;
                let value_text = self.erase(value)?;
                entries.push(format!(
                    "smelt_object.push(({key_text:?}.to_owned(), {value_text}));"
                ));
            }
            // Dynamic record sources (`Object.assign(fn, def)` with a variable
            // `def`) contribute their OWN enumerable entries at runtime. Copying
            // them AFTER the literal props preserves JS last-write-wins order,
            // while `__smelt_call` is inserted first and never overwritten by a
            // spread (a callable's own `__smelt_call` slot is not user data).
            for spread in spreads {
                let spread_text = self.erase(spread)?;
                entries.push(format!(
                    "if let SmeltUnknown::Object(smelt_spread) = {spread_text} {{ for (smelt_key, smelt_value) in smelt_spread.iter() {{ if smelt_key != \"__smelt_call\" {{ smelt_object.push((smelt_key, smelt_value)); }} }} }}"
                ));
            }
            return Ok(format!(
                "{{ let mut smelt_object = Vec::new(); {} SmeltUnknown::Object(SmeltObject::new(smelt_object)) }}",
                entries.join(" ")
            ));
        }
        // Dynamic record spreads only have a runtime home on the erased
        // (object) representation. A concrete callable-interface struct has
        // fixed fields, so a variable source cannot be merged into it.
        if !spreads.is_empty() {
            return Err(EmitError::new(
                "Object.assign with a dynamic record source onto a concrete callable interface is not supported",
            ));
        }
        // A concrete callable-interface destination (a struct carrying a
        // synthetic `__smelt_call` field plus its declared data/method fields)
        // is built as a typed struct literal: the base callable fills
        // `__smelt_call` and each recorded property fills its like-named field,
        // every value coerced to the field's exact type. This is what turns the
        // `debounce`/`throttle` pattern (a local function that receives
        // `fn.schedule = …` writes and is then returned at a callable-interface
        // type) into a real value instead of the previous behavior that dropped
        // the props and returned only the bare callable.
        if let Some(text) = self.callable_object_struct_text(callable, props, dest_ty)? {
            return Ok(text);
        }
        self.operand_text(callable)
    }

    /// Build a typed callable-interface struct literal for `CallableObjectAssign`.
    ///
    /// Reuses the same field-iteration contract as
    /// [`Self::record_literal_text_for_dest`]: the destination's declared fields
    /// (including the synthetic `__smelt_call` slot and any generic phantom) are
    /// emitted in order. The base callable supplies `__smelt_call`; each other
    /// field is filled from the recorded property of the same name (last write
    /// wins, already deduplicated upstream), coerced to the field's exact type
    /// through `value_at_type`. An `Optional` field with no matching property
    /// falls back to its default. A non-`Optional` field with no covering
    /// property is a hard [`EmitError`] rather than a silent `Default::default()`
    /// — a build blocker is preferable to an inert struct, and this only fires
    /// for genuinely uncovered required fields (the working `Object.assign`
    /// construction path always covers every declared field it targets).
    ///
    /// Returns `Ok(None)` when the destination is not a known record shape or is
    /// not a callable interface (no `__smelt_call` field), so the caller keeps
    /// its bare-callable fallback for non-interface destinations.
    fn callable_object_struct_text(
        &self,
        callable: &Operand,
        props: &[(Symbol, Operand)],
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        let type_symbol = *name;
        let type_args = args.clone();
        if !self.is_interface_record_type(dest_ty)
            && self.mir.classes.iter().all(|class| class.name != type_symbol)
        {
            return Ok(None);
        }
        let Some(fields) = self.structural_record_fields(dest_ty) else {
            return Ok(None);
        };
        if fields.is_empty() {
            return Ok(None);
        }
        // Only a genuine callable interface (one carrying the synthetic
        // `__smelt_call` storage slot) is constructed this way; a plain record
        // destination is left to the ordinary record path.
        if !fields
            .iter()
            .any(|field| self.symbol_name(field.name) == Ok("__smelt_call"))
        {
            return Ok(None);
        }

        let mut prop_values = HashMap::new();
        for (key, value) in props {
            // Keyed by the *interned* name, the same one the field side reads:
            // a source property `isPending` interns as `is_pending` and records
            // `isPending` as its source spelling, so matching source spelling
            // against interned field name would miss every camel-cased member.
            prop_values.insert(sanitize_ident(self.symbol_name(*key)?), value);
        }

        let mut field_text = Vec::new();
        for field in &fields {
            let raw_name = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(raw_name);
            let value = if raw_name == "__smelt_call" {
                self.value_at_type(callable, field.ty)?
            } else if let Some(entry_value) = prop_values.get(&field_name) {
                self.value_at_type(entry_value, field.ty)?
            } else if matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                self.default_value(field.ty)?
            } else {
                return Err(EmitError::new(format!(
                    "callable object construction is missing a value for required field `{raw_name}`"
                )));
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !type_args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }

        let type_name = sanitize_ident(self.symbol_name(type_symbol)?);
        Ok(Some(format!("{type_name} {{ {} }}", field_text.join(", "))))
    }


    /// Emits a field read against a named in-scope receiver value.
    pub(super) fn field_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(Type::Dict(key, value)) = self.mir.types.get(receiver_ty)
            && self.mir.types.get(*key) == Some(&Type::String)
        {
            let field_name = self.symbol_source_name(field)?;
            if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                if self.dict_uses_smelt_record(*key) {
                    return Ok(format!("{receiver_text}.get({field_name:?}).flatten()"));
                }
                return Ok(format!(
                    "{receiver_text}.get({field_name:?}).cloned().flatten()"
                ));
            }
            if self.dict_uses_smelt_record(*key) {
                return Ok(format!(
                    "{receiver_text}.get({field_name:?}).expect(\"missing field\")"
                ));
            }
            return Ok(format!(
                "{receiver_text}.get({field_name:?}).cloned().expect(\"missing field\")"
            ));
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(receiver_ty)
        {
            let field_name = self.symbol_source_name(field)?;
            // A concrete generated union (`SmeltUnion…`) is not a `SmeltUnknown`,
            // so the object-narrowing match below must first erase it through its
            // `IntoSmeltUnknown` boundary. `receiver_text` may be a borrow (e.g.
            // an `.as_ref().map(|_smelt_value| …)` closure parameter), so clone
            // before consuming. Genuine `Unknown` receivers already carry the
            // erased value and are matched directly.
            let scrutinee = if self.concrete_union_members(receiver_ty).is_some() {
                format!("{receiver_text}.clone().into_smelt_unknown()")
            } else {
                receiver_text.to_owned()
            };
            // `AbortController`/`AbortSignal` methods live on the host prototype,
            // not as own fields of the erased marker record, so the own-field read
            // below answers `null` for them and every `signal?.addEventListener(…)`
            // collapsed to a no-op default callback — the reason an aborted signal
            // never cancelled a pending `delay`. Bind them to the shared abort
            // record through the same runtime helper the non-optional receiver
            // path uses (see `place.rs`); the guard keeps ordinary objects that
            // happen to own a field of that name on the plain read.
            if matches!(
                field_name,
                "abort"
                    | "addEventListener"
                    | "removeEventListener"
                    | "dispatchEvent"
                    | "throwIfAborted"
            ) {
                return Ok(format!(
                    "match {scrutinee} {{ SmeltUnknown::Object(map) if (map.contains_key(\"__smelt_abortcontroller\") || map.contains_key(\"__smelt_abortsignal\")) && !map.contains_key({field_name:?}) => smelt_abort_method(map.clone(), {field_name:?}), SmeltUnknown::Object(map) => match map.get({field_name:?}).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(mut getter) if getter.contains_key(\"__smelt_get\") => match getter.remove(\"__smelt_get\") {{ Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{{}}\", error)), _ => SmeltUnknown::Null }}, value => value }}, _ => SmeltUnknown::Null }}"
                ));
            }
            return Ok(format!(
                "match {scrutinee} {{ SmeltUnknown::Object(map) => match map.get({field_name:?}).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(mut getter) if getter.contains_key(\"__smelt_get\") => match getter.remove(\"__smelt_get\") {{ Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{{}}\", error)), _ => SmeltUnknown::Null }}, value => value }}, _ => SmeltUnknown::Null }}"
            ));
        }
        if matches!(self.mir.types.get(receiver_ty), Some(Type::String)) {
            return self.string_field_text(receiver_text, field);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return self.regexp_field_text(receiver_text, field);
        }
        // A `SmeltMatch`/`MatchGroups` receiver reached through an optional chain
        // (e.g. `withoutSeparator?.groups.result`) must keep its typed match
        // accessor: a named-group read routes through `named_group_owned` rather
        // than falling through to raw struct field access, which has no such
        // field on `SmeltMatch`.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && let Some(kind) = self.match_class_kind(*name)?
        {
            return self.match_field_text(receiver_text, kind, field);
        }
        if self.storage_field_is_function(receiver_ty, field) {
            return Ok(format!(
                "{receiver_text}.{}.clone()",
                sanitize_ident(self.symbol_name(field)?)
            ));
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Err(EmitError::new(format!(
                "optional field codegen requires a class or string-keyed dict receiver, got {} for field `{}`",
                Self::type_text_for(self.mir, receiver_ty)
                    .unwrap_or_else(|_error| format!("{receiver_ty:?}")),
                self.symbol_source_name(field).unwrap_or_default()
            )));
        };
        if let Some(method_text) = self.class_method_reference_text(receiver_text, *name, field)? {
            return Ok(method_text);
        }
        Ok(format!(
            "{receiver_text}.{}.clone()",
            sanitize_ident(self.symbol_name(field)?)
        ))
    }

    /// Emits a bound closure for class method references such as `this.set`.
    ///
    /// TypeScript methods are callable values when read from an instance. Rust
    /// methods are not stored fields, so generated code captures a cloned
    /// receiver and forwards callback invocations through the inherent method.
    pub(super) fn class_method_reference_text(
        &self,
        receiver_text: &str,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let Some(function) = self.mir.functions.iter().find(|function| {
            matches!(
                function.origin,
                HirOrigin::ClassMethod {
                    class: function_class,
                    method: function_method,
                    ..
                } if function_class == class && function_method == method
            )
        }) else {
            return self.abstract_class_method_reference_text(class, method);
        };
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let unknown_ty = self.type_id(Type::Unknown)?;
        let args = function
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, param)| {
                let param_ty = self.function_local_decl(function, *param)?.ty;
                let item =
                    format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                self.value_at_type_text(&item, unknown_ty, param_ty)
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let call = format!("smelt_receiver.{method_name}({})", args.join(", "));
        let returned = if function.can_throw && !function.is_generator {
            format!("{call}?")
        } else {
            call
        };
        let wrapped_returned = self.erase_value_text(&returned, function.return_ty)?;
        let receiver_clone = if receiver_text == "self" {
            self.self_struct_clone_text(class)?
        } else {
            format!("{receiver_text}.clone()")
        };
        Ok(Some(format!(
            "{{ let smelt_receiver = {receiver_clone}; SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>({wrapped_returned}))) }}"
        )))
    }

    /// Emits an erased callable placeholder for abstract method signatures.
    ///
    /// Abstract methods have no body to emit, but source code can still pass the
    /// method reference through generic callback storage on an abstract base.
    /// The concrete override is a later virtual-dispatch problem; this keeps the
    /// abstract slice type-checkable without inventing a stored field.
    fn abstract_class_method_reference_text(
        &self,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let Some(class_item) = self.mir.classes.iter().find(|item| item.name == class) else {
            return Ok(None);
        };
        if !class_item
            .abstract_methods
            .iter()
            .any(|candidate| candidate.name == method)
        {
            return Ok(None);
        }
        Ok(Some("SmeltUnknown::Function(::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null)))".to_owned()))
    }

    /// Emits a concrete clone of `self` without relying on generic `Clone` bounds.
    fn self_struct_clone_text(&self, class: Symbol) -> Result<String, EmitError> {
        let Some(class_item) = self.mir.classes.iter().find(|item| item.name == class) else {
            return Ok("(*self).clone()".to_owned());
        };
        let mut class_name = crate::classes::class_name_text(self.mir, class_item)?;
        if !class_item.type_params.is_empty() {
            let args = class_item
                .type_params
                .iter()
                .map(|_| "SmeltUnknown")
                .collect::<Vec<_>>()
                .join(", ");
            class_name = format!("{class_name}::<{args}>");
        }
        let mut fields = crate::classes::effective_class_fields(self.mir, class_item)
            .into_iter()
            .map(|field| {
                let name = sanitize_ident(self.symbol_name(field.name)?);
                Ok(format!("{name}: self.{name}.clone()"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if !class_item.type_params.is_empty() {
            fields.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        Ok(format!("{class_name} {{ {} }}", fields.join(", ")))
    }

    /// Emits JavaScript RegExp-like metadata fields for regex strings.
    ///
    /// The frontend currently represents regex literals as pattern strings.
    /// Remeda tests still read fields such as `.source` and `.global` after a
    /// clone, so these field reads need to stay on the string representation
    /// instead of becoming Rust struct projection.
    pub(super) fn string_field_text(
        &self,
        receiver_text: &str,
        field: Symbol,
    ) -> Result<String, EmitError> {
        Ok(match self.symbol_name(field)? {
            "source" => format!("{receiver_text}.clone()"),
            "global" | "ignoreCase" | "ignore_case" | "multiline" => "false".to_owned(),
            "constructor" => "SmeltUnknown::Null".to_owned(),
            "length" => format!("({receiver_text}.chars().count() as i64)"),
            _ => "SmeltUnknown::Null".to_owned(),
        })
    }

    /// Emits JavaScript RegExp metadata field reads.
    pub(super) fn regexp_field_text(
        &self,
        receiver_text: &str,
        field: Symbol,
    ) -> Result<String, EmitError> {
        Ok(match self.symbol_name(field)? {
            "source" => format!("{receiver_text}.source.clone()"),
            "flags" => format!("{receiver_text}.flags.clone()"),
            "global" => format!("{receiver_text}.has_flag('g')"),
            "ignoreCase" | "ignore_case" => format!("{receiver_text}.has_flag('i')"),
            "multiline" => format!("{receiver_text}.has_flag('m')"),
            "sticky" => format!("{receiver_text}.has_flag('y')"),
            "unicode" => format!("{receiver_text}.has_flag('u')"),
            "dotAll" | "dot_all" => format!("{receiver_text}.has_flag('s')"),
            "lastIndex" | "last_index" => {
                // Parenthesize the cast: the read may be followed by a postfix
                // `.clone()` (or a comparison), and a bare `... as f64.clone()`
                // mis-parses the `.clone()` as part of the cast target type.
                format!("(*{receiver_text}.last_index.borrow() as f64)")
            }
            "constructor" => {
                "SmeltUnknown::Object(SmeltObject::new(Vec::from([])))"
                    .to_owned()
            }
            _ => "SmeltUnknown::Null".to_owned(),
        })
    }

    /// Emits a typed field read against a concrete `SmeltMatch` receiver.
    ///
    /// `kind` distinguishes the match value itself (`__SmeltMatch`) from its
    /// named-group accessor (`__SmeltMatchGroups`), which is the same underlying
    /// `SmeltMatch` value obtained through a `.groups` read:
    ///
    /// * On the match value: `index` -> `f64`, `length` -> `f64`, `input` ->
    ///   `String`, and `groups` yields the receiver itself (the named-group
    ///   accessor shares the `SmeltMatch` representation).
    /// * On the named-group accessor: every field is a named capture group,
    ///   read as `Option<String>` (JavaScript `undefined` when absent).
    ///
    /// These reads never build a `SmeltUnknown` property bag; the match value
    /// stays statically typed all the way to the primitive result.
    pub(super) fn match_field_text(
        &self,
        receiver_text: &str,
        kind: smelt_stdlib::StdlibClass,
        field: Symbol,
    ) -> Result<String, EmitError> {
        let field_name = self.symbol_source_name(field)?;
        match kind {
            smelt_stdlib::StdlibClass::MatchGroups => Ok(format!(
                "{receiver_text}.named_group_owned({field_name:?})"
            )),
            _ => Ok(match field_name {
                "index" => format!("{receiver_text}.index()"),
                "length" => format!("{receiver_text}.length()"),
                "input" => format!("{receiver_text}.input_owned()"),
                // `match.groups` is the same underlying `SmeltMatch` value; a
                // subsequent named-group read resolves through the
                // `MatchGroups` branch above.
                "groups" => format!("{receiver_text}.clone()"),
                _ => "SmeltUnknown::Null".to_owned(),
            }),
        }
    }

    /// Emits a numbered capture-group read against a concrete `SmeltMatch`.
    ///
    /// `match[n]` reads the n-th numbered group (entry 0 is the whole match) as
    /// an owned `Option<String>` — `None` for a group that did not participate,
    /// matching JavaScript `undefined`.
    pub(super) fn match_index_text(
        &self,
        receiver_text: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(format!("{receiver_text}.group_owned({index_text} as usize)"))
    }

    /// Emits an `Option<T>`-returning keyed read against a `Dict`-typed store.
    ///
    /// Shared by optional dict index reads and by class index-signature store
    /// reads (issue #84): both read a value by key from a `Dict(key, value)` and
    /// yield `Option<value>` where a missing key is `None` (JavaScript
    /// `undefined`). String keys coerce the index through
    /// `property_key_to_string_text`; a `SmeltRecord`/`SmeltJsMap` store already
    /// returns an owned `Option`, while a plain `HashMap` needs `.cloned()`.
    pub(super) fn dict_index_optional_read_text(
        &self,
        store_text: &str,
        key_ty: TypeId,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let key_text = if self.mir.types.get(key_ty) == Some(&Type::String) {
            let index_ty = self.operand_ty(index)?;
            if index_ty == key_ty {
                self.value_at_type(index, key_ty)?
            } else {
                self.property_key_to_string_text(&self.operand_text(index)?, index_ty)?
            }
        } else {
            self.value_at_type(index, key_ty)?
        };
        if self.dict_uses_smelt_record(key_ty) || self.dict_uses_js_key_map(key_ty) {
            Ok(format!("{store_text}.get(&{key_text})"))
        } else {
            Ok(format!("{store_text}.get(&{key_text}).cloned()"))
        }
    }



    // Converts an operand to console.log argument format and returns format string and value.
}
