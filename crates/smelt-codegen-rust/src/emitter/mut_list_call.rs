//! Passing a list to a callee that mutates it: the adapter that writes the
//! element back through the caller's place, whatever storage holds it.

use super::*;
use crate::rust::{RustExpr, RustShape};

impl FunctionEmitter<'_> {
    /// Builds a convert-in-place adapter block for a static call that forwards
    /// a `&mut` list argument whose element type differs from the callee's
    /// emitted parameter element type.
    ///
    /// A generic caller such as `pull<T>(arr: &mut SmeltList<T>)` may delegate
    /// to an erased monomorphization `pull_127(arr: &mut SmeltList<SmeltUnknown>)`.
    /// Rust `&mut` references are invariant in their element type, so
    /// `&mut SmeltList<T>` cannot be passed where `&mut SmeltList<SmeltUnknown>`
    /// is expected. This emits a block that:
    ///   1. builds an erased temporary list from the argument's current
    ///      contents (each element coerced to the callee's element type),
    ///   2. passes `&mut temp` to the callee,
    ///   3. writes the (possibly mutated) temp elements back through the
    ///      original `&mut` argument, converting each element back to the
    ///      caller's element type, and
    ///   4. converts the callee's returned value to the destination type.
    ///
    /// Returns `None` when the call has no writable local list argument passed
    /// to a `&mut` parameter of the callee that needs element conversion,
    /// leaving the ordinary call path untouched.
    pub(super) fn static_call_mut_list_adapter(
        &self,
        func: FuncId,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize")?)
            .ok_or_else(|| EmitError::new("call references an unknown function"))?;
        // Throwing calls are emitted through a dedicated terminator path; keep
        // this adapter to the ordinary (non-throwing) statement path.
        if function.can_throw && !function.is_generator {
            return Ok(None);
        }
        if args.len() != function.params.len() {
            return Ok(None);
        }
        let rust_function_name = self.function_rust_name(function)?;
        let emitted_params =
            self.emitted_function_param_types(&self.emitted_signature_key(function)?)?;
        // Resolve each argument's effective (emitted) target parameter type.
        let target_tys = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let emitted = emitted_params
                    .as_ref()
                    .and_then(|params| params.get(index).copied());
                match emitted {
                    Some(ty) => Ok(ty),
                    None => Ok(self.function_local_decl(function, *param)?.ty),
                }
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        // Detect which arguments need the convert-in-place mutable-list adapter.
        // `callee_generics` is the set of type parameters the callee actually emits
        // as real Rust generics; it is empty for an erased monomorphization, so
        // the same set both renders the callee's parameter types and decides
        // whether an argument monomorphizes them. It is an *emission scope*, not
        // a call-site binding: `crate::generic_bindings` cannot express it, and
        // the post-AST increment must not conflate the two.
        let callee_generics = self.callee_free_function_type_params(function);
        // This adapter renders callback arguments without a call-site binding map
        // (see the `Type::Function` arm of the render loop below), so it can only
        // handle callees whose callback parameters mention none of the type
        // parameters the callee actually emits. When one does, decline the
        // adapter and let the ordinary call path — which computes
        // `static_call_monomorphization` and threads those bindings into every
        // callback renderer — emit the call instead.
        if !callee_generics.is_empty()
            && target_tys.iter().any(|&target_ty| {
                matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                    && self.type_mentions_any(target_ty, &callee_generics)
            })
        {
            return Ok(None);
        }
        let caller_scope = self.current_function_type_params();
        let mut needs_adapter = false;
        for (index, arg) in args.iter().enumerate() {
            if self
                .mut_list_adapter_arg(function, index, arg, target_tys[index], &callee_generics)?
                .is_some()
            {
                needs_adapter = true;
            }
        }
        if !needs_adapter {
            return Ok(None);
        }
        let _boundary = self.context.carrier_boundary();
        let mut prelude = String::new();
        let mut rendered_args = Vec::with_capacity(args.len());
        let mut writebacks = String::new();
        // Set when an argument monomorphizes the callee's generics, recording the
        // callee parameter type it instantiated and the concrete type it bound;
        // the return conversion needs both.
        let mut monomorphized_arg: Option<(TypeId, TypeId)> = None;
        for (index, arg) in args.iter().enumerate() {
            let target_ty = target_tys[index];
            if let Some((place, kind)) =
                self.mut_list_adapter_arg(function, index, arg, target_ty, &callee_generics)?
            {
                let access = self.mut_list_place_access(&place)?;
                let place_ty = self.place_ty(&place)?;
                let temp = format!("smelt_mut_arg_{index}");
                match kind {
                    MutListArgKind::Monomorphized => {
                        // The callee's parameter is its own generic list shape, so
                        // Rust binds the callee's type parameter from the caller's
                        // list and no element conversion happens in either
                        // direction. Borrow the place itself where that is safe —
                        // the callee then mutates the caller's list directly, with
                        // no copy and no write-back. A shared capture cannot hold
                        // its borrow guard across the call, so it goes through a
                        // temporary at the caller's own list type instead.
                        if let Some(borrow) = access.mut_borrow {
                            rendered_args.push(format!("&mut {borrow}"));
                        } else {
                            let temp_ty = self.rust_type(
                                place_ty,
                                false,
                                &TypeSubstitution::lexical(&caller_scope),
                            )?;
                            prelude.push_str(&format!(
                                "let mut {temp}: {temp_ty} = {}; ",
                                access.read
                            ));
                            rendered_args.push(format!("&mut {temp}"));
                            writebacks
                                .push_str(&format!("{} = {temp}; ", access.assign_target));
                        }
                        monomorphized_arg = Some((target_ty, place_ty));
                    }
                    MutListArgKind::Erased => {
                        let arg_item = self.list_element_ty(place_ty)?;
                        let caller_elem = self.rust_type(
                            arg_item,
                            false,
                            &TypeSubstitution::lexical(&caller_scope),
                        )?;
                        // The callee renders its erased element as `SmeltUnknown`;
                        // build the temporary at the callee's rendered parameter
                        // type so the `&mut` reborrow is type-correct.
                        // Rendered in the CALLEE's emission scope, which is
                        // empty here precisely because the callee erased this
                        // element. The emptiness is the callee's, not the
                        // caller's; naming it keeps that deliberate.
                        let temp_ty =
                            self.rust_type(target_ty, false, &TypeSubstitution::erased())?;
                        prelude.push_str(&self.carrier_format(self.carrier()?, format_args!(
                            "let mut {temp}: {temp_ty} = {}.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<{temp_ty}>(); ",
                            access.read
                        )));
                        rendered_args.push(format!("&mut {temp}"));
                        // The temporary is a converted copy, so the callee's
                        // writes come back through the caller's own buffer:
                        // every alias of this JS array must observe them, which
                        // rebinding the place to a new list would not do.
                        writebacks.push_str(&self.carrier_format(self.carrier()?, format_args!(
                            "{}.replace_all({temp}.into_iter().map(|smelt_element| <{caller_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<Vec<_>>()); ",
                            access.read
                        )));
                    }
                    MutListArgKind::UnionInjected => {
                        let arg_item = self.list_element_ty(place_ty)?;
                        let target_item = self.list_element_ty(target_ty)?;
                        let scope = TypeSubstitution::lexical(&caller_scope);
                        let temp_ty = self.rust_type(target_ty, false, &scope)?;
                        let render_scope = self.render_scope();
                        let inject = self.rendered_at_type(
                            &RustExpr::path("smelt_element"),
                            arg_item,
                            target_item,
                            &render_scope,
                        )?;
                        let project = self.rendered_at_type(
                            &RustExpr::path("smelt_element"),
                            target_item,
                            arg_item,
                            &render_scope,
                        )?;
                        prelude.push_str(&format!(
                            "let mut {temp}: {temp_ty} = {}.into_iter().map(|smelt_element| {inject}).collect::<{temp_ty}>(); ",
                            access.read, inject = inject.delimited()
                        ));
                        rendered_args.push(format!("&mut {temp}"));
                        writebacks.push_str(&format!(
                            "{}.replace_all({temp}.into_iter().map(|smelt_element| {project}).collect::<Vec<_>>()); ",
                            access.read, project = project.delimited()
                        ));
                    }
                }
            } else if matches!(self.mir.types.get(target_ty), Some(Type::Function(_))) {
                // No call-site bindings on this path: the mutable-list adapter
                // runs its OWN unifier (`mut_list_adapter_arg`) rather than
                // `static_call_monomorphization`. Two unifiers deciding one call
                // site is the §4.1 hazard, so this path never guesses a
                // substitution. The guard above makes that safe rather than
                // merely hopeful: it already declined the adapter for any callee
                // whose callback parameter mentions a type parameter the callee
                // emits, so the callback reaching here has a declared type with
                // nothing to substitute, and the caller's own lexical scope is
                // the right (and unchanged) environment to render it in.
                rendered_args.push(self.borrowed_function_argument_expr(arg, target_ty, None)?.into_string());
            } else {
                rendered_args.push(self.value_at_type(arg, target_ty)?.into_string());
            }
        }
        let arg_values = rendered_args.join(", ");
        // Convert the callee's returned value to the destination type. The callee
        // erases its list element to `SmeltUnknown` while the destination keeps
        // the caller's element type, so a matching per-element un-erasure is
        // applied when the returned value is a list whose rendered element type
        // differs from the destination's.
        let return_ty = self
            .emitted_function_return_type(&self.emitted_signature_key(function)?)
            .unwrap_or(function.return_ty);
        let dest_is_list = matches!(self.mir.types.get(dest_ty), Some(Type::List(_)));
        let result_expr = match monomorphized_arg {
            // The callee's declared return type is literally the parameter type
            // this argument instantiated (`pullAt<T>(arr: T[]): T[]`), so at this
            // call site the returned value already has the argument's own list
            // type. Convert from that concrete type, not from the unbound generic
            // (which renders as `SmeltUnknown` and would emit a bogus extraction
            // against concrete elements). A non-list destination — a discarded
            // (void) call — has nothing to convert to.
            Some((param_ty, arg_ty)) if return_ty == param_ty && dest_is_list => {
                self.rendered_at_type(&RustExpr::path("smelt_mut_call_result"), arg_ty, dest_ty, &self.render_scope())?
            }
            // Any other return position that mentions the callee's generics is
            // likewise monomorphized here, and the destination local already
            // carries the substituted type from the frontend (the same reasoning
            // `call_emitted_source_ty` applies to type-parameter returns), so the
            // value passes through unchanged.
            Some(_) if self.type_mentions_any(return_ty, &callee_generics) => {
                RustExpr::path("smelt_mut_call_result")
            }
            // A fully concrete return position from a generic callee needs the
            // ordinary destination coercion.
            Some(_) => self.rendered_at_type(&RustExpr::path("smelt_mut_call_result"), return_ty, dest_ty, &self.render_scope())?,
            None => self.mut_list_adapter_return_expr(
                &RustExpr::path("smelt_mut_call_result"),
                return_ty,
                dest_ty,
                &caller_scope,
            )?,
        };
        Ok(Some(RustExpr::atom(format!(
            "{{ {prelude}let smelt_mut_call_result = {rust_function_name}({arg_values}); {writebacks}{result_expr} }}", result_expr = result_expr.delimited()
        ))))
    }

    /// Converts the value returned by a mutable-list adapter call to the
    /// destination type. When both the callee's rendered return type and the
    /// destination are lists whose rendered element types differ (the callee
    /// erased its element to `SmeltUnknown` while the destination keeps the
    /// caller's generic element), each element is un-erased through
    /// `SmeltFromUnknown`. Otherwise the value is returned unchanged.
    fn mut_list_adapter_return_expr(
        &self,
        value_text: &RustExpr,
        return_ty: TypeId,
        dest_ty: TypeId,
        caller_scope: &HashSet<Symbol>,
    ) -> Result<RustExpr, EmitError> {
        let (Some(Type::List(_)), Some(Type::List(dest_item))) =
            (self.mir.types.get(return_ty), self.mir.types.get(dest_ty))
        else {
            return Ok(value_text.to_owned());
        };
        // Deliberately two different environments in one comparison: the
        // callee's rendering (erased, hence the empty substitution) against the
        // caller's lexical one. Rendering both sides the same way would make the
        // equality below always hold and silently drop the un-erasure.
        let return_render = self
            .rust_type(return_ty, false, &TypeSubstitution::erased())?
            .into_string();
        let dest_render = self
            .rust_type(dest_ty, false, &TypeSubstitution::lexical(caller_scope))?
            .into_string();
        if return_render == dest_render {
            return Ok(value_text.to_owned());
        }
        let dest_elem =
            self.rust_type(*dest_item, false, &TypeSubstitution::lexical(caller_scope))?;
        Ok(RustExpr::atom_on(&value_text, self.carrier_format(self.carrier()?, format_args!(
            "{value_text}.into_iter().map(|smelt_element| <{dest_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<SmeltList<_>>()", value_text = value_text.tight()
        ))))
    }

    /// Returns the place of the `index`-th argument, and how its elements must be
    /// treated, when it needs the convert-in-place mutable-list adapter.
    ///
    /// The adapter applies when the callee needs a mutable reference to a list
    /// parameter, the argument is a writable local list place, and the caller
    /// renders the list element type differently from the callee. Because `&mut`
    /// references are invariant such a call cannot be emitted as a plain
    /// reborrow, and emitting it as `&mut <converted temporary>` would silently
    /// discard the callee's mutation.
    ///
    /// The two rendered-difference shapes need opposite treatment, so the result
    /// carries a [`MutListArgKind`]: a callee that emits real Rust generics binds
    /// its type parameter from the argument and must see the elements unchanged
    /// (`Monomorphized`), while an erased monomorphization needs each element
    /// converted in and back out (`Erased`).
    fn mut_list_adapter_arg(
        &self,
        function: &MirFunction,
        index: usize,
        arg: &Operand,
        target_ty: TypeId,
        callee_generics: &HashSet<Symbol>,
    ) -> Result<Option<(Place, MutListArgKind)>, EmitError> {
        let Some(param) = function.params.get(index).copied() else {
            return Ok(None);
        };
        if !self.parameter_needs_mutable_reference_in(function, param) {
            return Ok(None);
        }
        let local = match arg {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => return Ok(None),
        };
        let place = Place::Local(local);
        if !self.mut_list_place_is_writable(local)? {
            return Ok(None);
        }
        let arg_ty = self.place_ty(&place)?;
        let (Some(Type::List(arg_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(arg_ty), self.mir.types.get(target_ty))
        else {
            return Ok(None);
        };
        // Compare rendered element types: the MIR element types can be identical
        // (a shared `TypeParam`) while the caller keeps it generic and the callee
        // erases it. The callee renders its element with generics only when it
        // emits real Rust generics, which is what `callee_generics` encodes.
        let caller_scope = self.current_function_type_params();
        let arg_element = self.rust_type(
            *arg_item,
            false,
            &TypeSubstitution::lexical(&caller_scope),
        )?;
        let param_element = self.rust_type(
            *target_item,
            false,
            &TypeSubstitution::callee_emission(callee_generics),
        )?;
        if arg_element.as_str() == param_element.as_str() {
            // Already invariance-compatible: the ordinary call path passes the
            // place (or the forwarded reference) straight through.
            return Ok(None);
        }
        // The callee emits real generics and this argument is exactly its generic
        // list shape instantiated, so Rust infers the type parameter from the
        // caller's concrete list and no element conversion is possible *or*
        // wanted. This reuses the same instantiation rule the ordinary
        // pass-through path uses for by-value generic arguments.
        if !callee_generics.is_empty()
            && self.generic_param_instantiated_by(arg_ty, target_ty, callee_generics)
        {
            return Ok(Some((place, MutListArgKind::Monomorphized)));
        }
        // The erasing adapter converts elements with `IntoSmeltUnknown` on the way
        // in and `SmeltFromUnknown` on the way out, so it can only target a
        // parameter whose element really is the erased `SmeltUnknown`. Any other
        // rendered difference (a callee element that is itself a narrower generated
        // type, such as `SmeltErasedFunction`) has no such element bridge; leave
        // those on the ordinary call path rather than emit conversions that cannot
        // type-check.
        if param_element.shape() == RustShape::Erased {
            return Ok(Some((place, MutListArgKind::Erased)));
        }
        if self
            .concrete_union_members(*target_item)
            .is_some_and(|members| members.contains(arg_item))
        {
            return Ok(Some((place, MutListArgKind::UnionInjected)));
        }
        Ok(None)
    }

    /// Returns how a mutable-list argument's local place is stored in Rust.
    ///
    /// See [`MutListPlaceStorage`]; the classification drives both the writable
    /// check and the read/assign spellings.
    fn mut_list_place_storage(&self, local: LocalId) -> Result<MutListPlaceStorage, EmitError> {
        if self.function.id.0 != u32::MAX
            && self.function.params.contains(&local)
            && self.parameter_needs_mutable_reference(local)
        {
            return Ok(MutListPlaceStorage::Reference);
        }
        if self.local_shared_cell(local)?.is_some() {
            return Ok(MutListPlaceStorage::SharedCapture);
        }
        Ok(MutListPlaceStorage::Owned)
    }

    /// Returns whether the adapter may write a new list back through `local`.
    ///
    /// A `&mut` parameter and a shared closure capture are always writable (the
    /// first through its reference, the second through the cell's `borrow_mut()`,
    /// neither of which needs a `mut` binding). An owned local is only writable
    /// when its `let` is actually rendered `mut`; `local_binding_needs_mut` is the
    /// single source of truth for that, and it already forces `mut` for any local
    /// passed to a mutable-reference parameter of a static call — exactly this
    /// shape — so the check is a self-consistency guard rather than a
    /// restriction. Inside a closure body the emitter runs on a synthetic
    /// function whose mutability analysis does not cover the enclosing binding,
    /// so only the shared-capture storage is accepted there.
    fn mut_list_place_is_writable(&self, local: LocalId) -> Result<bool, EmitError> {
        match self.mut_list_place_storage(local)? {
            MutListPlaceStorage::Reference | MutListPlaceStorage::SharedCapture => Ok(true),
            MutListPlaceStorage::Owned => {
                Ok(self.function.id.0 != u32::MAX && self.local_binding_needs_mut(local))
            }
        }
    }

    /// Returns the read expression and assignment target for a mutable-list
    /// argument place.
    ///
    /// `place_expr` renders a read whose shape depends on the storage, so the
    /// adapter cannot assume a reference: `(*array).clone()` on an owned
    /// `SmeltList` would deref to a slice and clone that instead of the list, and
    /// `*array = ..` does not compile on a non-reference at all.
    fn mut_list_place_access(&self, place: &Place) -> Result<MutListPlaceAccess, EmitError> {
        let Place::Local(local) = place else {
            return Err(EmitError::new(
                "mutable-list adapter place must be a local",
            ));
        };
        let text = self.place_expr(place)?;
        match self.mut_list_place_storage(*local)? {
            MutListPlaceStorage::Reference => Ok(MutListPlaceAccess {
                read: format!("(*{text}).clone()", text = text.tight()),
                assign_target: format!("*{text}", text = text.tight()),
                mut_borrow: Some(format!("*{text}", text = text.tight())),
            }),
            MutListPlaceStorage::SharedCapture => {
                let cell = self
                    .local_shared_cell(*local)?
                    .ok_or_else(|| EmitError::new("shared-capture place lost its cell binding"))?
                    .cell;
                Ok(MutListPlaceAccess {
                    // `text` is already `(*smelt_capture_x.borrow())`, so the read
                    // clones through the shared borrow; the write needs its own
                    // short-lived `borrow_mut()` of the same cell.
                    read: format!("{text}.clone()", text = text.tight()),
                    assign_target: format!("*{cell}.borrow_mut()"),
                    mut_borrow: None,
                })
            }
            MutListPlaceStorage::Owned => Ok(MutListPlaceAccess {
                read: format!("{text}.clone()", text = text.tight()),
                assign_target: text.clone().into_string(),
                mut_borrow: Some(text.into_string()),
            }),
        }
    }

    /// Returns the element type of a list type, or an error when the type is
    /// not a list.
    pub(super) fn list_element_ty(&self, list_ty: TypeId) -> Result<TypeId, EmitError> {
        match self.mir.types.get(list_ty) {
            Some(Type::List(item)) => Ok(*item),
            _ => Err(EmitError::new("expected a list type")),
        }
    }
}

/// How a mutable-list argument needs its elements treated at a call site.
///
/// A `&mut SmeltList<..>` argument only type-checks when the caller's rendered
/// element type matches the callee's, because Rust `&mut` references are
/// invariant. There are two ways to reach that state and they are opposites, so
/// the convert-in-place adapter classifies every mutable-list argument into one
/// of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MutListArgKind {
    /// The callee emits real Rust generics and its parameter is that generic
    /// list shape instantiated by this argument (`arr: &mut SmeltList<T>`
    /// receiving a `SmeltList<f64>`). Rust binds the callee's type parameter
    /// from the argument, so the elements must pass through *unconverted*:
    /// erasing them would monomorphize the callee at `SmeltUnknown` and clash
    /// with the concrete destination.
    Monomorphized,
    /// The callee renders its element at a different concrete type (typically an
    /// erased `SmeltUnknown` monomorphization). `&mut` invariance rules out a
    /// direct reborrow, so each element is converted on the way in and back out.
    Erased,
    /// The callee's element is a concrete generated UNION the caller's element
    /// injects into (`&mut SmeltList<SmeltUnion>` — `(string | Promise<string>)[]`
    /// — receiving a `string[]`, Hono's `escapeToBuffer(str, buf)`). Each element
    /// is injected on the way in and projected back to the caller's element on
    /// the way out, so the callee's writes reach the caller's list.
    UnionInjected,
}

