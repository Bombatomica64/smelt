//! The `erase` verbs: boxing a typed value into `SmeltUnknown`, per type, and the
//! identity keys and object forms a boxed value keeps.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a statically typed operand into a tagged `SmeltUnknown` value.
    ///
    /// The operand-shaped twin of [`Self::erase_value`]; the two must agree
    /// about every `Type`, because the same MIR value reaches one or the other
    /// depending only on whether the caller already had it rendered.
    pub(in crate::emitter) fn erase(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        if matches!(operand, Operand::Const(Constant::Undefined)) {
            return Ok(self.carrier_expr(&RustExpr::undefined()));
        }
        let text = self.operand_expr(operand)?;
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = text.cloned_value();
        let ty = self.operand_ty(operand)?;
        match self.mir.types.get(ty) {
            Some(Type::TypeParam { name }) if !self.current_function_has_type_param(*name) => {
                Ok(text)
            }
            // A `void` value erases to the absent tag, but the expression that
            // produced it still has to RUN: `text` is routinely a call (`let r:
            // unknown = setter(k, v)`), and answering the bare constant deleted
            // the call and its side effects from the program.
            Some(Type::None) if text.is_unit() => Ok(self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null"))),
            Some(Type::None) => Ok(RustExpr::atom(self.carrier_format(format_args!("{{ let _ = {text}; SmeltUnknown::Null }}")))),
            Some(Type::List(item)) if !self.list_items_render_as_unknown(ty, &self.render_scope()) => {
                // A `List<None>` erases its elements to `Null`, but an
                // `[undefined, …]` literal must erase to `Undefined` (the type
                // lost the distinction; only the defining constants carry it).
                // Recover it from the def-site so genuine `null` arrays keep the
                // `Null` path. This mirrors the same recovery in `value_at_type`
                // for the `List<None> -> List<Unknown>` coercion shape.
                let value_wrap = if matches!(self.mir.types.get(*item), Some(Type::None))
                    && self.container_local_all_undefined_constants(operand)?
                {
                    self.carrier_expr(&RustExpr::undefined())
                } else {
                    self.erase_rendered(&RustExpr::path("value"), *item)?
                };
                // The typed list carries its own JS reference id (`SmeltList`),
                // so erasing to `SmeltUnknown::Array` reuses it directly — this is
                // what preserves `===`/`.toBe` identity across an erase/extract
                // round-trip (e.g. `tap`/`forEach` returning their input).
                //
                // The elements are taken through `Into<Vec<_>>` rather than
                // `into_iter()` because `{text}` is an owned list at most sites but a
                // `&SmeltList<_>` wherever the erased value is a by-reference callback
                // parameter (see `callback_param_is_shared_reference`), and
                // `(&SmeltList<T>).into_iter()` yields `&T` — which `{value_wrap}`
                // cannot consume (a primitive `value as f64` is not fixable by any
                // trait impl). `From<SmeltList<T>> for Vec<T>` MOVES the backing
                // storage and `From<&SmeltList<T>> for Vec<T>` clones it, so Rust's own
                // impl selection does the narrowing: the owned path stays copy-free and
                // only the borrowed path pays a clone — which erasing to an owned
                // `SmeltArray` requires anyway.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_l = {text}; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>())) }}"
                ))))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) != Some(&Type::Unknown) =>
            {
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_record = {smelt_owned_text}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_wrap = self.property_key_to_string_expr(&RustExpr::path("key"), *key)?;
                let value_wrap = self.dict_erased_value_wrap(operand, *item)?;
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "SmeltUnknown::Object(SmeltObject::new({text}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))"
                ))))
            }
            Some(Type::Set(item)) => {
                // A `SmeltJsSet`-backed Set carries JS identity: erase through the
                // single `IntoSmeltUnknown` adapter, which stamps the `__smelt_set`
                // marker (and preserves the set's stable object id) so
                // `isSet`/`instanceof Set`/`Object.prototype.toString` recognize the
                // erased value and `SmeltFromUnknown` round-trips it. Mirrors the
                // `SmeltJsMap` erasure. A `HashSet`-backed Set (value-equality
                // primitives) has no such adapter, so it keeps the bare-array
                // projection below.
                if !self.type_is_hash_set_key_safe(*item) {
                    return Ok(RustExpr::postfix_on(&smelt_owned_text, self.carrier_format(format_args!("{smelt_owned_text}.into_smelt_unknown()"))));
                }
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *item)?;
                // A Set erases to an array; like list bindings, the SAME source
                // Set binding erased twice must compare `===` equal (arrays
                // compare by id). `HashSet` has no `as_ptr`, so key the stable id
                // on the binding's own address (`&set`). Temps / fresh sets keep
                // `SmeltArray::new`.
                // The members erase in the set's own insertion order. They used
                // to be sorted by hash key, which was a deterministic answer to
                // a question the old unordered `HashSet` backing could not
                // answer at all; `SmeltPrimSet` keeps the entries, so the
                // erased array reads like the source set (H52).
                if let Some(bare_local) = self.list_local_identity_key(operand)? {
                    return Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "{{ let smelt_list_id = smelt_list_identity(&({bare_local}) as *const _ as *const () as usize); let values = {smelt_owned_text}.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); SmeltUnknown::Array(SmeltArray::with_id(smelt_list_id, values)) }}"
                    ))));
                }
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let values = {smelt_owned_text}.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); SmeltUnknown::Array(values.into()) }}"
                ))))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *inner)?;
                Ok(RustExpr::postfix_on(&smelt_owned_text, self.carrier_format(format_args!(
                    "{smelt_owned_text}.map_or(SmeltUnknown::Undefined, |value| {value_wrap})"
                ))))
            }
            Some(Type::Function(_)) => {
                // A bare function-item-as-value wrapper has a stable item key.
                // Every reference to the same named function value lowers to its
                // own closure, so erasing each one inline would build a distinct
                // `SmeltUnknown::Function` wrapper that compares unequal under
                // JavaScript reference identity (`===`). Route the erased value
                // through a per-item accessor so all references share ONE cached
                // erased value. User arrows have no key and fall through to the
                // ordinary fresh-erasure logic below.
                if let Some((key, accessor_body)) = self.function_item_erased_accessor(operand)? {
                    self.context
                        .function_item_accessors
                        .borrow_mut()
                        .entry(key)
                        .or_insert(accessor_body);
                    return Ok(RustExpr::chain(format!("__smelt_fn_value_{key}()")));
                }
                if let Some(erased_call) = self.erased_call_assignment_expr(operand)? {
                    return Ok(erased_call);
                }
                if let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(operand)?)
                    && self.is_erased_unknown_rest_function(function)
                    && !function.may_throw
                {
                    let function_text = self.operand_expr(operand)?;
                    return Ok(RustExpr::postfix_on(&function_text, self.carrier_format(format_args!("{function_text}.clone().into_smelt_unknown()"))));
                }
                let adapter = self
                    .rest_vector_unknown_adapter(operand)?
                    .unwrap_or_else(|| RustExpr::loose(self.carrier_format(format_args!("::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Null))"))));
                Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::Function({adapter})"))))
            }
            // A generated union erases through `into_smelt_unknown()`, which
            // takes `self` BY VALUE. Every other consuming arm of this match
            // hands it an owned copy (`smelt_owned_text`); this one passed the
            // raw operand text, so erasing a place read moved out of it and the
            // next read of the same local was a use-after-move (E0382). The
            // rendered-value twin `erase_concrete_union_expr` has always owned
            // its source for exactly this reason — erasing a value is an
            // INSPECTION and must not consume it.
            Some(Type::Union(_)) if self.concrete_union_members(self.operand_ty(operand)?).is_some() => {
                Ok(RustExpr::postfix_on(&smelt_owned_text, self.carrier_format(format_args!("{smelt_owned_text}.into_smelt_unknown()"))))
            }
            Some(Type::Future(item)) => {
                if let Some(bare_local) = self.future_local_identity_key(operand)? {
                    return Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "{{ let smelt_promise_id = smelt_promise_identity(&({bare_local}) as *const _ as *const () as usize); SmeltUnknown::Promise(SmeltPromise::pending_with_id(smelt_promise_id)) }}"
                    ))));
                }
                self.promise_future_unknown_expr(&text, *item)
            }
            // Genuine dynamic boundary: a generator crossing into source
            // `unknown` is a live resumable state machine that no concrete
            // type, generated union, or scoped generic can represent on the
            // erased side. The runtime prelude's `IntoSmeltUnknown` adapter
            // reproduces the JavaScript iterator protocol (`next` →
            // `{ value, done }` steps) over the same shared state machine.
            Some(Type::Generator { .. }) => Ok(RustExpr::postfix_on(&smelt_owned_text, self.carrier_format(format_args!("{smelt_owned_text}.into_smelt_unknown()")))),
            Some(Type::Never) | None => Ok(self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null"))),
            // The rest has nothing to read from the operand itself.
            Some(_) => self.erase_value(&RenderedValue::new(text, ty)),
        }
    }

    /// Return the BARE name of a source list local being erased, if any.
    ///
    /// When a list value crosses into `SmeltUnknown::Array` straight from a
    /// `Place::Local`, the local's backing `Vec` is still alive and has a stable
    /// storage address. That address keys [`smelt_list_identity`] so every
    /// erasure of the one binding reuses a single id (arrays compare `===` by
    /// id). The returned name is the local WITHOUT the trailing `.clone()` that
    /// [`Self::operand_expr`] adds, because the identity key reads `&local`
    /// (the live binding) while the erased values come from the cloned text.
    /// Non-local operands (list literals, transform temporaries) return `None`
    /// and keep the fresh-id `SmeltArray::new` path, matching JS semantics where
    /// distinct array expressions are never `===`.
    fn list_local_identity_key(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        // Only real source bindings (params / user `let`/`const`) get stable JS
        // reference identity. A temp holds a fresh intermediate array (e.g. a
        // transform/projection result like `Object.entries(..)`), which JS treats
        // as a new array each time — it keeps the fresh-id `SmeltArray::new` path.
        if matches!(self.local_decl(*local)?.kind, LocalKind::Temp) {
            return Ok(None);
        }
        // The in-scope place, not the local's source name: a renamed binding or a
        // shared capture is not spelled like its source. The identity key reads the
        // live binding
        // (via `.as_ptr()`, which auto-(de)refs both `Vec` and `&Vec` locals);
        // the erased values still come from the cloned text.
        Ok(Some(self.place_expr(&Place::Local(*local))?.into_string()))
    }

    /// Return the BARE name of a source future local being erased, if any.
    ///
    /// Future values cannot be cloned. When the same future local is erased
    /// more than once for identity-only operations, moving it into every
    /// `SmeltPromise::from_future` wrapper would double-move the future. Key a
    /// pending promise identity on the live local address instead; expression
    /// futures still move into an awaitable promise wrapper.
    fn future_local_identity_key(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        if matches!(self.local_decl(*local)?.kind, LocalKind::Temp)
            || !matches!(
                self.mir.types.get(self.local_decl(*local)?.ty),
                Some(Type::Future(_))
            )
        {
            return Ok(None);
        }
        Ok(Some(self.place_expr(&Place::Local(*local))?.into_string()))
    }

    /// The per-entry value erasure for a dict operand crossing into `unknown`.
    ///
    /// Identical to `erase_rendered` except for the one case the type cannot
    /// answer: a `Dict<_, None>` whose defining literal held `undefined`, not
    /// `null`. `{ a: undefined }` must erase to `SmeltUnknown::Undefined` so
    /// `typeof o.a === 'undefined'` and `Object.values(o)[0] !== null` stay
    /// true across the boundary. This is the dict twin of the recovery the list
    /// arm does, sharing
    /// [`Self::container_local_all_undefined_constants`] with it.
    fn dict_erased_value_wrap(
        &self,
        operand: &Operand,
        item: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if matches!(self.mir.types.get(item), Some(Type::None))
            && self.container_local_all_undefined_constants(operand)?
        {
            return Ok(self.carrier_expr(&RustExpr::undefined()));
        }
        self.erase_rendered(&RustExpr::path("value"), item)
    }

    /// Report whether a `Type::None`-holding CONTAINER operand was defined by a
    /// literal whose held values are *all* the `undefined` literal.
    ///
    /// `null` and `undefined` both collapse to MIR `Type::None`, so a
    /// `List<None>`, a `Tuple` of `None` or a `Dict<_, None>` carries no
    /// type-level hint about which JS singleton it holds; the distinction
    /// survives only as the per-value [`Constant::Undefined`] (see
    /// `specs/distinct-undefined.md`). When such a container is erased, the
    /// generic per-type erase would pick `SmeltUnknown::Null` for every held
    /// value — wrong for an `[undefined, …]` / `{ k: undefined }` literal,
    /// whose values must compare equal to the `undefined` any other producer
    /// yields (`typeof o.k === 'undefined'`, `isJSONValue(undefined) === false`).
    ///
    /// We recover the lost distinction by inspecting the operand's *defining*
    /// rvalue (mirroring [`Self::erased_call_assignment_expr`]): if every held
    /// value is the `undefined` constant, the whole container erases to
    /// `Undefined`. Mixed or all-`null` literals keep the historical `Null`
    /// erasure, so genuine `null` containers are untouched, as does any operand
    /// with no single literal definition (the same conservatism the list path
    /// has always had).
    ///
    /// One helper for all three container shapes on purpose: the list arm grew
    /// this recovery first, the dict arm reproduced the bug because it did not,
    /// and a tuple arm would have been the third.
    pub(super) fn container_local_all_undefined_constants(
        &self,
        operand: &Operand,
    ) -> Result<bool, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(false);
        };
        let mut defining_values: Option<Vec<&Operand>> = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign { dest, value } = statement
                    && dest == local
                {
                    // A reassignment of the same local would make the erasure
                    // ambiguous; only trust a single defining container literal.
                    match value {
                        Rvalue::List(items) | Rvalue::Tuple(items) => {
                            defining_values = Some(items.iter().collect());
                        }
                        Rvalue::Dict(entries) => {
                            defining_values =
                                Some(entries.iter().map(|(_, value)| value).collect());
                        }
                        _ => return Ok(false),
                    }
                }
            }
        }
        let Some(values) = defining_values else {
            return Ok(false);
        };
        Ok(!values.is_empty()
            && values
                .iter()
                .all(|value| matches!(value, Operand::Const(Constant::Undefined))))
    }

    /// Re-render a typed callback local from its erased callable source when it
    /// is immediately being boxed back into `SmeltUnknown`.
    ///
    /// Generic JavaScript helpers such as Remeda's purry utilities return
    /// first-class callable values through an erased `unknown` ABI. If codegen
    /// first adapts such a value to a concrete Rust callback and then wraps that
    /// adapter back into `SmeltUnknown::Function`, the adapter's static return
    /// type can erase real dynamic shapes. Reusing the original erased call
    /// preserves the runtime callable and its result.
    pub(in crate::emitter) fn erased_call_assignment_expr(&self, operand: &Operand) -> Result<Option<RustExpr>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        // Re-inlining the defining call here is only sound when its typed-callback
        // binding is *also* suppressed (see
        // `emit_call_terminator_statement`/`emit_statement`). Otherwise the binding
        // is emitted *and* the call is re-rendered at this erase site, evaluating
        // the call twice and double-moving its arguments (E0382). Pair the two
        // decisions: when the binding survives, fall through so the erase reads the
        // existing binding instead of re-inlining.
        if !self.function_call_result_dead_when_erased(*local)? {
            return Ok(None);
        }
        let mut found = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest != local {
                    continue;
                }
                let Rvalue::ClosureCall { .. } = value else {
                    return Ok(None);
                };
                found = Some(value);
            }
        }
        let Some(value) = found else {
            for block in &self.function.blocks {
                if let Some(Terminator::Call {
                    callee, args, dest, ..
                }) = &block.terminator
                    && dest == local
                {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    return Ok(Some(self.call_expr_for_dest(callee, args, unknown_ty)?));
                }
            }
            return Ok(None);
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        Ok(Some(self.rvalue_expr_for_dest(value, unknown_ty)?))
    }

    /// Wrap a rendered value expression with a known static type into `SmeltUnknown`.
    ///
    /// Transitional wrapper: callers that still thread bare `(text, type)` reach
    /// the [`RenderedValue`]-based [`FunctionEmitter::erase_value`] through here
    /// by treating the text as a primary/postfix [`Precedence::Atom`]. New
    /// callers should build a [`RenderedValue`] with the right precedence and
    /// call [`FunctionEmitter::erase_value`] directly so erase can parenthesize
    /// loose operands itself.
    pub(in crate::emitter) fn erase_rendered(
        &self,
        value_text: &RustExpr,
        ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.erase_value(&RenderedValue::new(value_text.clone(), ty))
    }

    /// Box a typed [`RenderedValue`] into the erased `SmeltUnknown` runtime form.
    ///
    /// This is the real per-`Type` mechanics behind the `erase` verb. Unlike the
    /// old text-only entry, the rendered value knows its own precedence, so erase
    /// wraps the operand itself wherever it inlines the value as a cast operand
    /// (`… as f64`) or a method receiver (`.into()`, `.into_iter()`, `.clone()`,
    /// `.into_smelt_unknown()`). For an [`Precedence::Atom`] those wraps are
    /// no-ops, so output stays byte-identical to the historical inlining.
    pub(in crate::emitter) fn erase_value(&self, value: &RenderedValue) -> Result<RustExpr, EmitError> {
        let ty = value.ty();
        let value_text = value.expr();
        // `value_text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_value = value_text.cloned_value();
        match self.mir.types.get(ty) {
            Some(Type::Unknown) => Ok(value_text.to_owned()),
            // Unconditional, and correct for both spellings of a type
            // parameter: a monomorphized `T` converts through the
            // `IntoSmeltUnknown` bound its signature declares, and an erased one
            // is a `SmeltUnknown` whose `IntoSmeltUnknown` impl is the identity.
            // The operand twin (`erase`) has to gate the same conversion on
            // scope because it must not perturb the erased spelling's bytes.
            Some(Type::TypeParam { .. }) => Ok(RustExpr::atom(self.carrier_format(format_args!("({value_text}).into_smelt_unknown()")))),
            // As in `rendered_at_type`: erase the `void` value, but run the
            // expression that produced it.
            Some(Type::None | Type::Never) | None if !value_text.is_unit() => {
                Ok(RustExpr::atom(self.carrier_format(format_args!("{{ let _ = {value_text}; SmeltUnknown::Null }}"))))
            }
            Some(Type::None | Type::Never) | None => Ok(self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null"))),
            Some(Type::Bool) => Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::Bool({value_text})")))),
            Some(Type::BigInt) => Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::BigInt({smelt_owned_value})")))),
            Some(Type::Int | Type::FixedInt(_) | Type::Float) => {
                // Cast operand: a loose top-level operator must be parenthesized
                // so `as f64` does not reassociate across it.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "SmeltUnknown::Number({} as f64)",
                    value.expr().as_cast_operand()
                ))))
            }
            Some(Type::String) => Ok(crate::rust::erased_string(value_text)),
            Some(Type::List(item)) if self.list_items_render_as_unknown(ty, &self.render_scope()) => {
                // Keep the rendered-value and operand erasure paths in agreement:
                // an already-erased generic array aliases the same backing buffer.
                // Boundary, split as in `erase`: see `crate::erased_list_alias`.
                // The helper takes the value as a call argument, so only the
                // direct wrap's method receiver needs a loose operand wrapped.
                if erased_list_alias::leaves_erased_generic(self.mir.types.get(*item)) {
                    Ok(erased_list_alias::array_alias_expr(value_text))
                } else {
                    Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "SmeltUnknown::Array({}.into())",
                        value.parenthesized_if_needed()
                    ))))
                }
            }
            Some(Type::List(item)) => {
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *item)?;
                // Carry the list's own JS reference id across erasure (so an
                // erase/extract round-trip stays identity-stable, e.g. the array
                // a forEach/reduce callback receives `===` the input array).
                //
                // `Into<Vec<_>>` rather than `into_iter()`, for the same reason as the
                // operand-based erasure above: the value can be a `&SmeltList<_>` (a
                // by-reference callback parameter), whose `into_iter()` yields `&T`.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_l = {}; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|value| {value_wrap}).collect::<Vec<_>>())) }}",
                    value.parenthesized_if_needed()
                ))))
            }
            // A source-spelled `Map` erases through its `SmeltJsMap`
            // `IntoSmeltUnknown` adapter, stamping the `__smelt_map` marker so
            // the erased value stays observable as a Map (see the operand-based
            // erasure above).
            Some(Type::JsMap(_, _)) => {
                Ok(RustExpr::postfix_on(&smelt_owned_value, self.carrier_format(format_args!("{smelt_owned_value}.into_smelt_unknown()"))))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "SmeltUnknown::Object(SmeltObject::from_unknown_record({smelt_owned_value}))"
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *item)?;
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_record = {smelt_owned_value}; SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, {value_wrap})).collect())) }}"
                ))))
            }
            Some(Type::Dict(key, item)) => {
                let key_wrap = self.property_key_to_string_expr(&RustExpr::path("key"), *key)?;
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *item)?;
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "SmeltUnknown::Object(SmeltObject::new({}.into_iter().map(|(key, value)| ({key_wrap}, {value_wrap})).collect()))",
                    value.parenthesized_if_needed()
                ))))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(RustExpr::postfix_on(&smelt_owned_value, self.carrier_format(format_args!("{smelt_owned_value}.into_smelt_unknown()"))))
            }
            // A concrete match value crossing into a dynamic `unknown` boundary
            // is erased through the single explicit `IntoSmeltUnknown` adapter,
            // reproducing the JavaScript match-array-with-properties shape.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => {
                Ok(RustExpr::postfix_on(&smelt_owned_value, self.carrier_format(format_args!("{smelt_owned_value}.into_smelt_unknown()"))))
            }
            // See the operand-form arm: the fetch types erase through their own
            // adapter rather than through the declared-field record builder.
            Some(Type::Class { .. }) if self.is_fetch_runtime_class_type(ty)? => {
                Ok(RustExpr::postfix_on(&smelt_owned_value, self.carrier_format(format_args!("{smelt_owned_value}.into_smelt_unknown()"))))
            }
            Some(Type::Class { name, .. })
                if self.is_erased_class_type(ty) && self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Date) =>
            {
                Ok(self.date_unknown_identity_expr(value_text))
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(ty) => Ok(value_text.to_owned()),
            Some(Type::Class { .. }) => self.class_unknown_object_expr(value_text, ty),
            Some(Type::Set(item)) => {
                // See the sibling `Type::Set` arm above: a `SmeltJsSet`-backed Set
                // erases through the `__smelt_set` marker adapter; only a
                // `HashSet`-backed primitive Set falls through to a bare array.
                if !self.type_is_hash_set_key_safe(*item) {
                    return Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "({}).clone().into_smelt_unknown()",
                        value.parenthesized_if_needed()
                    ))));
                }
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *item)?;
                // Insertion order, not hash order: see the sibling arm.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let values = {}.clone().into_iter().map(|value| {value_wrap}).collect::<Vec<_>>(); SmeltUnknown::Array(values.into()) }}",
                    value.parenthesized_if_needed()
                ))))
            }
            Some(Type::Tuple(items)) => {
                let values = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.erase_rendered(
                            &{ let receiver = value.expr(); RustExpr::postfix_on(receiver, format!("{receiver}.{index}.clone()")) }.ending_in_clone(),
                            *item,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::Array(vec![{values}].into())"))))
            }
            Some(Type::Optional(inner)) => {
                let value_wrap = self.erase_rendered(&RustExpr::path("value"), *inner)?;
                Ok(RustExpr::postfix_on(value.expr(), self.carrier_format(format_args!(
                    "{}.clone().map_or(SmeltUnknown::Undefined, |value| {value_wrap})",
                    value.parenthesized_if_needed()
                ))))
            }
            Some(Type::Function(function)) => {
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok(RustExpr::postfix_on(value.expr(), self.carrier_format(format_args!(
                        "{}.clone().into_smelt_unknown()",
                        value.parenthesized_if_needed()
                    ))));
                }
                let args = self.function_args_from_smelt_args_text(function, ErasedCallTargetAbi::Declared)?;
                let call_text = RustExpr::atom(format!("(smelt_function_value)({args})"));
                let return_text = if self.mir.types.get(function.return_ty) == Some(&Type::None) {
                    // A `void`-returning callback erased to a callable value
                    // returns JavaScript `undefined`, not `null`, so `!== undefined`
                    // guards on the result behave as in JS.
                    if function.may_throw {
                        self.carrier_format(format_args!(
                            "{{ {call_text}?; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"
                        ))
                    } else {
                        self.carrier_format(format_args!(
                            "{{ {call_text}; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"
                        ))
                    }
                } else if matches!(self.mir.types.get(function.return_ty), Some(Type::Future(_))) {
                    // A SYNC function that returns a promise and can throw
                    // before it does yields `Result<Future, _>`, so the fallible
                    // call is unwrapped with `?` to recover the bare future
                    // before it is erased into a promise; erasing the `Result`
                    // directly would double-wrap it (the promise task would then
                    // await a `Result<Future, _>`).
                    //
                    // An ASYNC function carries its throw INSIDE the future
                    // (`Future<Output = Result<..>>`) and its call yields the
                    // future itself, so there is nothing to unwrap: a `?` there
                    // is applied to a `SmeltFuture`, which does not implement
                    // `Try` (radash's `async_test.rs`, whose callback became
                    // future-typed once a block-bodied callback's return type
                    // came from its own returns). This is the same rule
                    // `throwing_call_suffix` applies to a direct call.
                    let future_call = if function.may_throw && !function.is_async {
                        RustExpr::atom(format!("{call_text}?"))
                    } else {
                        call_text
                    };
                    let erased_return = self.erase_rendered(&future_call, function.return_ty)?;
                    self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({erased_return})"))
                // A returned value skips the erasure step only when its Rust
                // representation ALREADY IS `SmeltUnknown`. That is what
                // `is_erased_class_type` answers. This used to ask
                // `class_has_no_known_fields`, which answers a different
                // question — "the class declares no fields I can build an
                // object from" — and is true of every MODELED host class too,
                // because a modeled class is not in `mir.classes`. So a
                // callback returning a `Request` or an `ArrayBuffer` had its
                // concrete `SmeltRequest` / `SmeltArrayBuffer` handed straight
                // to `Ok::<SmeltUnknown, _>(..)` (E0308, two sites in Hono).
                // The general arms below already know how each class erases —
                // a fetch runtime class through `into_smelt_unknown`, a `Date`
                // through its identity, any other through its fields — and an
                // erased class's own arm returns the text unchanged, so this
                // branch keeps doing exactly what it did for those.
                } else if self.is_erased_class_type(function.return_ty) {
                    if function.may_throw {
                        call_text.into_string()
                    } else {
                        self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({call_text})"))
                    }
                } else if function.may_throw {
                    let erased_return =
                        self.erase_rendered(&RustExpr::atom(format!("{call_text}?")), function.return_ty)?;
                    self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({erased_return})"))
                } else {
                    let erased_return = self.erase_rendered(&call_text, function.return_ty)?;
                    self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({erased_return})"))
                };
                // Erasing a function value builds a fresh forwarding closure, so
                // the allocation address JavaScript `===` would compare is a new
                // one every time. The adapter and the typed callable it wraps are
                // ONE JavaScript function, so both must resolve to one canonical
                // identity: that is what keeps two erasures of the same closure —
                // a field read through an erased view and the same closure erased
                // into an object literal — `===` to each other, what lets
                // `smelt_same_erased_function` see through both wrappers, and what
                // makes an own property of the function (`f.prototype`) the same
                // object whichever spelling reads it. The rest-vector adapter in
                // `coercion/function_adapter.rs` links itself the same way. The identity is taken
                // before the callable is moved into the adapter.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_function_value = {value_text}; if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) {{ smelt_callable_object }} else {{ let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function_value); let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, {ERROR_BOX}>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); smelt_link_function_identity_key(&smelt_erased_function, smelt_origin_identity); SmeltUnknown::Function(smelt_erased_function) }} }}"
                ))))
            }
            Some(Type::Union(_)) if self.concrete_union_members(ty).is_some() => {
                Ok(RustExpr::postfix_on(&value_text, self.carrier_format(format_args!("{value_text}.into_smelt_unknown()"))))
            }
            Some(Type::Union(_)) => Ok(value_text.to_owned()),
            Some(Type::Future(item)) => self.promise_future_unknown_expr(value_text, *item),
            // Same dynamic boundary as the operand-based erase above: route the
            // generator through the prelude's `IntoSmeltUnknown` iterator
            // adapter rather than failing emission.
            Some(Type::Generator { .. }) => Ok(RustExpr::postfix_on(value.expr(), self.carrier_format(format_args!(
                "{}.clone().into_smelt_unknown()",
                value.parenthesized_if_needed()
            )))),
            Some(Type::GeneratorResult { yield_ty, return_ty }) => {
                self.erased_iterator_result_expr(&RustExpr::opaque(value.parenthesized_if_needed()), *yield_ty, *return_ty)
            }
        }
    }

    /// An iterator result as the `{ value, done }` object JavaScript hands out.
    fn erased_iterator_result_expr(
        &self,
        result_text: &RustExpr,
        yield_ty: TypeId,
        return_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let yielded = self.erase_rendered(&RustExpr::path("value"), yield_ty)?;
        let returned = if return_ty == self.none_ty {
            self.carrier_expr(&RustExpr::undefined())
        } else {
            self.erase_rendered(&RustExpr::path("value"), return_ty)?
        };
        let object = |value: &str, done: bool| {
            format!(
                "SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"value\".to_owned(), {value}), (\"done\".to_owned(), SmeltUnknown::Bool({done}))])))"
            )
        };
        let (yielded, returned) = (object(&yielded, false), object(&returned, true));
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "match {result_text}.clone() {{ SmeltGeneratorResult::Yielded(value) => {yielded}, SmeltGeneratorResult::Complete(value) => {returned} }}"
        ))))
    }

    /// The erased `null` tag of `SmeltUnknown`.
    ///
    /// This is the canonical boxed *value* `null`: a `Type::None` value the
    /// source spelled, a `None` return, the default of an erased target. It is
    /// NOT the answer for a property that does not exist — JavaScript
    /// distinguishes the two, and `===`/`Object.is` see the difference, so an
    /// absent property asks for [`Self::absent_value`] instead. Keeping
    /// the variant text here means callers outside the seam never spell
    /// `SmeltUnknown::Null` by hand; they ask for the null tag by intent.
    pub(in crate::emitter) fn null_value(&self) -> RustExpr {
        self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null"))
    }

    /// The erased tag for a property that is ABSENT.
    ///
    /// Reading a property an object does not have evaluates to `undefined` in
    /// JavaScript, never to `null`. The two used to share
    /// [`Self::null_value`], which made a miss on a class receiver
    /// (`instance['nope']`) compare `=== null` and `!== undefined` — both
    /// backwards. Absence and the `null` value are different values, so they
    /// get different helpers and callers state which one they mean.
    pub(in crate::emitter) fn absent_value(&self) -> RustExpr {
        self.carrier_expr(&RustExpr::undefined())
    }

    /// Wrap a live concrete future in an erased, cloneable promise handle.
    ///
    /// The source future remains the only owner of the async computation; the
    /// `SmeltPromise` stores it behind shared state so erased identity and later
    /// awaits observe the same resolved value. The concrete future expression is
    /// evaluated before the `async move` block so callback captures owned by an
    /// outer `Fn` closure are not moved into the returned promise task.
    fn promise_future_unknown_expr(
        &self,
        future_text: &RustExpr,
        item_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let value_text = self.erase_rendered(&RustExpr::path("smelt_value"), item_ty)?;
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "{{ let smelt_future = {future_text}; SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move {{ let smelt_value = smelt_future.await?; Ok::<SmeltUnknown, {ERROR_BOX}>({value_text}) }}))) }}"
        ))))
    }

    /// Box an iterator of already-rendered `String` values into a `SmeltUnknown`
    /// string array, owning the per-element `SmeltUnknown::String` construction.
    ///
    /// `value_iter_text` must evaluate to something iterable yielding owned
    /// `String`s (e.g. a `Vec<String>` or a chained iterator). Callers that have
    /// produced a list of strings and need the erased array form route through
    /// here instead of writing the `SmeltUnknown::Array(..)` / `SmeltUnknown::String`
    /// variants themselves.
    pub(in crate::emitter) fn erase_string_array_expr(&self, value_iter_text: &RustExpr) -> RustExpr {
        RustExpr::atom(self.carrier_format(format_args!(
            "SmeltUnknown::Array({value_iter_text}.into_iter().map(|value| SmeltUnknown::String(value.into())).collect())"
        )))
    }

    /// Box an already-`f64` numeric expression into a `SmeltUnknown` number,
    /// owning the `SmeltUnknown::Number` construction.
    ///
    /// Unlike [`erase_rendered`](Self::erase_rendered) for `Int`/`Float`,
    /// this assumes `value_text` already evaluates to an `f64` and so adds no
    /// `as f64` cast. Use it where the numeric value was already coerced to
    /// `f64` before crossing the seam.
    pub(in crate::emitter) fn erase_f64_expr(&self, value_text: &RustExpr) -> RustExpr {
        RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::Number({value_text})")))
    }

    /// Box a fixed list of already-erased element expressions into a
    /// `SmeltUnknown` array.
    ///
    /// `elements_text` is the comma-joined Rust text of elements that have each
    /// already crossed the seam (e.g. via [`erase`](Self::erase)). This owns the
    /// outer `SmeltUnknown::Array(vec![..].into())` construction so literal-array
    /// erase paths need not spell the variant.
    pub(in crate::emitter) fn erase_array_expr(&self, elements_text: &RustExpr) -> RustExpr {
        RustExpr::atom(self.carrier_format(format_args!("SmeltUnknown::Array(vec![{elements_text}].into())")))
    }

    /// Box a fixed set of already-erased `(key, value)` entry expressions into a
    /// `SmeltUnknown` object.
    ///
    /// `entries_text` is the comma-joined Rust text of `(String, SmeltUnknown)`
    /// tuples whose values have already crossed the seam. This owns the outer
    /// `SmeltUnknown::Object(SmeltObject::new(..))` construction so literal-object
    /// erase paths need not spell the variant.
    pub(in crate::emitter) fn erase_object_expr(&self, entries_text: &RustExpr) -> RustExpr {
        RustExpr::atom(self.carrier_format(format_args!(
            "SmeltUnknown::Object(SmeltObject::new(Vec::from([{entries_text}])))"
        )))
    }

    /// Mark a timestamp-backed `Date` only when it crosses an erased value boundary.
    ///
    /// Internally dates stay as numeric timestamps for compact date arithmetic. The
    /// marker preserves JavaScript object identity for later dynamic `instanceof Date`
    /// checks without changing ordinary typed Date storage or comparisons.
    fn date_unknown_identity_expr(&self, value_text: &RustExpr) -> RustExpr {
        // `value_text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_value = value_text.cloned_value();
        RustExpr::atom(self.carrier_format(format_args!(
            "match {smelt_owned_value} {{ SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_DATE}\") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"{SMELT_DATE}\".to_owned(), SmeltUnknown::Number(value))]))), value => value }}"
        )))
    }

    /// Whether an interface field is declared `p: T | null`: it admits `null`
    /// and can never be absent, so its empty `Option` is `null`.
    ///
    /// Read from the interface's `FieldDecodeFacts`; a class records none and
    /// keeps the `undefined` reading.
    fn field_absent_is_null(&self, owner: Symbol, field: Symbol) -> bool {
        self.mir
            .interfaces
            .iter()
            .find(|interface| interface.name == owner)
            .is_some_and(|interface| {
                interface.decode_facts.iter().any(|facts| {
                    facts.name == field && facts.admits_null && !facts.may_be_absent
                })
            })
    }

    /// The effective fields of the class or interface named `name`, at their
    /// declared types (empty when `name` is neither).
    fn class_or_interface_fields(&self, name: Symbol) -> Vec<MirField> {
        crate::classes::find_class(self.mir, name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
            .or_else(|| {
                self.mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == name)
                    .map(|interface| crate::classes::effective_interface_fields(self.mir, interface))
            })
            .unwrap_or_default()
    }

    /// Wrap a generated class or interface value into an erased object.
    ///
    /// TypeScript structural objects often reach erased helper surfaces through
    /// callbacks. Preserving their known fields keeps those values observable
    /// after the type is widened to `unknown` instead of silently replacing the
    /// object with an empty map.
    fn class_unknown_object_expr(
        &self,
        value_text: &RustExpr,
        target: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.record_conversion_stack.borrow().contains(&target) {
            return Ok(self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null")));
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(target) else {
            return Ok(self.carrier_expr(&RustExpr::atom("SmeltUnknown::Null")));
        };
        let conversion_entry = helper_reservation::push_entry(&self.record_conversion_stack, target);
        // Field types at the value's own type arguments: `LazyMany<R>` stores
        // `next: SmeltList<R>`, not the declaration's `SmeltList<T>`, and each
        // field erases by what it holds here. Judging the declared `T` against
        // this render scope mistook an in-scope `R` list for an erased one
        // (E0277 `SmeltArray: From<SmeltList<R>>`).
        //
        // A callable field keeps its DECLARED type: which of its parameters
        // the stored `Rc<dyn Fn>` takes by shared reference is decided on the
        // declaration (`&T`), and the erasure adapter must call it with that
        // ABI whatever `T` is bound to here.
        let declared_fields = self.class_or_interface_fields(*name);
        let fields = match self.structural_record_fields(target) {
            Some(substituted) if substituted.len() == declared_fields.len() => substituted
                .into_iter()
                .zip(&declared_fields)
                .map(|(mut field, declared)| {
                    if matches!(self.mir.types.get(declared.ty), Some(Type::Function(_))) {
                        field.ty = declared.ty;
                    }
                    field
                })
                .collect(),
            _ => declared_fields,
        };

        // A reference class stores its fields inside the shared `Rc<RefCell<Inner>>`
        // cell, so erasing one to a plain object must read each field through
        // `.0.borrow()` rather than a direct named-field access against the
        // newtype (was E0609).
        let field_base = if self.is_reference_class_type(target) {
            RustExpr::chain("smelt_object_value.0.borrow()")
        } else {
            RustExpr::path("smelt_object_value")
        };
        let entries_result = fields
            .iter()
            .filter(|field| field.visibility.is_own_property())
            .map(|field| {
                let source_name = self.symbol_source_name(field.name)?;
                let field_name = sanitize_ident(self.symbol_name(field.name)?);
                // An accessor erases the value its getter answers now.
                let accessor = crate::accessor_prelude::is_accessor_field(self.mir, *name, field.name);
                let optional_read = if accessor {
                    crate::accessor_prelude::read_expr(&RustExpr::field_on(&field_base, format!("{field_base}.{field_name}")))
                } else {
                    RustExpr::postfix_on(&field_base, format!("{field_base}.{field_name}.clone()")).ending_in_clone()
                };
                if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                    let field_value = self.erase_rendered(&RustExpr::path("value"), *inner)?;
                    // A `p: T | null` field holds `null` when its `Option` is
                    // empty, so its key is present with a `null` value; an
                    // absent `undefined` is omitted, as JavaScript omits it.
                    if self.field_absent_is_null(*name, field.name) {
                        return Ok(self.carrier_format(format_args!(
                            "smelt_object_entries.push(({source_name:?}.to_owned(), match {optional_read} {{ Some(value) => {field_value}, None => SmeltUnknown::Null }}));"
                        )));
                    }
                    return Ok(format!(
                        "if let Some(value) = {optional_read} {{ smelt_object_entries.push(({source_name:?}.to_owned(), {field_value})); }}"
                    ));
                }
                let field_value = if let Some(value) =
                    self.virtual_method_storage_field_expr(target, target, field.name)?
                {
                    self.erase_rendered(&value, field.ty)?
                } else if self.is_reference_class_type(target) {
                    // Reading through `.0.borrow()` yields a `Ref` guard; the value
                    // must be cloned out rather than moved (was E0507).
                    self.erase_rendered(
                        &RustExpr::postfix_on(&field_base, format!("{field_base}.{field_name}.clone()")).ending_in_clone(),
                        field.ty,
                    )?
                } else if accessor {
                    self.erase_rendered(
                        &RustExpr::atom(format!(
                            "({})",
                            crate::accessor_prelude::read_expr(&RustExpr::field_on(&field_base, format!("{field_base}.{field_name}")))
                        )),
                        field.ty,
                    )?
                } else {
                    self.erase_rendered(
                        &RustExpr::field_on(&field_base, format!("{field_base}.{field_name}")),
                        field.ty,
                    )?
                };
                Ok(format!(
                    "smelt_object_entries.push(({source_name:?}.to_owned(), {field_value}));"
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>();
        drop(conversion_entry);
        let entries = entries_result?.join(" ");

        // Inject a hidden provenance marker so an erased class instance is no longer
        // indistinguishable from a plain object of the same shape. The marker is
        // VISIBLE to structural equality (so a class instance is not `==` to an
        // equal-shape plain object) but INVISIBLE to key enumeration / JSON via the
        // `__smelt_class` filters, and drives the `"__smelt_proto:class"` sentinel in
        // `smelt_prototype_sentinel`.
        // A user class whose base chain reaches a modeled host object (e.g.
        // `class File extends Blob`) IS an instance of that host in JavaScript, so
        // its erased record carries the host base's identity marker(s). This keeps
        // `value instanceof Blob` (a marker check on the erased value) honest for
        // host subclasses — including override classes assigned into a
        // `globalThis.<Name>` slot — without any globalThis special-casing.
        let mut host_markers = String::new();
        for (marker, value_text) in self.host_base_markers(*name) {
            use std::fmt::Write as _;
            // `marker` is a fixed `__smelt_*` identifier, so wrap it in explicit
            // quotes rather than Debug-formatting it into a Rust string literal.
            let _ = write!(
                host_markers,
                "smelt_object_entries.push((\"{marker}\".to_owned(), {value_text})); "
            );
        }
        // The marker VALUE is the class name, not a bare `true`. JavaScript exposes
        // `instance.constructor`, and es-toolkit `isEqualWith` gates instance
        // comparison on `areObjectsEqual(a.constructor, b.constructor)` — with no
        // name recorded, two instances of DIFFERENT classes both answered
        // `undefined` for `.constructor`, `Object.is(undefined, undefined)` held,
        // and they compared equal. `smelt_get_object_field` reads the name back to
        // intern one constructor value per class.
        //
        // An INTERFACE-backed record is not a class instance: in JavaScript it is
        // an ordinary object literal, with no constructor and no prototype to
        // report. Stamping it would make `{ a: 1 }` typed as `{ a: number }`
        // unequal to the same plain object and give it a bogus `.constructor`.
        // The interface's own generated `IntoSmeltUnknown` (see
        // `emit_record_into_smelt_unknown_impl`) already omits the marker; this
        // inline adapter must agree, or the same value erases two different ways
        // depending on which path the emitter took.
        let class_marker = if self.is_interface_record_type(target) {
            String::new()
        } else {
            let class_name = self.symbol_source_name(*name)?;
            self.carrier_format(format_args!(
                "smelt_object_entries.push((\"{SMELT_CLASS}\".to_owned(), SmeltUnknown::String({class_name:?}.into()))); "
            ))
        };
        // A reference record's JS identity is its shared cell, so every erasure
        // of any handle on that cell must produce the same object id — otherwise
        // `const b = a; erase(a) === erase(b)` (and `expect(x).toBe(obj)` after a
        // mutation) is false for what is one object. A by-value record has no
        // identity to preserve and keeps minting a fresh id per erasure.
        //
        // The reference view also holds the live handle, so narrowing it back
        // yields this instance rather than a rebuilt copy (`reference_origin`).
        let object_ctor = if self.is_reference_class_type(target) {
            crate::reference_origin::with_origin_expr(
                "SmeltObject::with_id(smelt_reference_object_identity(&smelt_struct_value.0), smelt_object_entries)",
                "smelt_struct_value",
            )
        } else {
            self.carrier_expr(&RustExpr::atom("SmeltObject::new(smelt_object_entries)"))
        };
        // A class instance keeps its methods across the erasure seam, as
        // prototype-carried members (`__smelt_proto:<name>`). Without them an
        // erased instance answered `undefined` for every method read and the
        // erased call site substituted a fabricated default. Interfaces have no
        // method bodies to bind, so they contribute nothing here — which is why
        // this consults `mir.classes` rather than the record type.
        let proto_entries = match crate::classes::find_class(self.mir, *name)
        {
            Some(class)
                if crate::class_proto::class_has_proto_entries(self.mir, self.context, class) =>
            {
                format!(
                    "smelt_object_entries.extend(smelt_struct_value.{method}()); ",
                    method = crate::class_proto::PROTO_ENTRIES_METHOD,
                )
            }
            _ => String::new(),
        };
        // A reference-class view forwards property writes to the instance
        // (see `view_write_through`); the generated `into_smelt_unknown`
        // installs the same hook.
        let write_through = match crate::classes::find_class(self.mir, *name) {
            Some(class) if crate::reference_storage::field_setter_host(self.mir, self.context, class).is_some() => {
                view_write_through::erased_view_entry_expr(
                    &RustExpr::path("smelt_struct_value"),
                    "smelt_object_entries",
                    view_write_through::has_live_callable_fields(
                        self.mir,
                        self.context.program_reads_this(),
                        &crate::classes::effective_class_fields(self.mir, class),
                    ),
                )
            }
            _ => RustExpr::opaque(""),
        };
        // A NON-GENERIC reference record builds its snapshot on first use: the
        // view already holds the live handle, and a value carried across an
        // erased ABI and narrowed straight back never reads the snapshot at
        // all (`lazy_view`). A generic one stays eager, because the `'static`
        // boxed builder could not name its type parameters.
        if self.is_reference_class_type(target)
            && self.rust_shape(target)? != RustShape::GenericClass
        {
            let lazy = crate::lazy_view::lazy_reference_object_expr(
                &RustExpr::chain("smelt_reference_object_identity(&smelt_struct_value.0)"),
                &format!("{entries} {host_markers}{class_marker}{proto_entries}{write_through}"),
                "smelt_struct_value",
            );
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_object_value = {value_text}; let smelt_struct_value = smelt_object_value.clone(); SmeltUnknown::Object({lazy}) }}"
            ))));
        }
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "{{ let smelt_object_value = {value_text}; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::new(); {entries} {host_markers}{class_marker}{proto_entries}{write_through}SmeltUnknown::Object({object_ctor}) }}"
        ))))
    }
}
