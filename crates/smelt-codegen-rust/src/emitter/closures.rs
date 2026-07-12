//! Closure rendering engine: emits Rust closure literals and their block/match bodies for MIR closures, including extra-parameter adapters, await detection, and closure operand text.

use super::*;
use smelt_hir::FunctionType;
use rendered_text_rewrite::{replace_shared_capture_uses, shared_capture_cell_name};

impl FunctionEmitter<'_> {
    /// Converts a non-escaping MIR closure into a Rust closure literal.
    pub(super) fn closure_text(&self, id: smelt_mir::ClosureId) -> Result<String, EmitError> {
        self.closure_text_with_extra_params(id, &[], &[], None)
    }

    /// Whether a contextual `target_ty` re-types a closure parameter whose own
    /// MIR local (`local_ty`) dropped an optional wrapper.
    ///
    /// This is true when `target_ty` is `Optional(inner)` and either:
    /// * `inner` is (structurally) the same type as the flat `local_ty` — the
    ///   source spelled the param optional (`x?: T`) but the closure body's own
    ///   local is the bare `T`; or
    /// * the closure body already erased the param to `Unknown`, in which case
    ///   the body treats it dynamically and unwrapping any optional erased
    ///   contextual value is lossless.
    ///
    /// Restricting to these cases keeps the emitted body rebind a lossless
    /// `unwrap_or`, so we never re-type a parameter whose contextual type is a
    /// genuinely different shape (which would break the body).
    pub(super) fn optional_target_matches_flat_local(
        &self,
        target_ty: TypeId,
        local_ty: TypeId,
    ) -> bool {
        let Some(Type::Optional(inner)) = self.mir.types.get(target_ty) else {
            return false;
        };
        *inner == local_ty
            || self.mir.types.get(*inner) == self.mir.types.get(local_ty)
            || matches!(self.mir.types.get(local_ty), Some(Type::Unknown))
    }

    /// Converts a MIR closure into a Rust closure literal shaped for `dest_ty`.
    pub(super) fn closure_text_for_type(
        &self,
        id: smelt_mir::ClosureId,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        // A JS callback often accepts fewer arguments than the contextual
        // function type provides (e.g. `.map(value => ...)` is called with
        // `(value, index, array)`). The emitted closure appends one ignored
        // parameter per surplus contextual argument; capture the *types* of
        // those surplus parameters so the emitted `_arg{i}` can be annotated
        // (an unannotated ignored param is uninferable — E0282 — when the
        // closure is stored behind an `Rc`/`dyn Fn` that erases its signature).
        let extra_param_tys: Vec<TypeId> = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => {
                let closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                function
                    .params
                    .iter()
                    .skip(closure.params.len())
                    .copied()
                    .collect()
            }
            _ => Vec::new(),
        };
        let has_captures = !self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .is_empty();
        let captures_borrowed_callback = self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .iter()
            .any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            });
        if captures_borrowed_callback
            && self
                .mir
                .closures
                .get(id_index(id.0, "closure id does not fit usize")?)
                .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
                .escapes
            && matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
        {
            return self.default_value(dest_ty);
        }
        let target_return_ty = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => Some(function.return_ty),
            _ => None,
        };
        // The contextual function type may spell an overlapping parameter more
        // loosely than the closure's own MIR param local (e.g. an optional
        // `target?: V` lowers to `Option<SmeltUnknown>` in the destination
        // `dyn Fn` while the closure body declares its param as bare
        // `SmeltUnknown`). Thread those target param types into closure-param
        // emission so the emitted signature matches the `dyn Fn` it is cast to;
        // the body rebinds each param back to its own local type.
        let target_param_tys: Vec<TypeId> = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => {
                let closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                function
                    .params
                    .iter()
                    .take(closure.params.len())
                    .copied()
                    .collect()
            }
            _ => Vec::new(),
        };
        let closure = self.closure_text_with_extra_params(
            id,
            &extra_param_tys,
            &target_param_tys,
            target_return_ty,
        )?;
        if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
            let adjusted_closure = if !has_captures
                || closure.starts_with("move ")
                || closure.starts_with('{')
            {
                closure
            } else {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let mut cloned_captures = HashSet::new();
                let mut shared_replacements = Vec::new();
                let capture_prelude = source_closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                        {
                            return None;
                        }
                        if !matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .declared_locals
                                .borrow()
                                .contains(&capture.source_local)
                        {
                            return None;
                        }
                        let name = self.local_name(capture.source_local).ok()?.to_owned();
                        if name.starts_with("(*smelt_capture_") {
                            return None;
                        }
                        cloned_captures.insert(name.clone()).then(|| {
                            if self.closure_capture_needs_shared_access(source_closure, capture)
                                || self.local_uses_shared_capture_storage(capture.source_local)
                            {
                                shared_replacements.push((
                                    name.clone(),
                                    format!("(*smelt_capture_{name}.borrow_mut())"),
                                ));
                                return format!(
                                    "let smelt_capture_{name} = smelt_capture_{name}.clone();"
                                );
                            }
                            // Mutability is driven purely by whether the closure
                            // body actually writes through the capture. A blanket
                            // `mut` for every list/set/dict capture made almost
                            // every collection binding `mut` regardless of use;
                            // `closure_capture_body_writes` now recognizes every
                            // write shape (including callback/mut-ref forwarding),
                            // so read-only captures stay non-`mut`.
                            let mutability = if self.mutable_locals.contains(&capture.source_local)
                                || source_closure
                                    .captures
                                    .iter()
                                    .find(|candidate| {
                                        candidate.source_local == capture.source_local
                                    })
                                    .is_some_and(|candidate| {
                                        self.closure_capture_body_writes(source_closure, candidate)
                                    }) {
                                "mut "
                            } else {
                                ""
                            };
                            format!("let {mutability}{name} = {name}.clone();")
                        })
                    })
                    .collect::<Vec<_>>();
                let adjusted_closure = replace_shared_capture_uses(closure, &shared_replacements);
                if capture_prelude.is_empty() {
                    format!("move {adjusted_closure}")
                } else {
                    format!(
                        "{{\n    {}\n    move {adjusted_closure}\n}}",
                        capture_prelude.join("\n    ")
                    )
                }
            };
            let callback_text = format!("::std::rc::Rc::new({adjusted_closure})");
            if let Some(Type::Function(function)) = self.mir.types.get(dest_ty)
                && self.is_erased_unknown_rest_function(function)
                && !function.may_throw
            {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let params = source_closure
                    .params
                    .iter()
                    .map(|param| {
                        let local_index =
                            id_index(param.0, "closure param index does not fit usize")?;
                        source_closure
                            .locals
                            .get(local_index)
                            .map(|local| local.ty)
                            .ok_or_else(|| EmitError::new("closure param has no local declaration"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?;
                let source_function = FunctionType {
                    params,
                    rest: source_closure.rest,
                    required_params: source_closure.required_params,
                    mutable_params: Vec::new(),
                    return_ty: source_closure.return_ty,
                    is_async: false,
                    may_throw: source_closure.can_throw,
                };
                let args = self.function_args_from_smelt_args_text(&source_function)?;
                let call = if source_closure.can_throw {
                    format!(
                        "(smelt_callback)({args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                    )
                } else {
                    format!("(smelt_callback)({args})")
                };
                let unknown_ty = self.type_id(Type::Unknown)?;
                let return_text =
                    if self.mir.types.get(source_closure.return_ty) == Some(&Type::None) {
                        format!("{{ {call}; SmeltUnknown::Null }}")
                    } else {
                        self.value_at_type_text(&call, source_closure.return_ty, unknown_ty)?
                    };
                let length = source_closure
                    .required_params
                    .unwrap_or_else(|| source_closure.rest.unwrap_or(source_closure.params.len()));
                let erased_function = format!(
                    "SmeltErasedFunction {{ callback: {{ let smelt_callback = ::std::rc::Rc::new({adjusted_closure}); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}) }}, length: {length}.0, object: None }}"
                );
                // A bare function-item-as-value wrapper carries a stable item
                // key. Building this `SmeltErasedFunction` inline would mint a
                // fresh callback `Rc` on every call, so two calls of the same
                // named function constant (e.g. `doNothing()`) would never be
                // `Rc::ptr_eq`. JavaScript returns one shared singleton instead.
                // Route the build through a per-item accessor that caches ONE
                // `SmeltErasedFunction`; every call returns clones sharing one
                // inner callback `Rc`. User arrows have no key and keep the
                // inline fresh build (each arrow expression is a distinct
                // function in JavaScript).
                if let Some(key) = source_closure.function_item_key {
                    self.context
                        .function_item_erased_fn_accessors
                        .borrow_mut()
                        .entry(key)
                        .or_insert(erased_function);
                    return Ok(format!("__smelt_fn_erased_{key}()"));
                }
                return Ok(erased_function);
            }
            return Ok(callback_text);
        }
        Ok(closure)
    }

    /// Emits ignored trailing parameters when a JS callback accepts fewer
    /// arguments than the contextual function type provides.
    ///
    /// `extra_param_tys` carries the contextual types of those surplus
    /// parameters (e.g. the `index: number` and `array: T[]` a `.map` callback
    /// ignores). Each is emitted as an annotated `_arg{i}: <ty>` binding so the
    /// closure signature is fully typed even when it is stored behind an erased
    /// `Rc`/`dyn Fn` where Rust cannot otherwise infer the ignored parameters.
    fn closure_text_with_extra_params(
        &self,
        id: smelt_mir::ClosureId,
        extra_param_tys: &[TypeId],
        target_param_tys: &[TypeId],
        return_override: Option<TypeId>,
    ) -> Result<String, EmitError> {
        let closure = self
            .mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?;
        if closure.escapes
            && closure.captures.iter().any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            })
        {
            let return_ty = return_override.unwrap_or(closure.return_ty);
            let mut param_decls = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let local_index = id_index(param.0, "closure param index does not fit usize")?;
                    let local = closure
                        .locals
                        .get(local_index)
                        .ok_or_else(|| EmitError::new("closure param has no local declaration"))?;
                    // The body ignores these params (it returns a default
                    // value), so honour the contextual type when the
                    // destination `dyn Fn` dropped an optional wrapper the
                    // closure local kept flat (see the main path below).
                    let param_ty = target_param_tys
                        .get(index)
                        .copied()
                        .filter(|target_ty| {
                            self.optional_target_matches_flat_local(*target_ty, local.ty)
                        })
                        .unwrap_or(local.ty);
                    Ok(format!(
                        "arg{index}: {}",
                        self.type_text_with_impl_trait(param_ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            for (index, extra_ty) in extra_param_tys.iter().enumerate() {
                param_decls.push(format!(
                    "_arg{index}: {}",
                    self.type_text_with_impl_trait(*extra_ty, false)?
                ));
            }
            return Ok(format!(
                "|{}| -> {} {{ {} }}",
                param_decls.join(", "),
                self.type_text_with_impl_trait(return_ty, false)?,
                self.default_value(return_ty)?
            ));
        }
        let closure_param_names = closure
            .params
            .iter()
            .enumerate()
            .map(|(index, _)| format!("closure_arg_{index}"))
            .collect::<HashSet<_>>();
        let mut capture_aliases = HashMap::new();
        let body = {
            let mut closure_locals = closure.locals.clone();
            let fallback_span = closure_locals.first().map_or(
                Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
                |local| local.span,
            );
            for capture in &closure.captures {
                for captured_local in [Some(capture.source_local), capture.target_local]
                    .into_iter()
                    .flatten()
                {
                    let index = id_index(captured_local.0, "local index does not fit usize")?;
                    if closure_locals.len() <= index {
                        closure_locals.resize(
                            index.saturating_add(1),
                            LocalDecl {
                                ty: capture.ty,
                                kind: LocalKind::Temp,
                                span: fallback_span,
                            },
                        );
                        if let Some(local) = closure_locals.get_mut(index) {
                            local.ty = capture.ty;
                        }
                    }
                }
            }
            let function = MirFunction {
                id: FuncId(u32::MAX),
                name: Symbol(u32::MAX),
                type_params: Vec::new(),
                origin: HirOrigin::Body(smelt_hir::BodyId(u32::MAX)),
                is_async: false,
                is_test: false,
                can_throw: closure.can_throw,
                params: closure.params.clone(),
                rest: closure.rest,
                return_ty: return_override.unwrap_or(closure.return_ty),
                locals: closure_locals,
                blocks: closure.blocks.clone(),
                entry: closure.entry,
            };
            let mut emitter = FunctionEmitter::new(self.mir, self.context, &function)?;
            // The closure is emitted inline inside the enclosing function, so
            // the enclosing function's in-scope type parameters remain visible
            // in the generated Rust. Propagate them so `T`-typed closure
            // parameters keep their generic shape instead of erasing to
            // `SmeltUnknown` (which would otherwise force the enclosing generic
            // signature to erase as well). The enclosing set is already gated by
            // that function's own erasure decision.
            emitter.enclosing_type_params = self.current_function_type_params();
            for (index, param) in closure.params.iter().enumerate() {
                emitter.names.insert(*param, format!("closure_arg_{index}"));
            }
            for capture in &closure.captures {
                if let Some(target) = capture.target_local {
                    let source = self.local_decl(capture.source_local)?;
                    let source_name = self.local_name(capture.source_local)?.to_owned();
                    let alias_name = if closure_param_names.contains(&source_name) {
                        capture_aliases
                            .entry(capture.source_local)
                            .or_insert_with(|| format!("smelt_captured_{source_name}"))
                            .clone()
                    } else {
                        source_name.clone()
                    };
                    if matches!(self.mir.types.get(source.ty), Some(Type::Function(_)))
                        && matches!(source.kind, LocalKind::Param { .. })
                        && !self.function_parameter_requires_owned(capture.source_local)?
                    {
                        emitter.borrowed_callback_names.insert(alias_name.clone());
                    }
                    let capture_name = if alias_name.starts_with("(*smelt_capture_") {
                        // The source binding is itself captured from an
                        // enclosing shared closure and already renders as
                        // `(*smelt_capture_x.borrow_mut())`. Reuse that rendered
                        // form; wrapping it again would emit the invalid
                        // `(*smelt_capture_(*smelt_capture_x.borrow_mut())...)`.
                        alias_name
                    } else if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        format!("(*smelt_capture_{alias_name}.borrow_mut())")
                    } else {
                        alias_name
                    };
                    emitter.names.insert(target, capture_name);
                    emitter.mark_local_declared(target);
                }
            }
            // When the contextual `dyn Fn` spells a parameter with a different
            // (looser) type than the closure's own MIR local, emit the closure
            // parameter with the contextual type and rebind it to the local
            // type in a body prelude. This keeps the closure signature
            // assignable to the target `dyn Fn` while the body continues to see
            // its own local type. Rebinds are collected so they precede the body.
            let mut param_rebinds = String::new();
            let mut params = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let local = emitter.local_decl(*param)?;
                    let needs_mut = emitter.local_binding_needs_mut(*param);
                    let mutability = if needs_mut { "mut " } else { "" };
                    let name = emitter.local_name(*param)?.to_owned();
                    let local_ty_text = emitter.type_text_with_impl_trait(local.ty, false)?;
                    // Only re-type a parameter when the contextual function
                    // dropped an optional wrapper the closure's own local kept
                    // flat (source `target?: V` lowers the destination param to
                    // `Option<Inner>` while the closure body declares `target`
                    // as the bare `Inner`). Restricting to this dropped-optional
                    // case keeps the coercion a lossless `unwrap_or` and avoids
                    // re-typing params whose contextual type is a genuinely
                    // different shape (which would break the body).
                    let retype = target_param_tys
                        .get(index)
                        .copied()
                        .filter(|target_ty| {
                            emitter.optional_target_matches_flat_local(*target_ty, local.ty)
                        });
                    match retype {
                        Some(target_ty) => {
                            let target_ty_text =
                                emitter.type_text_with_impl_trait(target_ty, false)?;
                            let coerced = emitter.value_at_type_text(
                                &name,
                                target_ty,
                                local.ty,
                            )?;
                            param_rebinds.push_str(&format!(
                                "let {mutability}{name}: {local_ty_text} = {coerced};\n"
                            ));
                            Ok(format!("{name}: {target_ty_text}"))
                        }
                        None => Ok(format!("{mutability}{name}: {local_ty_text}")),
                    }
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            for (index, extra_ty) in extra_param_tys.iter().enumerate() {
                params.push(format!(
                    "_arg{index}: {}",
                    emitter.type_text_with_impl_trait(*extra_ty, false)?
                ));
            }
            let params_text = params.join(", ");
            let mut body_text = param_rebinds;
            emitter.emit_mutable_local_preludes(&mut body_text)?;
            emitter.emit_closure_block(emitter.entry_block()?, &mut body_text)?;
            let returns_future = matches!(
                emitter.mir.types.get(function.return_ty),
                Some(Type::Future(_))
            );
            // Whether this closure needs the async wrapper is decided from MIR,
            // not from scanning the emitted text: a closure is async when its
            // declared return type is a future, or when its own body performs an
            // `await` (nested Promise-continuation closures are separate MIR
            // functions and are excluded by `closure_body_awaits`).
            let awaits_inside_body = emitter.closure_body_awaits();
            if returns_future || awaits_inside_body {
                let output_ty = match emitter.mir.types.get(function.return_ty) {
                    Some(Type::Future(item)) => *item,
                    _ => function.return_ty,
                };
                let output_text = emitter.type_text_with_impl_trait(output_ty, false)?;
                let return_ty = format!("Result<{output_text}, Box<dyn std::error::Error>>");
                // Whether `smelt_async_value` is itself a future to await is read
                // from the MIR return operand types, not from the emitted text.
                let async_value_needs_await = emitter.closure_yields_future_value()?;
                let return_value = if closure.can_throw && async_value_needs_await {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "let smelt_async_output = smelt_async_value?.await; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_output.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "let smelt_async_output = smelt_async_value?.await; Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_output)"
                        )
                    }
                } else if closure.can_throw {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value?.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_value?)"
                        )
                    }
                } else if matches!(
                    emitter.mir.types.get(output_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || emitter.is_erased_class_type(output_ty)
                {
                    if async_value_needs_await {
                        "let smelt_async_output = smelt_async_value.await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_output.into_smelt_unknown())".to_owned()
                    } else {
                        "Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value.into_smelt_unknown())".to_owned()
                    }
                } else if async_value_needs_await {
                    format!(
                        "let smelt_async_output = smelt_async_value.await?; Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_output)"
                    )
                } else {
                    format!("Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_value)")
                };
                // Pin the type of the `smelt_async_value` binding for the fallible,
                // non-awaited branches. When the closure body only ever throws
                // (every path is `return Err(..)`), the block's `Ok` type cannot be
                // inferred from the body alone, so `smelt_async_value?` and the
                // following `.into_smelt_unknown()` fail with E0282. This mirrors the
                // explicit return-type annotation the synchronous fallible-closure
                // path already emits: those branches apply `?` directly to
                // `smelt_async_value`, so the block is a `Result<Ok, Box<dyn Error>>`
                // whose `Ok` type equals the erased `SmeltUnknown` or the concrete
                // `output_text` the `Ok::<..>` returns already use.
                let async_value_annotation: Option<String> =
                    if closure.can_throw && !async_value_needs_await {
                        let ok_ty_text = if matches!(
                            emitter.mir.types.get(output_ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        ) || emitter.is_erased_class_type(output_ty)
                        {
                            "SmeltUnknown"
                        } else {
                            output_text.as_str()
                        };
                        Some(format!(": Result<{ok_ty_text}, Box<dyn std::error::Error>>"))
                    } else {
                        None
                    };
                let async_value_annotation = async_value_annotation.unwrap_or_default();
                let mut cloned_async_captures = HashSet::new();
                let async_capture_lines = closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .function_parameter_requires_owned(capture.source_local)
                                .ok()?
                        {
                            return None;
                        }
                        let source_name = self.local_name(capture.source_local).ok()?.to_owned();
                        let name = capture_aliases
                            .get(&capture.source_local)
                            .cloned()
                            .unwrap_or_else(|| source_name.clone());
                        // A capture that lives in shared `Rc<RefCell>` storage is
                        // read through `(*smelt_capture_x.borrow_mut())` in the
                        // body. The `async move` block must own its own `Rc` clone
                        // of that cell, so clone the `smelt_capture_x` HANDLE into a
                        // fresh binding of the same name. Re-binding the source name
                        // here instead would be rewritten by the enclosing wrapper's
                        // `replace_shared_capture_uses` into `let
                        // (*smelt_capture_x.borrow_mut()) = ...` — a place
                        // expression in a binding position, which is not a pattern.
                        // The shared-storage decision mirrors the capture-name
                        // rewrite above so prelude and body agree.
                        if self.closure_capture_needs_shared_access(closure, capture)
                            || self.local_uses_shared_capture_storage(capture.source_local)
                        {
                            let cell = format!("smelt_capture_{name}");
                            return cloned_async_captures
                                .insert(cell.clone())
                                .then(|| format!("let {cell} = {cell}.clone();"));
                        }
                        cloned_async_captures
                            .insert(name.clone())
                            .then(|| format!("let {name} = {source_name}.clone();"))
                    })
                    .collect::<Vec<_>>();
                let async_capture_prelude = if async_capture_lines.is_empty() {
                    String::new()
                } else {
                    format!("{} ", async_capture_lines.join(" "))
                };
                format!(
                    "|{params_text}| {{ {async_capture_prelude}SmeltFuture::from_future(Box::pin(async move {{\n        let smelt_async_value{async_value_annotation} = {{\n{body_text}        }};\n        {return_value}\n    }}) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = {return_ty}>>>) }}"
                )
            } else if function.can_throw {
                // A fallible closure returns `Result<T, Box<dyn Error>>`. When its
                // body only ever throws (every path is `return Err(..)`), neither
                // the `Ok` type nor the error type can be inferred from the body
                // alone (E0282/E0283). Spell the return type explicitly so both
                // parameters are pinned; the `Ok::<T, _>` returns emitted for this
                // closure already use this same `T`, so the annotation is exact.
                let return_ty_text =
                    emitter.type_text_with_impl_trait(function.return_ty, false)?;
                format!(
                    "|{params_text}| -> Result<{return_ty_text}, Box<dyn std::error::Error>> {{\n{body_text}    }}"
                )
            } else {
                // Shared captures are emitted as safe `Rc<RefCell<T>>` and accessed via
                // `borrow_mut()` (see core.rs), so the closure body contains no `unsafe`
                // operations. Emit the body directly; wrapping it in an `unsafe` block only
                // produced `unnecessary unsafe block` warnings in generated crates.
                format!("|{params_text}| {{\n{body_text}    }}")
            }
        };
        let capture_prefix = if closure.escapes
            || matches!(self.mir.types.get(closure.return_ty), Some(Type::Future(_)))
            || !closure.captures.is_empty()
            || closure.captures.iter().any(|capture| {
                capture.mode == smelt_hir::CaptureMode::ByValue
                    && !self
                        .capture_is_borrowed_callback_param(capture.source_local)
                        .unwrap_or(false)
                    && !self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            }) {
            "move "
        } else {
            ""
        };
        if capture_prefix.is_empty() {
            return Ok(body);
        }
        let mut cloned_captures = HashSet::new();
        let mut shared_replacements = Vec::new();
        let capture_prelude = closure
            .captures
            .iter()
            .filter_map(|capture| {
                let local = self.local_decl(capture.source_local).ok()?;
                if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                    && matches!(local.kind, LocalKind::Param { .. })
                    && !self
                        .function_parameter_requires_owned(capture.source_local)
                        .ok()?
                {
                    return None;
                }
                let source_name = self.local_name(capture.source_local).ok()?.to_owned();
                let name = capture_aliases
                    .get(&capture.source_local)
                    .cloned()
                    .unwrap_or_else(|| source_name.clone());
                if let Some(cell_ref) = shared_capture_cell_name(&source_name) {
                    // The source binding already renders through shared storage
                    // (it was captured from an enclosing shared closure). The
                    // nested closure body references the same `smelt_capture_x`
                    // cell directly, so clone the `Rc` into this closure's
                    // header; otherwise `move` steals the enclosing closure's
                    // cell and later uses of it fail to borrow-check.
                    let cell = cell_ref.to_owned();
                    return cloned_captures
                        .insert(cell.clone())
                        .then(|| format!("let {cell} = {cell}.clone();"));
                }
                cloned_captures.insert(name.clone()).then(|| {
                    if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        shared_replacements.push((
                            name.clone(),
                            format!("(*smelt_capture_{name}.borrow_mut())"),
                        ));
                        return format!("let smelt_capture_{name} = smelt_capture_{name}.clone();");
                    }
                    // See the sibling capture-prelude above: mutability follows
                    // real writes only, no blanket `mut` for collection types.
                    let mutability = if self.mutable_locals.contains(&capture.source_local)
                        || self.closure_capture_body_writes(closure, capture)
                    {
                        "mut "
                    } else {
                        ""
                    };
                    format!("let {mutability}{name} = {source_name}.clone();")
                })
            })
            .collect::<Vec<_>>();
        let adjusted_body = replace_shared_capture_uses(body, &shared_replacements);
        if capture_prelude.is_empty() {
            Ok(format!("{capture_prefix}{adjusted_body}"))
        } else {
            Ok(format!(
                "{{\n    {}\n    {capture_prefix}{adjusted_body}\n}}",
                capture_prelude.join("\n    ")
            ))
        }
    }

    /// Return whether this closure's own MIR body performs an `await`.
    ///
    /// The decision is read from MIR types rather than emitted text: a closure
    /// awaits when any of its own basic blocks contains an `Await` terminator or
    /// an `Rvalue::Await` statement. Nested closures (Promise continuations,
    /// `async move` blocks) are separate `MirFunction`s referenced through
    /// `Rvalue::Closure` and are never inlined into `self.function.blocks`, so
    /// their awaits are naturally excluded — this is the structural, formatting-
    /// independent replacement for the old brace-counting text scan.
    pub(super) fn closure_body_awaits(&self) -> bool {
        self.function.blocks.iter().any(|block| {
            let terminator_awaits = matches!(&block.terminator, Some(Terminator::Await { .. }));
            let statement_awaits = block.statements.iter().any(|statement| {
                matches!(
                    statement,
                    Statement::Assign {
                        value: Rvalue::Await(_),
                        ..
                    }
                )
            });
            terminator_awaits || statement_awaits
        })
    }

    /// Return whether the value this closure yields is itself a future.
    ///
    /// An async wrapper stores the closure body's result in `smelt_async_value`.
    /// When that value is already a future (the closure returns a `new Promise`
    /// or another future without awaiting it first) the wrapper must `.await`
    /// it before producing the resolved output. This is decided from the MIR
    /// return operand types: the closure yields a future iff every `Return`
    /// terminator carries an operand whose type is `Type::Future(_)`.
    ///
    /// Returns `false` when the closure has no `Return` terminator so the
    /// wrapper falls back to treating the body value as already-resolved, which
    /// matches the previous text-based behaviour for those degenerate shapes.
    pub(super) fn closure_yields_future_value(&self) -> Result<bool, EmitError> {
        let mut saw_return = false;
        for block in &self.function.blocks {
            if let Some(Terminator::Return(operand)) = &block.terminator {
                saw_return = true;
                if !matches!(
                    self.mir.types.get(self.operand_ty(operand)?),
                    Some(Type::Future(_))
                ) {
                    return Ok(false);
                }
            }
        }
        Ok(saw_return)
    }

    /// Emits a closure block with return terminators scoped to the closure body.
    pub(super) fn emit_closure_block(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        self.emit_closure_block_inner(block, out, &mut Vec::new(), None, false)
    }

    /// Emits a closure body MIR `Return` as an explicit `return` statement.
    ///
    /// Used both for the direct loop-exit return and for any `Return` reached
    /// while emitting a branch nested inside a loop-shaped closure body. Inside a
    /// `loop { ... }` a value can only leave the closure through an explicit
    /// `return`; a bare tail expression would instead type the loop-body block as
    /// its value type and mismatch the required `()`.
    fn emit_closure_return_statement(
        &self,
        operand: &Operand,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if self.function.return_ty == self.none_ty {
            if !matches!(operand, Operand::Const(Constant::None))
                && self.operand_ty(operand)? != self.none_ty
            {
                out.push_str(&format!("    {};\n", self.operand_text(operand)?));
            }
            if self.function.can_throw {
                out.push_str("    return Ok::<(), Box<dyn std::error::Error>>(());\n");
            } else {
                out.push_str("    return ();\n");
            }
            return Ok(());
        }
        let operand_is_future = matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Future(_))
        );
        let return_ty = match self.mir.types.get(self.function.return_ty) {
            Some(Type::Future(item)) if !operand_is_future => *item,
            _ => self.function.return_ty,
        };
        let value = self.value_at_type(operand, return_ty)?;
        if self.function.can_throw {
            let return_ty_text = self.type_text_with_impl_trait(return_ty, false)?;
            out.push_str(&format!(
                "    return Ok::<{return_ty_text}, Box<dyn std::error::Error>>({value});\n"
            ));
        } else {
            out.push_str(&format!("    return {value};\n"));
        }
        Ok(())
    }

    /// Emits a closure block while guarding against unstructured CFG cycles.
    ///
    /// `in_loop` is set while emitting the branches of a loop-shaped closure
    /// body. In that context a MIR `Return` must be lowered to an explicit
    /// `return` statement rather than a bare Rust tail expression, because a
    /// value produced as the tail of a `loop { ... }` body block would mistype
    /// the block as its value type instead of the required `()`.
    fn emit_closure_block_inner(
        &self,
        block: &BasicBlock,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
        in_loop: bool,
    ) -> Result<(), EmitError> {
        if Some(block.id) == stop {
            return Ok(());
        }
        let block_ptr = std::ptr::from_ref(block);
        if active.contains(&block_ptr) {
            out.push_str("    panic!(\"recursive closure control flow is not structured yet\")\n");
            return Ok(());
        }
        active.push(block_ptr);
        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("closure basic block has no terminator"));
        };
        if let Terminator::Switch {
            cond,
            then_block,
            else_block,
        } = terminator
            && self.block_can_reach(*then_block, block.id, &mut HashSet::new())
        {
            out.push_str("    loop {\n");
            for statement in &block.statements {
                self.emit_statement(statement, out)?;
            }
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            // Both branches run inside the generated `loop`, so any MIR `Return`
            // they reach must become an explicit `return` (see `in_loop`). A
            // branch that instead flows back to the header (`stop`) emits nothing
            // and the loop naturally continues.
            self.emit_closure_block_inner(
                self.block(*then_block)?,
                out,
                active,
                Some(block.id),
                true,
            )?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_closure_block_inner(
                self.block(*else_block)?,
                out,
                active,
                Some(block.id),
                true,
            )?;
            out.push_str("    }\n");
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            active.pop();
            return Ok(());
        }
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        let result = match terminator {
            Terminator::Return(operand) if in_loop => {
                // A `Return` reached inside a loop-shaped closure body can only
                // leave the closure through an explicit `return`; the enclosing
                // `loop { ... }` body must stay typed `()`.
                self.emit_closure_return_statement(operand, out)?;
                Ok(())
            }
            Terminator::Return(operand) => {
                if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None))
                        && self.operand_ty(operand)? != self.none_ty
                    {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                    if self.function.can_throw {
                        out.push_str("    Ok::<(), Box<dyn std::error::Error>>(())\n");
                    } else {
                        out.push_str("    ()\n");
                    }
                } else {
                    // For an async closure whose declared return is `Future<inner>`
                    // the surrounding async wrapper stores this body value in
                    // `smelt_async_value` and awaits it when it is itself a future
                    // (see `closure_yields_future_value`). Deciding the tail type
                    // from MIR keeps the future flowing through: when the returned
                    // operand is already a future we yield it at the `Future` type,
                    // so a `new Promise(...)` body returns the future rather than
                    // collapsing to the inner type's default (the old null / `0.0`
                    // fallthrough that the textual promotion pass patched up after
                    // the fact). Only when the operand is a resolved value do we
                    // strip to the inner type as before.
                    let operand_is_future = matches!(
                        self.mir.types.get(self.operand_ty(operand)?),
                        Some(Type::Future(_))
                    );
                    let return_ty = match self.mir.types.get(self.function.return_ty) {
                        Some(Type::Future(item)) if !operand_is_future => *item,
                        _ => self.function.return_ty,
                    };
                    let value = self.value_at_type(operand, return_ty)?;
                    if self.function.can_throw {
                        let return_ty_text = self.type_text_with_impl_trait(return_ty, false)?;
                        out.push_str(&format!(
                            "    Ok::<{return_ty_text}, Box<dyn std::error::Error>>({value})\n"
                        ));
                    } else {
                        out.push_str(&format!("    {value}\n"));
                    }
                }
                Ok(())
            }
            Terminator::Goto(target) => {
                if Some(*target) == stop {
                    Ok(())
                } else {
                    self.emit_closure_block_inner(self.block(*target)?, out, active, stop, in_loop)
                }
            }
            Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let call_text = self.closure_call_text_for_dest(callee, args, local.ty)?;
                let name = self.local_name(*dest)?;
                out.push_str(&format!(
                    "    let {name}: {} = {call_text};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop, in_loop)
            }
            Terminator::Await {
                future,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let value = format!("{}.await", self.await_operand_text(future)?);
                out.push_str(&format!(
                    "    let {name}: {} = {value};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop, in_loop)
            }
            Terminator::Switch {
                cond,
                then_block,
                else_block,
            } => {
                let branch_declared = self.declared_locals_snapshot();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
                self.emit_closure_block_inner(
                    self.block(*then_block)?,
                    out,
                    active,
                    stop,
                    in_loop,
                )?;
                out.push_str("    } else {\n");
                self.restore_declared_locals(branch_declared.clone());
                self.emit_closure_block_inner(
                    self.block(*else_block)?,
                    out,
                    active,
                    stop,
                    in_loop,
                )?;
                out.push_str("    }\n");
                self.restore_declared_locals(branch_declared);
                Ok(())
            }
            Terminator::Match {
                scrutinee,
                arms,
                default,
            } => self.emit_closure_match(scrutinee, arms, *default, out, active, stop, in_loop),
            Terminator::Throw(operand) => {
                // Same rationale as the function-level throw terminator: pin the
                // `Result` error type to `Box<dyn std::error::Error>` at the `Err`
                // construction so a throwing closure never leaves `E` ambiguous.
                out.push_str(&format!(
                    "    return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into());\n",
                    self.operand_text(operand)?
                ));
                Ok(())
            }
            Terminator::Unreachable => {
                out.push_str("    unreachable!()\n");
                Ok(())
            }
        };
        active.pop();
        result
    }

    /// Emits a MIR match inside a Rust closure body.
    ///
    /// Closure blocks can end as Rust tail expressions, unlike ordinary function
    /// blocks that usually emit explicit `return` statements. This helper keeps
    /// shared match join blocks outside the generated `match` expression and
    /// lets non-joining matches remain the closure tail expression.
    fn emit_closure_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
        in_loop: bool,
    ) -> Result<(), EmitError> {
        let join = self.match_join(arms, default)?;
        let scrutinee_text = self.match_scrutinee_text(scrutinee)?;
        out.push_str(&format!("    match {scrutinee_text} {{\n"));
        let match_declared = self.declared_locals_snapshot();
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text(&arm.label)
            ));
            self.emit_closure_block_inner(self.block(arm.target)?, out, active, join, in_loop)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared.clone());
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_closure_block_inner(self.block(default_block)?, out, active, join, in_loop)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared);
        } else {
            out.push_str("        _ => {}\n");
        }
        if join.is_some() {
            out.push_str("    };\n");
        } else {
            out.push_str("    }\n");
        }
        if let Some(join_block) = join {
            self.emit_closure_block_inner(self.block(join_block)?, out, active, stop, in_loop)?;
        }
        Ok(())
    }

    /// Resolve a callback operand to the Rust closure expression that constructed it.
    pub(super) fn closure_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == local
                {
                    return self.closure_text(*id);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Resolve a local to the closure local it aliases, following simple copy assignments.
    fn closure_source_local(&self, local: LocalId) -> LocalId {
        let mut current = local;
        for _ in 0_u8..8 {
            let mut next = None;
            for block in &self.function.blocks {
                for statement in &block.statements {
                    if let Statement::Assign {
                        dest,
                        value:
                            Rvalue::Use(
                                Operand::Copy(Place::Local(source))
                                | Operand::Move(Place::Local(source)),
                            ),
                    } = statement
                        && *dest == current
                    {
                        next = Some(*source);
                    }
                }
            }
            let Some(source) = next else {
                return current;
            };
            if source == current {
                return current;
            }
            current = source;
        }
        current
    }

    /// Resolve a callback operand to a Rust closure shaped like its function type.
    ///
    /// JavaScript array callbacks may declare fewer formal parameters than the
    /// runtime supplies. The declared operand type records the contextual
    /// callback shape, so this path asks closure emission to add ignored
    /// trailing parameters before iterator code calls the callback with
    /// `(item, index, array)` or `(acc, item, index, array)`.
    pub(super) fn closure_operand_text_for_declared_type(
        &self,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        let dest_ty = self.operand_ty(operand)?;
        let source_local = self.closure_source_local(local);
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == source_local
                {
                    return self.closure_text_for_type(*id, dest_ty);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }
}
