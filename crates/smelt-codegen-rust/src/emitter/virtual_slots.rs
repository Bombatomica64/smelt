//! Virtual method slots: the storage fields that carry a class method through a
//! record, and the closures that dispatch them on a receiver.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Attach a callable object's dropped own properties to its erased slot.
    ///
    /// `call_field_text` is the adapted `__smelt_call` value for a narrowing
    /// record conversion. Every data field the SOURCE record declares that the
    /// TARGET does not is a JavaScript property of the underlying function
    /// value, so it is erased and merged into the erased callable's property
    /// bag; a later undeclared-member read (`place_expr`) finds it there. When
    /// the source drops nothing, the value is returned unchanged so ordinary
    /// conversions emit exactly what they did before.
    pub(super) fn callable_object_carried_properties_expr(
        &self,
        call_field_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (Some(source_fields), Some(target_fields)) = (
            self.structural_record_fields(source),
            self.structural_record_fields(target),
        ) else {
            return Ok(call_field_text.to_owned());
        };
        let mut entries = Vec::new();
        for source_field in &source_fields {
            let name = self.symbol_source_name(source_field.name)?;
            if name == SMELT_CALL {
                continue;
            }
            if target_fields.iter().any(|target_field| {
                self.symbol_source_name(target_field.name)
                    .is_ok_and(|target_name| target_name == name)
            }) {
                continue;
            }
            let field_name = self.context.names().field_text(self.mir, source_field.name);
            let value = self.erase_rendered(
                &RustExpr::path(format!("smelt_struct_value.{field_name}")).cloned_value(),
                source_field.ty,
            )?;
            entries.push(format!(
                "({name:?}.to_owned(), {value})",
                value = value.delimited()
            ));
        }
        if entries.is_empty() {
            return Ok(call_field_text.to_owned());
        }
        Ok(RustExpr::atom(format!(
            "({call_field_text}).smelt_with_properties(vec![{}])",
            entries.join(", "),
            call_field_text = call_field_text.delimited()
        )))
    }

    /// Returns true when `field` is a callable slot that represents a class method.
    ///
    /// Abstract/base classes store virtual method members as function fields so
    /// structurally adapted subclass values can keep overriding behavior after
    /// they are viewed through the base class type.
    pub(super) fn is_virtual_method_storage_field(&self, ty: TypeId, field: Symbol) -> bool {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        // A slot a class inherits is still a slot: `class L extends B {}`
        // stores `B`'s `check` slot in its flattened layout.
        let mut current = Some(*name);
        let mut seen = HashSet::new();
        while let Some(class_name) = current
            && seen.insert(class_name)
        {
            let Some(class) = crate::classes::find_class(self.mir, class_name) else {
                return false;
            };
            let declares = class
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
                });
            if declares {
                return true;
            }
            current = class.base;
        }
        false
    }

    /// Instantiate a type declared inside `source`'s class at `source`'s own
    /// class arguments.
    ///
    /// A method bound into a record slot is called on a receiver of type
    /// `source` (`ListRouter<string>`), so its declared `handler: T` really
    /// takes a `String` there. Adapting a slot argument to the bare declared `T`
    /// instead rendered it as the erased carrier (`expected String, found
    /// SmeltUnknown`). A receiver that pins nothing concretely (an erased
    /// argument, a non-class source) keeps the declared type, which is what the
    /// emitted method's Rust `T` then is.
    fn source_class_substituted_ty(&self, source: TypeId, declared: TypeId) -> TypeId {
        let Some(class) = self.class_for_type(source) else {
            return declared;
        };
        let names = class
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return declared;
        }
        let bindings = crate::generic_bindings::bind_class_type_params(self.mir, &names, source);
        crate::generic_bindings::substituted_type_id(self.mir, declared, &bindings)
            .unwrap_or(declared)
    }

    /// Returns whether an interface record's callable slot is filled by one of
    /// the SOURCE class's methods.
    ///
    /// `interface Router { add(m: string): void }` is emitted as a record whose
    /// `add` is a function-typed field, while `class ListRouter implements
    /// Router` carries `add` as a method, not a field. Structural assignability
    /// (`const r: Router = new ListRouter()`) is about members, so the slot is
    /// filled by binding the source's method to the source value — the same
    /// bound closure [`Self::virtual_method_storage_field_expr`] already emits
    /// for a base-class slot. Without this the field pairing found no source for
    /// `add` and the adapter declined (`expected Router, found ListRouter`).
    pub(super) fn is_interface_method_slot_bound_by_source(
        &self,
        source: TypeId,
        target: TypeId,
        field: Symbol,
    ) -> bool {
        if !self.is_interface_record_type(target) {
            return false;
        }
        let slot_is_callable = self
            .structural_record_fields(target)
            .and_then(|fields| fields.into_iter().find(|candidate| candidate.name == field))
            .is_some_and(|slot| matches!(self.mir.types.get(slot.ty), Some(Type::Function(_))));
        slot_is_callable
            && self
                .class_for_type(source)
                .and_then(|class| self.find_class_method_function(class.name, field))
                .is_some()
    }

    /// Emits a bound closure for a virtual method storage field when possible.
    ///
    /// The closure captures the concrete source value and dispatches to the
    /// source class implementation. This preserves JavaScript/TypeScript
    /// overridable method behavior for base-typed structural storage such as
    /// `Record<string, Parser<any>>`.
    pub(super) fn virtual_method_storage_field_expr(
        &self,
        source: TypeId,
        target: TypeId,
        method: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
        self.virtual_method_slot_closure(source, target, method, &SlotReceiver::SourceValue)
    }

    /// Bind every virtual method slot of a freshly allocated instance to the
    /// instance's own method.
    ///
    /// `instance` names the new handle (a reference class; see
    /// `classify::collect_virtual_slot_triggers`). Each slot gets a closure
    /// over a `Weak` handle on the instance's cell that calls `Self`'s method,
    /// which is the most-derived one: an inherited method is emitted into
    /// `Self`'s `impl`. JavaScript resolves `this.g()` on the prototype from
    /// the first statement of the constructor on, so the binding happens right
    /// after allocation. `Weak` keeps the instance from owning itself through
    /// its own slots. A slot whose method is abstract on this class is left at
    /// its default (the class cannot be instantiated). Answers `None` when the
    /// class has no slot to bind.
    pub(super) fn bind_own_virtual_slots_expr(
        &self,
        class: &MirClass,
        instance_ty: TypeId,
        instance: &str,
    ) -> Result<Option<RustExpr>, EmitError> {
        let mut bindings = Vec::new();
        for slot in crate::classes::own_virtual_slot_names(self.mir, class) {
            if self.find_class_method_function(class.name, slot).is_none() {
                continue;
            }
            let receiver = SlotReceiver::WeakSelf {
                handle: instance.to_owned(),
            };
            if let Some(closure) =
                self.virtual_method_slot_closure(instance_ty, instance_ty, slot, &receiver)?
            {
                let name = self.context.names().field_text(self.mir, slot);
                bindings.push(format!(
                    "{instance}.0.borrow_mut().{name} = {closure};",
                    closure = closure.delimited()
                ));
            }
        }
        if bindings.is_empty() {
            return Ok(None);
        }
        Ok(Some(RustExpr::from(bindings.join(" "))))
    }

    /// The slot closure behind [`Self::virtual_method_storage_field_expr`],
    /// with the receiver it dispatches on chosen by `receiver`.
    fn virtual_method_slot_closure(
        &self,
        source: TypeId,
        target: TypeId,
        method: Symbol,
        receiver: &SlotReceiver,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !self.is_virtual_method_storage_field(target, method)
            && !self.is_interface_method_slot_bound_by_source(source, target, method)
        {
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
        // A reference-class source keeps its fields inside the shared cell, so
        // its stored callable is read through it (the `Ref` guard ends with the
        // `.clone()`, before the call runs).
        let source_field_base = if self.is_reference_class_type(source) {
            "smelt_method_receiver.0.borrow()"
        } else {
            "smelt_method_receiver"
        };
        if dispatches_to_source_field {
            // The storage field's PARAMETER types erase while its return
            // (below) uses the caller's lexical scope. Same split as
            // `param_type`, threaded as a named empty substitution.
            let field_substitution = TypeSubstitution::erased();
            let params = self
                .callback_arg_decls(&function_ty, &field_substitution, MutablePrefix::Apply)?
                .join(", ");
            let param_types = function_ty
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    self.callback_param_type(
                        &function_ty,
                        index,
                        *param,
                        &field_substitution,
                        MutablePrefix::Apply,
                    )
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let args = function_ty
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    if self.param_passed_by_mut_ref(&function_ty, index) {
                        Ok(RustExpr::path(format!("smelt_arg{index}")))
                    } else {
                        self.rendered_at_type(
                            &RustExpr::path(format!("smelt_arg{index}")).cloned_value(),
                            *param,
                            *param,
                            &self.render_scope(),
                        )
                    }
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let method_name = self.context.names().field(self.mir, method);
            let call = if args.is_empty() {
                RustExpr::atom(format!(
                    "{{ let smelt_callable = {source_field_base}.{method_name}.clone(); smelt_callable() }}"
                ))
            } else {
                RustExpr::atom(format!(
                    "{{ let smelt_callable = {source_field_base}.{method_name}.clone(); smelt_callable({args}) }}"
                ))
            };
            let body = self.rendered_at_type(
                &call,
                function_ty.return_ty,
                function_ty.return_ty,
                &self.render_scope(),
            )?;
            let return_ty = if function_ty.may_throw {
                format!(
                    "Result<{}, {ERROR_BOX}>",
                    self.rendered_type(function_ty.return_ty)?
                )
            } else {
                self.rendered_type(function_ty.return_ty)?.into_string()
            };
            let wrapped_body = if function_ty.may_throw {
                format!("Ok::<_, {ERROR_BOX}>({body})", body = body.delimited())
            } else {
                body.into_string()
            };
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_virtual_receiver = smelt_struct_value.clone(); let smelt_virtual_method: ::std::rc::Rc<dyn Fn({param_types}) -> {return_ty}> = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let smelt_method_receiver = smelt_virtual_receiver.clone(); {wrapped_body} }}); smelt_virtual_method }}"
            ))));
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
        // Same deliberate erasure as the dispatches-to-source-field arm above.
        let field_substitution = TypeSubstitution::erased();
        let params = self
            .callback_arg_decls(&function_ty, &field_substitution, MutablePrefix::Apply)?
            .join(", ");
        let param_types = function_ty
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                self.callback_param_type(
                    &function_ty,
                    index,
                    *param,
                    &field_substitution,
                    MutablePrefix::Apply,
                )
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
                        .map_or(*target_param, |decl| {
                            self.source_class_substituted_ty(source, decl.ty)
                        })
                };
                let arg_source_text = if self.param_passed_by_mut_ref(&function_ty, index) {
                    RustExpr::loose(format!("*smelt_arg{index}")).cloned_value()
                } else {
                    RustExpr::path(format!("smelt_arg{index}")).cloned_value()
                };
                let arg_text = self.rendered_at_type(
                    &arg_source_text,
                    *target_param,
                    source_param,
                    &self.render_scope(),
                )?;
                if !dispatches_to_source_field
                    && let Some(source_local) =
                        source_function.params.get(index.saturating_add(1)).copied()
                    && self.parameter_needs_mutable_reference_in(source_function, source_local)
                {
                    if self.param_passed_by_mut_ref(&function_ty, index) {
                        Ok(RustExpr::path(format!("smelt_arg{index}")))
                    } else {
                        let local_name = format!("smelt_arg_{index}");
                        arg_preludes.push(format!(
                            "let mut {local_name} = {arg_text};",
                            arg_text = arg_text.delimited()
                        ));
                        Ok(RustExpr::loose(format!("&mut {local_name}")))
                    }
                } else {
                    Ok(arg_text)
                }
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let method_name = self.method_rust_name(method)?;
        let receiver_mut = if method_mutates_this(source_function) {
            "mut "
        } else {
            ""
        };
        let receiver_value = if let SlotReceiver::WeakSelf { handle } = receiver {
            format!("::std::rc::Rc::downgrade(&{handle}.0)")
        } else if direct_source_function.is_some()
            || inherited_source_function.is_some()
            || dispatches_to_source_field
        {
            // An inherited method is emitted into the source class's own `impl`
            // (`classes::effective_class_methods`), so the source value answers
            // it too. Dispatching it on a default TARGET instance instead called
            // the base's method on a fresh object, or an abstract one that does
            // not exist (`StrictYear` converted to `Parser<number>` called
            // `Parser::parse`, E0599).
            "smelt_struct_value.clone()".to_owned()
        } else {
            let target_ty = self.rendered_type(target)?;
            format!("<{target_ty} as Default>::default()")
        };
        let receiver_rebind = match receiver {
            SlotReceiver::WeakSelf { .. } => {
                "Self(smelt_virtual_receiver.upgrade().expect(\"a virtual method slot outlived its instance\"))"
            }
            SlotReceiver::SourceValue => "smelt_virtual_receiver.clone()",
        };
        let call = if dispatches_to_source_field {
            if args.is_empty() {
                RustExpr::atom(format!(
                    "{{ let smelt_callable = {source_field_base}.{method_name}.clone(); smelt_callable() }}"
                ))
            } else {
                RustExpr::atom(format!(
                    "{{ let smelt_callable = {source_field_base}.{method_name}.clone(); smelt_callable({args}) }}"
                ))
            }
        } else if args.is_empty() {
            RustExpr::chain(format!("smelt_method_receiver.{method_name}()"))
        } else {
            RustExpr::chain(format!("smelt_method_receiver.{method_name}({args})"))
        };
        let source_can_throw = !dispatches_to_source_field
            && source_function.can_throw
            && !source_function.is_generator;
        let source_return_ty = if dispatches_to_source_field {
            function_ty.return_ty
        } else {
            self.source_class_substituted_ty(source, source_function.return_ty)
        };
        let adjusted_call = if source_can_throw && function_ty.may_throw {
            call
        } else if source_can_throw {
            call.postfix(".unwrap_or_else(|_| Default::default())")
        } else {
            call
        };
        let body = self.rendered_at_type(
            &adjusted_call,
            source_return_ty,
            function_ty.return_ty,
            &self.render_scope(),
        )?;
        let return_ty = if function_ty.may_throw {
            format!(
                "Result<{}, {ERROR_BOX}>",
                self.rendered_type(function_ty.return_ty)?
            )
        } else {
            self.rendered_type(function_ty.return_ty)?.into_string()
        };
        let wrapped_body = if function_ty.may_throw && !source_can_throw {
            format!("Ok::<_, {ERROR_BOX}>({body})", body = body.delimited())
        } else {
            body.into_string()
        };
        let prelude = if arg_preludes.is_empty() {
            String::new()
        } else {
            format!("{} ", arg_preludes.join(" "))
        };
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_virtual_receiver = {receiver_value}; let smelt_virtual_method: ::std::rc::Rc<dyn Fn({param_types}) -> {return_ty}> = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let {receiver_mut}smelt_method_receiver = {receiver_rebind}; {prelude}{wrapped_body} }}); smelt_virtual_method }}"
        ))))
    }

    /// Return the MIR class described by a class type.
    fn class_for_type(&self, ty: TypeId) -> Option<&MirClass> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        crate::classes::find_class(self.mir, *name)
    }

    /// Find a concrete method implementation on a class or its base chain.
    fn find_class_method_function(
        &self,
        class_name: Symbol,
        method: Symbol,
    ) -> Option<&MirFunction> {
        let class = crate::classes::find_class(self.mir, class_name)?;
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
        let class = crate::classes::find_class(self.mir, class_name)?;
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
}

/// Which value a virtual method slot closure dispatches on.
pub(super) enum SlotReceiver {
    /// A strong copy of the value being converted (`smelt_struct_value`): the
    /// slot of a base-typed record built from a subclass instance.
    SourceValue,
    /// A `Weak` handle on a freshly allocated reference-class instance, named
    /// `handle`; see `FunctionEmitter::bind_own_virtual_slots_expr`.
    WeakSelf {
        /// The Rust name of the new instance handle.
        handle: String,
    },
}
