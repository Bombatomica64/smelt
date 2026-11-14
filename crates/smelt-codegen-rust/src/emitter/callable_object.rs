//! Callable objects: records with a call signature, their `__smelt_call` slot,
//! and the dispatch of a call on a value only known at runtime to be callable.

use super::*;
use crate::rust::RustExpr;

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
    pub(super) fn callable_object_slot_call_expr(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let callee_ty = self.operand_ty(callee)?;
        let Some(call_ty) = self.callable_interface_call_field_ty(callee_ty) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(call_ty).cloned() else {
            return Ok(None);
        };
        // A callee read through a reference class's `RefCell` renders as
        // `recv.0.borrow().f.clone()`, whose `Ref` guard would live to the end
        // of the statement and so across the call. The call is often what
        // `borrow_mut()`s that same cell (Hono's `router.get(..)` slot sets
        // `this.#path`), which panicked "already borrowed" at startup. Binding
        // the slot in its own `let` drops the guard first, the same rule
        // `indirect_call` applies to a plain function-typed field.
        let slot_text = if self.operand_reads_through_ref_cell(callee) {
            format!(
                "{{ let smelt_callable_slot = {}.{SMELT_CALL}.clone(); smelt_callable_slot }}",
                self.operand_expr(callee)?.tight()
            )
        } else {
            format!(
                "{}.{SMELT_CALL}.clone()",
                self.operand_expr(callee)?.tight()
            )
        };
        let (call_text, source_ty) = if self.is_erased_unknown_rest_function(&function)
            && !function.may_throw
        {
            // The slot is a `SmeltErasedFunction`: its ABI takes the argument
            // vector and always answers a bare `SmeltUnknown`.
            let rendered = args
                .iter()
                .map(|arg| self.erase(arg))
                .collect::<Result<Vec<_>, EmitError>>()?;
            (
                RustExpr::atom(slot_text).postfix(format!(".call(vec![{}])", rendered.join(", "))),
                self.type_id(Type::Unknown)?,
            )
        } else {
            // The slot is a Rust closure with the borrowed-callback ABI of
            // any other callable value: a `&T` parameter takes a reference.
            let mut rendered = args
                .iter()
                .zip(function.params.iter())
                .enumerate()
                .map(|(index, (arg, param))| {
                    if self.callback_param_is_shared_reference(&function, index, *param) {
                        self.shared_reference_argument_expr(arg, *param)
                    } else {
                        self.closure_value_argument_expr(arg, *param)
                    }
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            // A call may omit trailing optional parameters; the slot is a
            // Rust closure, which takes every one.
            for (index, param) in function.params.iter().enumerate().skip(args.len()) {
                let padding = self.closure_value_padding_expr(*param)?;
                rendered.push(self.callback_call_arg_expr(
                    &function,
                    index,
                    *param,
                    padding.into_string(),
                ));
            }
            let call = format!("({slot_text})({})", rendered.join(", "));
            let call = if function.may_throw {
                if self.body_can_propagate_error() {
                    format!("{call}?")
                } else {
                    format!("{call}.unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error))")
                }
            } else {
                call
            };
            (RustExpr::from(call), function.return_ty)
        };
        if self.mir.types.get(dest_ty) == Some(&Type::None) {
            return Ok(Some(RustExpr::atom(format!(
                "{{ {call_text}; () }}",
                call_text = call_text.delimited()
            ))));
        }
        Ok(Some(self.rendered_at_type(
            &call_text,
            source_ty,
            dest_ty,
            &self.render_scope(),
        )?))
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
    /// thrown error via `smelt_panic_throw`), and, when the value is not callable,
    /// throws JavaScript's `TypeError: <callee> is not a function` only when
    /// `not_callable` names the callee (`Some`), which is what an erased method
    /// call on a receiver whose runtime value has no such method
    /// (`(5 as any).map(f)`) must do.
    ///
    /// `callee_text` is the expression producing the callee value. `args_expr` is
    /// the expression fed to `Into::into` to build the argument vector; callers
    /// differ only in how they materialize that expression (an explicit
    /// `vec![...]` for `ClosureCall`, a pre-flattened list for
    /// `ClosureCallSpread`), so it is the single parameterized hole in the shared
    /// snippet. A non-callable callee throws `TypeError`; `not_callable` is its
    /// source spelling when one is known (see
    /// [`Self::prototype_method_not_callable`]).
    pub(super) fn dynamic_callable_dispatch_expr(
        &self,
        callee_text: &RustExpr,
        args_expr: &RustExpr,
        not_callable: Option<&str>,
    ) -> Result<RustExpr, EmitError> {
        // `callee_text` is usually already an owned temporary (an operand render
        // clones the local it reads), so take an owned copy rather than
        // deep-copying it a second time.
        let callee_text = &callee_text.cloned_value();
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let smelt_function_value = {callee_text}; let smelt_call_args: Vec<SmeltUnknown> = Into::into({args_expr}); let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"{SMELT_CALL}\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (smelt_function)(smelt_call_args).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)) }} else {{ {fallback} }} }}",
            fallback = not_callable.map_or_else(
                || RustExpr::chain("smelt_value_not_callable()"),
                |description| RustExpr::chain(format!("smelt_throw_not_callable({description:?})"))
            ).delimited(), callee_text = callee_text.delimited(), args_expr = args_expr.delimited(),
        ))))
    }

    /// The `TypeError` spelling for a non-callable callee, when it must throw.
    ///
    /// A callee that is a prototype method READ off an erased value
    /// (`anyValue.at`, resolved at run time by the erased method tables — see
    /// `smelt_stdlib::erased_prototype_methods`) throws JavaScript's
    /// `TypeError: x.at is not a function` when the value has no such method:
    /// that is the behaviour the runtime resolution exists to provide, and the
    /// read answers a callable whenever JavaScript would.
    ///
    /// Every other erased callee answers `None` and throws the same `TypeError`
    /// without a spelling, from the dispatch. The rule is by callee SHAPE, not by
    /// name alone: the chain must end in a field read of a table method.
    ///
    /// JavaScript names the callee the way the source wrote it. MIR keeps no
    /// expression text, but the callee of an erased method call is a compiler
    /// temporary assigned once from a field read (`%t = copy %x.map`, possibly
    /// through a receiver bind and a forwarding copy), so the spelling is
    /// recovered by following those single assignments back to named locals and
    /// field symbols; an unrecoverable base answers the generic `"expression"`.
    pub(super) fn prototype_method_not_callable(&self, callee: &Operand) -> Option<String> {
        let (Operand::Copy(place) | Operand::Move(place)) = callee else {
            return None;
        };
        let field = self.terminal_field_name(place, 0)?;
        if !smelt_stdlib::is_erased_prototype_method(field) {
            return None;
        }
        Some(
            self.place_description(place, 0)
                .unwrap_or_else(|| "expression".to_owned()),
        )
    }

    /// The field name a callee's single-assignment chain ends in, if any.
    fn terminal_field_name(&self, place: &Place, depth: usize) -> Option<&str> {
        match place {
            Place::Field { field, .. } => self.symbol_source_name(*field).ok(),
            Place::Local(local) => {
                let source = self.single_assigned_source(*local, depth)?;
                self.terminal_field_name(source, depth.saturating_add(1))
            }
            Place::Index { .. } | Place::Global { .. } => None,
        }
    }

    /// Recover `name` / `base.field` for a place, following single assignments.
    ///
    /// `depth` bounds the walk so a malformed (cyclic) assignment chain cannot
    /// loop; real chains are a handful of steps.
    fn place_description(&self, place: &Place, depth: usize) -> Option<String> {
        match place {
            Place::Local(local) => self.local_description(*local, depth),
            Place::Field { base, field } => {
                let field_name = self.symbol_source_name(*field).ok()?;
                // A `__smelt_`-prefixed field is a storage key Smelt minted (a
                // symbol-keyed member, say), not a name the source wrote, so
                // spelling it would put an internal key in a user-visible
                // message.
                if field_name.starts_with("__smelt_") {
                    return None;
                }
                Some(format!(
                    "{}.{field_name}",
                    self.local_description(*base, depth)?
                ))
            }
            Place::Index { .. } | Place::Global { .. } => None,
        }
    }

    /// Recover the source spelling of one local: its binding name, or what the
    /// single assignment to a compiler temporary read.
    fn local_description(&self, local: LocalId, depth: usize) -> Option<String> {
        match self.local_decl(local).ok()?.kind {
            LocalKind::UserBinding(symbol)
            | LocalKind::Param {
                symbol: Some(symbol),
            } => {
                return self.symbol_source_name(symbol).ok().map(str::to_owned);
            }
            LocalKind::Param { symbol: None } => return None,
            LocalKind::Temp => {}
        }
        let source = self.single_assigned_source(local, depth)?;
        self.place_description(source, depth.saturating_add(1))
    }

    /// Emits `Object.assign` when the target is a callable JavaScript value.
    ///
    /// JavaScript functions can carry own properties, but Rust closures cannot.
    /// Three representations are emitted: an erased object path (the callable is
    /// kept as the `__smelt_call` entry of a runtime object, with each property
    /// value erased), a typed struct path for a concrete callable-interface
    /// destination (the callable fills `__smelt_call` and each property fills its
    /// like-named field), and a bare-callable fallback otherwise. MIR operands
    /// preserve evaluation of the property expressions before this rvalue is
    /// emitted. This gives the frontend and
    /// MIR a distinct operation for callable object assignment without making
    /// closure calls or function returns stop compiling.
    pub(super) fn callable_object_assign_expr(
        &self,
        callable: &Operand,
        props: &[(Symbol, Operand)],
        spreads: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.has_dynamic_shape(dest_ty) {
            let callable_text = self.erase(callable)?;
            let mut entries = vec![format!(
                "smelt_object.push((JsString::from(\"{SMELT_CALL}\"), {callable_text}));",
                callable_text = callable_text.delimited()
            )];
            for (key, value) in props {
                let key_text = self.symbol_source_name(*key)?;
                let value_text = self.erase(value)?;
                entries.push(format!(
                    "smelt_object.push((JsString::from({key_text:?}), {value_text}));",
                    value_text = value_text.delimited()
                ));
            }
            // Dynamic record sources (`Object.assign(fn, def)` with a variable
            // `def`) contribute their OWN enumerable entries at runtime. Copying
            // them AFTER the literal props preserves JS last-write-wins order,
            // while `__smelt_call` is inserted first and never overwritten by a
            // spread (a callable's own `__smelt_call` slot is not user data).
            for spread in spreads {
                let spread_text = self.erase(spread)?;
                entries.push(self.carrier_format(self.carrier()?, format_args!(
                    "if let SmeltUnknown::Object(smelt_spread) = {spread_text} {{ for (smelt_key, smelt_value) in smelt_spread.iter() {{ if smelt_key != \"{SMELT_CALL}\" {{ smelt_object.push((smelt_key, smelt_value)); }} }} }}", spread_text = spread_text.delimited()
                )));
            }
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let mut smelt_object = Vec::new(); {} SmeltUnknown::Object(SmeltObject::new(smelt_object)) }}",
                entries.join(" ")
            ))));
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
        if let Some(text) = self.callable_object_struct_expr(callable, props, dest_ty)? {
            return Ok(text);
        }
        self.operand_expr(callable)
    }

    /// Build a typed callable-interface struct literal for `CallableObjectAssign`.
    ///
    /// Reuses the same field-iteration contract as
    /// [`Self::record_literal_expr_for_dest`]: the destination's declared fields
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
    fn callable_object_struct_expr(
        &self,
        callable: &Operand,
        props: &[(Symbol, Operand)],
        dest_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        let type_symbol = *name;
        let type_args = args.clone();
        if !self.is_interface_record_type(dest_ty) && !self.mir.has_class(type_symbol) {
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
            .any(|field| self.symbol_name(field.name) == Ok(SMELT_CALL))
        {
            return Ok(None);
        }

        let mut prop_values = HashMap::new();
        for (key, value) in props {
            // Keyed by the *interned* name, the same one the field side reads:
            // a source property `isPending` interns as `is_pending` and records
            // `isPending` as its source spelling, so matching source spelling
            // against interned field name would miss every camel-cased member.
            prop_values.insert(self.context.names().field_text(self.mir, *key), value);
        }

        let mut field_text = Vec::new();
        for field in &fields {
            let raw_name = self.symbol_name(field.name)?;
            let field_name = self.context.names().field_text(self.mir, field.name);
            let value = if raw_name == SMELT_CALL {
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
            field_text.push(format!("{field_name}: {value}", value = value.delimited()));
        }
        self.callable_record_literal(field_text, type_symbol, type_args.len())
            .map(Some)
    }

    /// The struct literal of a callable record, from its rendered `fields`.
    pub(super) fn callable_record_literal(
        &self,
        mut fields: Vec<String>,
        class: Symbol,
        type_arg_count: usize,
    ) -> Result<RustExpr, EmitError> {
        self.push_phantom_field(&mut fields, class, type_arg_count);
        let type_name = self.type_rust_name(class)?;
        Ok(RustExpr::loose(format!(
            "{type_name} {{ {} }}",
            fields.join(", ")
        )))
    }
}