/// How a mutable-list argument's caller-side place is spelled in Rust.
///
/// The adapter both *reads* the place's current contents into a temporary and
/// *writes* the mutated contents back, and `place_expr` renders only a read
/// expression whose shape depends on how the binding is stored. The three
/// storages need three different spellings, so mixing them up is a silent
/// miscompile rather than a type error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MutListPlaceStorage {
    /// A forwarded `&mut` parameter of the current function: `place_expr` is the
    /// reference itself (`arr`), so the value lives behind a deref.
    Reference,
    /// A shared closure capture: `place_expr` is `(*smelt_capture_x.borrow())`,
    /// and the write needs a fresh `borrow_mut()` of the same cell.
    SharedCapture,
    /// An ordinary owned local: `place_expr` is already the value.
    Owned,
}

/// The read expression and assignment target for a mutable-list argument place.
struct MutListPlaceAccess {
    /// Expression producing an owned clone of the list currently in the place.
    read: String,
    /// Place expression the mutated list is assigned back to.
    assign_target: String,
    /// Place expression that may be borrowed mutably *for the whole call*, when
    /// that is safe. `None` for a shared capture, whose `RefCell` guard must not
    /// stay alive across the callee (a callback could re-enter the same cell and
    /// panic with "already borrowed"), so those must go through a temporary.
    mut_borrow: Option<String>,
}
