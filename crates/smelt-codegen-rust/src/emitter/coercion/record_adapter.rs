//! Structural record adapters, behind the coercion seam: a value of one record
//! shape rebuilt field by field as another, or as or from a string-keyed dict.

use super::*;

impl FunctionEmitter<'_> {
    /// `mapped`, a conversion of field `key` of `target`, with a step on the
    /// error path while it runs, when the field's type holds a record.
    fn conversion_framed(
        &self,
        mapped: RustExpr,
        source_is_erased: bool,
        target: TypeId,
        key: &str,
        field_ty: TypeId,
    ) -> RustExpr {
        if !source_is_erased || !record_coercion::type_holds_generated_record(self.mir, field_ty) {
            return mapped;
        }
        let record = json_decode::ts_type_text(self.mir, target);
        RustExpr::atom(format!(
            "{{ let _smelt_conv_frame = smelt_conv_enter({record:?}, {key:?}); {mapped} }}",
            mapped = mapped.delimited()
        ))
    }

    /// Returns true when a generated storage field is itself a callable value.
    ///
    /// Callable fields must be read from the struct before method-reference
    /// fallback runs; otherwise abstract virtual slots such as `this.parse`
    /// are erased into placeholder method references instead of dispatching
    /// through the concrete adapter-bound closure.
    pub(in crate::emitter) fn storage_field_is_function(&self, ty: TypeId, field: Symbol) -> bool {
        self.structural_record_fields(ty)
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|candidate| candidate.name == field)
                    .map(|candidate| candidate.ty)
            })
            .is_some_and(|field_ty| matches!(self.mir.types.get(field_ty), Some(Type::Function(_))))
    }

    /// Emits a field-wise adapter between structurally compatible record types.
    pub(in crate::emitter) fn structural_record_adapter(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        // The callee's own `Box<T>` of the same record: its `T` is bound from this
        // value, so there is nothing to convert and no spelling of `T` here.
        if let (Some(Type::Class { name: from, .. }), Some(Type::Class { name: to, args })) =
            (self.mir.types.get(source), self.mir.types.get(target))
            && from == to
            && !args.is_empty() && args.iter().all(|arg| {
            matches!(
                self.mir.types.get(*arg),
                Some(Type::TypeParam { name, .. }) if !self.current_function_has_type_param(*name)
            )
        }) {
            return Ok(Some(value_text.cloned_value()));
        }
        // An instance seen at other type arguments of a widened class
        // (`Parser<number>` as `Parser<unknown>`) stays the same object: the
        // handle retargets it (`crate::class_widening`).
        if let Some(retarget) = self.widened_retarget_expr(value_text, source, target)? {
            return Ok(Some(retarget));
        }
        // A reference class is a handle newtype over `Rc<RefCell<Inner>>`, not a
        // field-wise struct, so it cannot be rebuilt with a `Name { field: .. }`
        // literal (that fails with E0560). When the source already has the
        // target's reference-class type, cloning the handle is the correct
        // adaptation: it shares the same underlying cell, matching JavaScript
        // reference identity. (Cross-type adaptation into a reference class is
        // not modeled here and falls through to `None`.)
        if self.is_reference_class_type(target)
            && matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Class { name: src_name, .. }), Some(Type::Class { name: tgt_name, .. })) if src_name == tgt_name
            )
        {
            return Ok(Some(value_text.cloned_value()));
        }
        // An instance of a subclass viewed as a polymorphic ancestor is the
        // SAME object: the upcast re-views its shared cell
        // (`crate::class_polymorphism`), never copies it.
        if let Some(upcast) = self.polymorphic_upcast_expr(value_text, source, target)? {
            return Ok(Some(upcast));
        }
        // A record viewed as another record type that shares its properties
        // is the SAME object too: the conversion re-views its cell
        // (`crate::structural_views`), never copies it.
        if let Some(view) = self.structural_view_expr(value_text, source, target)? {
            return Ok(Some(view));
        }
        // A MODELED HOST class has no generated fields to pair up — its data
        // lives behind the prelude struct's accessors — but it still EXPOSES
        // members, and structural assignability is about members. A `Response`
        // is assignable to a `ResponseInit` because it has `status`,
        // `statusText` and `headers`; nothing about that is an overload or a
        // union, it is plain structural typing, which Smelt otherwise requires
        // nominal identity for. Asked before the field-pairing path below
        // because `structural_record_fields` answers `None` for such a source
        // and the whole adapter would decline.
        if let Some(adapter) =
            self.host_class_to_record_adapter(value_text, source, target, scope)?
        {
            return Ok(Some(adapter));
        }
        let Some(adapted_fields) = self.structural_record_adapter_fields(source, target) else {
            return Ok(None);
        };
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        self.trace_record_copy(source, target);
        // Field-wise adaptation recurses through each field's own coercion, and
        // a callable interface's `__smelt_call` field is a function whose return
        // value can be the record again (es-toolkit's
        // `CurriedFunction1<T1, R>` returns `CurriedFunction1<T1, R>`), so this
        // pair can be requested again from inside its own expansion. Truncate
        // that cycle with the target's default value rather than `None`: the
        // callers that reach an adapter here have already committed to a
        // record-shaped destination, and handing them the *source* value back
        // would be ill-typed (`CurriedFunction2` where `CurriedFunction1` is
        // expected).
        let Some(_guard) = self.enter_type_expansion(source, target) else {
            // A pair that reaches itself through a field whose value is built
            // LAZILY (a callable slot returning the record, `swap():
            // Pair<T>`) calls a hoisted helper for the same pair. The
            // default is kept only where the pair cannot be spelled at module
            // level.
            if let Some(call) = self.hoisted_record_adapter_call(value_text, source, target)? {
                return Ok(Some(RustExpr::from(call)));
            }
            return Ok(Some(self.default_value(target)?));
        };
        let target_name = self.type_rust_name(*name)?;
        let mut field_text = Vec::new();
        for (source_field_match, target_field) in adapted_fields {
            let field_name = self.context.names().field_text(self.mir, target_field.name);
            let mut value_at_slot_abi = false;
            let value = if let Some(value) =
                self.virtual_method_storage_field_expr(source, target, target_field.name)?
            {
                value
            } else if let Some(value) = match &source_field_match {
                Some(source_field) => self.receiver_bound_field_slot_expr(
                    source,
                    target,
                    source_field,
                    &target_field,
                    scope,
                )?,
                None => None,
            } {
                value
            } else if let Some(source_field) = source_field_match {
                let source_field_name =
                    self.context.names().field_text(self.mir, source_field.name);
                // A reference-class source keeps its fields inside the shared
                // `Rc<RefCell<Inner>>` cell, so they are read through it.
                let source_value = if self.is_reference_class_type(source) {
                    RustExpr::chain(self.reference_field_read_place(
                        "smelt_struct_value",
                        source,
                        source_field.name,
                    )?)
                    .cloned_value()
                } else {
                    RustExpr::path("smelt_struct_value")
                        .postfix(format!(".{source_field_name}"))
                        .cloned_value()
                };
                let adapted =
                    self.rendered_at_type(&source_value, source_field.ty, target_field.ty, scope)?;
                value_at_slot_abi = adapted == source_value
                    && self.record_slot_read_keeps_slot_abi(
                        source,
                        &source_field,
                        target,
                        &target_field,
                    );
                // Narrowing a callable object to a callable interface that
                // declares fewer members drops the source's own data fields —
                // in JavaScript those are properties of the *function value*
                // and survive the narrowing (es-toolkit's `curry` assigns
                // `wrapper.placeholder`, and its `flow` spec reads that
                // property back off a `CurriedFunction1`). The erased callable
                // carries a property bag for exactly this, so hand the dropped
                // properties to it instead of losing them at the seam.
                if self.symbol_source_name(target_field.name)? == SMELT_CALL {
                    self.callable_object_carried_properties_expr(&adapted, source, target)?
                } else {
                    adapted
                }
            } else {
                // No source field supplies this value, so nothing here can
                // spell a type parameter: an explicitly empty environment.
                self.default_value_with_scoped_type_params(
                    target_field.ty,
                    &TypeSubstitution::erased(),
                )?
            };
            // A callable slot keeps the ABI its DECLARATION gave it, whatever
            // the record was instantiated at (`emitter::record_slot_abi`). A
            // value read straight off the same slot of the same declaration is
            // at that ABI already and must not be adapted a second time.
            let value = if value_at_slot_abi {
                value
            } else {
                self.record_field_value_at_slot_abi(value.into_string(), target, &target_field)?
            };
            field_text.push(format!("{field_name}: {value}", value = value.delimited()));
        }
        self.push_phantom_field(&mut field_text, *name, args.len());
        // An upcast keeps the class the instance was constructed as.
        let identity_source = if self.is_reference_class_type(source) {
            "smelt_struct_value.0.borrow()"
        } else {
            "smelt_struct_value"
        };
        self.push_class_identity_field(&mut field_text, *name, Some((source, identity_source)));
        self.push_function_identity_field(&mut field_text, *name, Some((source, identity_source)));
        // A reference-class target is a handle newtype over its `Inner` record,
        // so the field-wise value is built as the record and wrapped in a fresh
        // shared cell. This is the structural conversion between two distinct
        // classes TS accepts because they expose the same members (Hono's
        // `basePath` returns the base class, handed to a parameter typed as the
        // subclass that adds no members); it copies the members into a new
        // instance.
        if self.is_reference_class_type(target) {
            let inner = crate::class_polymorphism::inner_literal(
                self.mir,
                self.context,
                *name,
                &field_text,
            )?;
            let copied =
                format!("{target_name}(::std::rc::Rc::new(::std::cell::RefCell::new({inner})))");
            // A downcast checks whether the instance IS one of the target's
            // classes first and keeps it when it is; the field-wise copy is
            // only the answer for an instance of another class.
            let checked = match self.polymorphic_downcast_expr(source, target)? {
                Some(found) => Some(found),
                // A view cast to another record type is the object it stands
                // for when that object is one of the target's records
                // (`crate::structural_views`).
                None => self.structural_view_downcast_expr(source, target)?,
            };
            if let Some(downcast) = checked {
                return Ok(Some(RustExpr::atom(format!(
                    "{{ let smelt_struct_value = {value_text}.clone(); match {downcast} {{ Some(smelt_downcast) => smelt_downcast, None => {copied} }} }}",
                    value_text = value_text.tight()
                ))));
            }
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_struct_value = {value_text}.clone(); {copied} }}",
                value_text = value_text.tight()
            ))));
        }
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", "),
            value_text = value_text.tight()
        ))))
    }

    /// The call to a hoisted adapter from `source` to `target`, registering
    /// it on first use; see `EmitContext::record_adapter_helpers`.
    ///
    /// The helper is reserved BEFORE its body renders, and the body renders
    /// with the expansion stack cleared, so the cycle inside it resolves to a
    /// call to the helper itself. That recursion is finite at run time: the
    /// self-reaching field is a callable slot, adapted lazily. `None` when
    /// either type spells a type parameter (a module-level helper cannot), or
    /// when no structural adapter exists for the pair.
    pub(in crate::emitter) fn hoisted_record_adapter_call(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let scoped = (
            self.rendered_type(source)?.into_string(),
            self.rendered_type(target)?.into_string(),
        );
        let hoisted = HoistedModuleItemScope::enter(self);
        let module = (
            self.rendered_type(source)?.into_string(),
            self.rendered_type(target)?.into_string(),
        );
        if module != scoped {
            return Ok(None);
        }
        let existing = self
            .context
            .minted
            .record_adapter_helpers
            .borrow()
            .get(&module)
            .map(|(ordinal, _)| *ordinal);
        let ordinal = if let Some(ordinal) = existing {
            ordinal
        } else {
            // Reserved before the body renders (the cycle calls it); released
            // unless a body is committed (see `helper_reservation`).
            let reservation = helper_reservation::reserve(
                &self.context.minted,
                &self.context.minted.record_adapter_helpers,
                module,
            );
            let body = {
                // The helper's body is a fresh module-level item: the
                // expansions in progress at this call site are not its own.
                let _suspended =
                    helper_reservation::suspend(&self.conversions.type_expansion_stack);
                let module_scope = self.render_scope();
                self.structural_record_adapter(
                    &RustExpr::path("smelt_value"),
                    source,
                    target,
                    &module_scope,
                )
            };
            let Some(body) = body? else {
                return Ok(None);
            };
            reservation.commit(body.into_string())
        };
        drop(hoisted);
        Ok(Some(format!(
            "{}{ordinal}({value_text})",
            crate::RECORD_ADAPTER_HELPER_PREFIX,
            value_text = value_text.delimited()
        )))
    }

    /// Build a target record by READING each of its fields off a host value.
    ///
    /// Structural assignability: a value whose type exposes every member a
    /// target record type declares converts to that record by reading each
    /// target field from the source and building the struct. The read is the
    /// member-read rule the ordinary `x.member` uses
    /// (`host_class_member_read`), so a `Response`'s `status` is the
    /// prelude struct's `status()` accessor and not a field that does not
    /// exist.
    ///
    /// Declines — leaving every existing conversion exactly as it was — unless
    /// the source is a modeled host class with a member table AND every
    /// non-optional target field is one of its members. An OPTIONAL target
    /// field the source does not expose is simply absent, which is what an
    /// optional property means.
    fn host_class_to_record_adapter(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if source == target || !self.exposes_host_members(source)? {
            return Ok(None);
        }
        let Some(target_fields) = self.structural_record_fields(target) else {
            return Ok(None);
        };
        if target_fields.is_empty() {
            return Ok(None);
        }
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let mut field_text = Vec::new();
        for target_field in &target_fields {
            let field_name = self.context.names().field_text(self.mir, target_field.name);
            let member = self.symbol_source_name(target_field.name)?.to_owned();
            let read =
                self.host_class_member_read(&RustExpr::path("smelt_host_value"), source, &member)?;
            let value = match read {
                Some((read_expr, member_ty)) => {
                    self.rendered_at_type(&read_expr, member_ty, target_field.ty, scope)?
                }
                // An optional property the source does not expose is absent;
                // any other missing member means this is not a structural
                // conversion at all, so nothing is emitted.
                None if matches!(
                    self.mir.types.get(target_field.ty),
                    Some(Type::Optional(..))
                ) =>
                {
                    RustExpr::path("None")
                }
                None => return Ok(None),
            };
            field_text.push(format!("{field_name}: {value}", value = value.delimited()));
        }
        let target_name = self.type_rust_name(*name)?;
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        self.push_class_identity_field(&mut field_text, *name, None);
        self.push_function_identity_field(&mut field_text, *name, None);
        // The source is read once per field, so it is bound once: `value_text`
        // may be a call, and re-evaluating it per field would both duplicate
        // its effects and move any by-value argument more than once.
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_host_value = {value_text}; {target_name} {{ {} }} }}",
            field_text.join(", "),
            value_text = value_text.delimited()
        ))))
    }

    /// Emits a map-to-record adapter for object literals that were first
    /// materialized as string-keyed dictionaries.
    ///
    /// This preserves TypeScript's structural object-literal assignment
    /// semantics when the frontend has not retained the original literal at the
    /// destination site. Missing optional fields become `None`; missing required
    /// fields fall back to the target field default.
    pub(super) fn string_dict_record_adapter(
        &self,
        value_text: &RustExpr,
        source_key: TypeId,
        source_value: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if self
            .mir
            .types
            .get(source_key)
            .is_none_or(|candidate| !candidate.is_text())
            || !self.is_structural_record_adapter_target(target)
        {
            return Ok(None);
        }
        let Some(Type::Class { name, args }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let Some(target_fields) = self.structural_record_fields(target) else {
            return Ok(None);
        };
        if target_fields.is_empty() {
            return Ok(None);
        }

        let target_name = self.type_rust_name(*name)?;
        // The target class/interface may be parameterized by type params (e.g.
        // `CurriedFunction1<T1, _>`). Those names are only legal to spell in the
        // adapter body when they are actually in scope for the function being
        // emitted. Inside a generic class member or generic free function the
        // enclosing signature declares them, so a field default may keep the
        // generic shape. At a non-generic call site (e.g. a spec function) the
        // same name is unresolvable and must erase to `SmeltUnknown` instead of
        // printing a dangling `T1`. Intersect the target's type-param
        // args with the current function's in-scope type params so only genuinely
        // scoped names survive.
        let in_scope = self.current_function_type_params();
        let scoped_type_params = args
            .iter()
            .filter_map(|arg| match self.mir.types.get(*arg) {
                Some(Type::TypeParam {
                    name: type_param_name,
                }) if in_scope.contains(type_param_name) => Some(*type_param_name),
                _ => None,
            })
            .collect::<HashSet<_>>();
        // The prototype chain only exists on an erased JavaScript object. A
        // concretely typed dictionary (`HashMap<String, String>`) has no
        // prototype slot and can never carry one of the hidden keys, so the
        // fallback is emitted only for the erased projection — where the source
        // value type is the tagged carrier itself.
        let source_is_erased = matches!(self.mir.types.get(source_value), Some(Type::Unknown));
        let mut field_text = Vec::new();
        for field in target_fields {
            // The record's keys are the JavaScript property names, so the
            // lookup spells the field as the source does: `isHTML`, `URL`,
            // `snake_case`. The Rust field name is a rendering of that
            // spelling and cannot be inverted back into it.
            let field_key = self.symbol_source_name(field.name)?;
            let field_name = self.context.names().field_text(self.mir, field.name);
            // A record projection of an erased value is a JavaScript member read
            // of each field, so it answers what `smelt_get_object_field` would:
            // prototype members, accessors, a host object's members. A
            // `#private` field travels under a method-prefixed key instead.
            let hidden = !field.visibility.is_own_property();
            let own_read = if source_is_erased && hidden {
                let hidden_key = format!("{}{field_key}", crate::class_proto::METHOD_KEY_PREFIX);
                format!("smelt_record_map.get_text({hidden_key:?})")
            } else if source_is_erased {
                format!("smelt_erased_record_member(&smelt_record_map, {field_key:?})")
            } else {
                format!("smelt_record_map.get_text({field_key:?})")
            };
            // A callable slot is not a property in JavaScript, so a non-callable
            // object asserted to a callable type is observably fine until it is
            // called, and then it throws. Remeda hands `pipe` plain
            // `{ lazy, lazyArgs }` definitions through exactly that assertion.
            let lookup_text = if source_is_erased && field_key == SMELT_CALL {
                format!("{own_read}.or_else(|| Some(smelt_not_callable_slot()))")
            } else {
                own_read
            };
            let lookup_value = if let Some(Type::Dict(key, _)) = self.mir.types.get(source_value) {
                if self.dict_uses_smelt_record(*key) {
                    lookup_text
                } else {
                    format!("{lookup_text}.cloned()")
                }
            } else {
                format!("{lookup_text}.cloned()")
            };
            let receiver_bound = if source_is_erased
                && self.dict_uses_smelt_record(source_key)
                && self.can_render_dict_value_as(source_value, field.ty)
            {
                self.erased_record_callable_slot_expr("smelt_record_map", target, &field, scope)?
            } else {
                None
            };
            let value = if let Some(value) = receiver_bound {
                value
            } else if let Some(Type::Optional(inner, _)) = self.mir.types.get(field.ty)
                && self.record_value_misses_concrete_union(source_value, *inner)
            {
                // The record's values cannot be this field's type, so the key
                // is absent from any value the source holds.
                RustExpr::path("None")
            } else if let Some(Type::Optional(inner, _)) = self.mir.types.get(field.ty) {
                if self.can_render_dict_value_as(source_value, *inner) {
                    let rendered = self.rendered_at_type(
                        &RustExpr::path("smelt_value"),
                        source_value,
                        *inner,
                        scope,
                    )?;
                    let mapped = self.conversion_framed(
                        rendered,
                        source_is_erased,
                        target,
                        field_key,
                        *inner,
                    );
                    if source_is_erased
                        && !matches!(self.mir.types.get(*inner), Some(Type::Unknown))
                    {
                        // A present `null`/`undefined` is the field's ABSENT
                        // arm, not a value to convert: `score: number | null`
                        // read off `{ score: null }` is `None`, where mapping
                        // it through the number conversion answered `NaN`.
                        RustExpr::atom(lookup_value).postfix(format!(
                            ".and_then(|smelt_value| if smelt_unknown_is_nullish(&smelt_value) {{ None }} else {{ Some({mapped}) }})", mapped = mapped.delimited()
                        ))
                    } else {
                        RustExpr::atom(lookup_value).postfix(format!(
                            ".map(|smelt_value| {mapped})",
                            mapped = mapped.delimited()
                        ))
                    }
                } else if self.can_render_dict_value_as(source_value, field.ty) {
                    // The dictionary value is already optional-shaped, so the
                    // `get` produced an `Option<Option<_>>`; flatten it instead of
                    // discarding the field. Without this arm the projection
                    // answered `None` for a value that was actually present.
                    let mapped = self.rendered_at_type(
                        &RustExpr::path("smelt_value"),
                        source_value,
                        field.ty,
                        scope,
                    )?;
                    RustExpr::atom(lookup_value).postfix(format!(
                        ".map_or(None, |smelt_value| {mapped})",
                        mapped = mapped.delimited()
                    ))
                } else {
                    RustExpr::path("None")
                }
            } else if self.can_render_dict_value_as(source_value, field.ty) {
                let rendered = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    source_value,
                    field.ty,
                    scope,
                )?;
                let mapped =
                    self.conversion_framed(rendered, source_is_erased, target, field_key, field.ty);
                // See `emit_record_from_smelt_unknown_impl` for why `#private` is exempt.
                let missing = (source_is_erased && !hidden)
                    .then(|| {
                        let record = json_decode::ts_type_text(self.mir, target);
                        record_coercion::missing_required_field_throw(
                            self.mir, &record, field_key, field.ty,
                        )
                    })
                    .flatten();
                if let Some(throw) = missing {
                    RustExpr::atom(lookup_value).postfix(format!(
                        ".map_or_else(|| {throw}, |smelt_value| {mapped})",
                        mapped = mapped.delimited()
                    ))
                } else {
                    RustExpr::atom(lookup_value).postfix(format!(
                        ".map_or({}, |smelt_value| {mapped})",
                        self.default_value_with_scoped_type_params(
                            field.ty,
                            &TypeSubstitution::lexical_subset(&scoped_type_params),
                        )?
                        .delimited(),
                        mapped = mapped.delimited()
                    ))
                }
            } else {
                self.default_value_with_scoped_type_params(field.ty, &TypeSubstitution::erased())?
            };
            // A callable slot keeps the ABI its DECLARATION gave it, whatever
            // the record was instantiated at (`emitter::record_slot_abi`).
            let value = self.record_field_value_at_slot_abi(value.into_string(), target, &field)?;
            field_text.push(format!("{field_name}: {value}", value = value.delimited()));
        }
        self.push_phantom_field(&mut field_text, *name, args.len());
        // An erased instance names its class in its `__smelt_class` marker.
        if source_is_erased && self.context.carries_class_identity(*name) {
            field_text.push(format!(
                "{}: {}",
                crate::class_identity::CLASS_IDENTITY_FIELD,
                crate::class_identity::identity_from_marker_expr(
                    self.mir,
                    *name,
                    "smelt_record_map.get_text(\"__smelt_class\")"
                )
            ));
        } else {
            self.push_class_identity_field(&mut field_text, *name, None);
        }
        self.push_function_identity_field(&mut field_text, *name, None);
        // A reference class is a `Rc<RefCell<Inner>>` newtype, not a named-field
        // struct. The record round-trip must mint a fresh shared cell around the
        // reconstructed inner record rather than emit a struct literal against a
        // tuple struct.
        //
        // A record view that came from an erasure of this reference record
        // carries the live handle, and JavaScript narrowing yields that SAME
        // object -- so the rebuild is only the answer for a view with no origin
        // (a literal, a JSON value, a spread copy). See `reference_origin`.
        if self.context.is_reference_class(*name) {
            let inner = crate::class_polymorphism::inner_literal(
                self.mir,
                self.context,
                *name,
                &field_text,
            )?;
            let rebuild = RustExpr::atom(target_name.clone()).postfix(format!(
                "(::std::rc::Rc::new(::std::cell::RefCell::new({inner})))"
            ));
            // Only an erased-valued `SmeltRecord` view can share an erased
            // object's store; a concretely typed dictionary never carries one.
            let body = if source_is_erased && self.dict_uses_smelt_record(source_key) {
                // A polymorphic class also takes back an erased instance of
                // a subclass, as the same object (`crate::class_polymorphism`).
                let subclass_origins = match crate::classes::find_class(self.mir, *name) {
                    Some(class) if self.context.is_polymorphic_class(*name) => {
                        let arg_texts = args
                            .iter()
                            .map(|arg| Ok(self.rendered_type(*arg)?.as_str().to_owned()))
                            .collect::<Result<Vec<_>, EmitError>>()?;
                        let mut origins = crate::class_polymorphism::subclass_origins_at(
                            self.mir,
                            self.context,
                            class,
                            &arg_texts,
                            &|index, ty| args.get(index) == Some(&ty),
                        )?;
                        // A widened class also takes back an instance at
                        // other arguments, as the same object
                        // (`crate::class_widening`).
                        if self.context.is_widened_class(*name) {
                            let carried = self
                                .context
                                .type_param_elision()
                                .retain_carried(*name, arg_texts);
                            let path = if carried.is_empty() {
                                target_name
                            } else {
                                format!("{target_name}::<{}>", carried.join(", "))
                            };
                            origins.extend(crate::class_widening::instantiation_origins(
                                self.mir,
                                self.context,
                                class,
                                &path,
                            )?);
                        }
                        origins
                    }
                    _ => Vec::new(),
                };
                crate::reference_origin::restore_subclass_or_rebuild_expr(
                    &RustExpr::path("smelt_record_map"),
                    &rebuild,
                    &subclass_origins,
                    self.context.is_live_record(*name),
                )
            } else if let Some(adopted) =
                self.typed_dict_adopting_rebuild(&rebuild, source_key, source_value, *name, scope)?
            {
                adopted
            } else {
                rebuild
            };
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_record_map = {value_text}.clone(); {body} }}",
                value_text = value_text.tight(),
                body = body.delimited()
            ))));
        }
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_record_map = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", "),
            value_text = value_text.tight()
        ))))
    }

    /// The conversion of `smelt_record_map`, a dictionary of the concrete
    /// value type `source_value`, to the live record `target` (built by
    /// `rebuild`): the record the dictionary already stands for, or a new one
    /// the dictionary ADOPTS (`SmeltRecord::smelt_adopt_typed`), becoming its
    /// live typed view. `None` when the target is not a live record or the
    /// program has no views, which keeps the copy.
    ///
    /// An object literal whose values share one type is kept as such a
    /// dictionary when its uses rule out a struct (`literal_record_uses`). It
    /// is still one JavaScript object: a callee that writes the record it
    /// receives (`touch(flags)` with `touch(f: Flags) { f.touched = true }`)
    /// writes the caller's object, and the caller's later writes reach the
    /// callee's alias. A copy lost both (`crate::dict_adoption::RecordFlows`
    /// lifts the record and carries the view runtime for exactly this).
    fn typed_dict_adopting_rebuild(
        &self,
        rebuild: &RustExpr,
        source_key: TypeId,
        source_value: TypeId,
        target: Symbol,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !self.context.is_live_record(target)
            || crate::runtime_prelude::flag(self.context, "live_views") != Some(true)
            || !self.dict_uses_smelt_record(source_key)
            || matches!(self.mir.types.get(source_value), Some(Type::Unknown))
        {
            return Ok(None);
        }
        let wrap = if let Some(call) = self.hoisted_value_conversion(source_value)? {
            RustExpr::atom(call)
        } else if let Ok(inline) =
            self.extract_rendered(&RustExpr::path("smelt_value"), source_value, scope)
        {
            inline
        } else {
            return Ok(None);
        };
        let unwrap = if let Some(call) = self.hoisted_value_erasure(source_value)? {
            RustExpr::atom(call)
        } else if let Ok(inline) =
            self.erase_rendered(&RustExpr::path("smelt_typed").cloned_value(), source_value)
        {
            inline
        } else {
            return Ok(None);
        };
        let target_name = self.type_rust_name(target)?;
        Ok(Some(RustExpr::atom(format!(
            "match smelt_record_map.smelt_origin::<{target_name}>() {{ Some(smelt_origin) => smelt_origin, None => {{ let smelt_rebuilt = {rebuild}; smelt_record_map.smelt_adopt_typed(&smelt_rebuilt, move |smelt_value: SmeltUnknown| {wrap}, |smelt_typed| {unwrap}); smelt_rebuilt }} }}",
            rebuild = rebuild.delimited(),
            wrap = wrap.delimited(),
            unwrap = unwrap.delimited()
        ))))
    }

    /// `__smelt_from_unknown_record_<n>(smelt_value)`: the hoisted conversion
    /// of an erased value to `value`, registered on first use, which every
    /// adopting conversion of a dictionary of that value type calls
    /// ([`Self::typed_dict_adopting_rebuild`]); `None` (keep it inline) when
    /// the type's spelling here is not its module-level one, or the
    /// conversion does not render.
    ///
    /// The table is the one `hoisted_record_conversion` fills, keyed the same
    /// way by the type's Rust spelling: every entry is "this erased value as
    /// that type". Spelled inline, the conversion of a dictionary value
    /// holding callbacks is kilobytes, repeated at every site that hands such
    /// a dictionary to the record.
    fn hoisted_value_conversion(&self, value: TypeId) -> Result<Option<String>, EmitError> {
        let scoped_text = self.rendered_type(value)?.into_string();
        let hoisted = HoistedModuleItemScope::enter(self);
        let module_text = self.rendered_type(value)?.into_string();
        if module_text != scoped_text {
            return Ok(None);
        }
        let existing = self
            .context
            .minted
            .record_conversion_helpers
            .borrow()
            .get(&module_text)
            .map(|(ordinal, _)| *ordinal);
        let ordinal = if let Some(ordinal) = existing {
            ordinal
        } else {
            let reservation = helper_reservation::reserve(
                &self.context.minted,
                &self.context.minted.record_conversion_helpers,
                module_text,
            );
            let module_scope = self.render_scope();
            let Ok(body) =
                self.extract_rendered(&RustExpr::path("smelt_value"), value, &module_scope)
            else {
                drop(reservation);
                return Ok(None);
            };
            reservation.commit(body.into_string())
        };
        drop(hoisted);
        Ok(Some(format!(
            "{}{ordinal}(smelt_value)",
            crate::RECORD_CONVERSION_HELPER_PREFIX
        )))
    }

    /// `__smelt_adapt_record_<n>(smelt_typed.clone())`: the hoisted erasure
    /// of a value of type `value`, registered on first use in the
    /// `(value, SmeltUnknown)` entry of the adapter table, for the same
    /// reason as [`Self::hoisted_value_conversion`]; `None` keeps it inline.
    fn hoisted_value_erasure(&self, value: TypeId) -> Result<Option<String>, EmitError> {
        let scoped_text = self.rendered_type(value)?.into_string();
        let hoisted = HoistedModuleItemScope::enter(self);
        let module_text = self.rendered_type(value)?.into_string();
        if module_text != scoped_text {
            return Ok(None);
        }
        let key = (module_text, "SmeltUnknown".to_owned());
        let existing = self
            .context
            .minted
            .record_adapter_helpers
            .borrow()
            .get(&key)
            .map(|(ordinal, _)| *ordinal);
        let ordinal = if let Some(ordinal) = existing {
            ordinal
        } else {
            let reservation = helper_reservation::reserve(
                &self.context.minted,
                &self.context.minted.record_adapter_helpers,
                key,
            );
            let Ok(body) = self.erase_rendered(&RustExpr::path("smelt_value"), value) else {
                drop(reservation);
                return Ok(None);
            };
            reservation.commit(body.into_string())
        };
        drop(hoisted);
        Ok(Some(format!(
            "{}{ordinal}(smelt_typed.clone())",
            crate::RECORD_ADAPTER_HELPER_PREFIX
        )))
    }

    /// Emits a class/interface record as a string-keyed dictionary.
    ///
    /// Typed source records sometimes flow back into object operations. Rust
    /// structs cannot be matched as maps, so this reconstructs the source
    /// property names and converts each field to the requested map value type.
    pub(super) fn structural_record_to_string_dict_adapter(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target_key: TypeId,
        target_value: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        if self
            .mir
            .types
            .get(target_key)
            .is_none_or(|candidate| !candidate.is_text())
        {
            return Ok(None);
        }
        let Some(source_fields) = self.structural_record_fields(source) else {
            return Ok(None);
        };
        if source_fields.is_empty() {
            return Ok(None);
        }

        // A reference-class source keeps its fields inside the shared cell, so
        // the read must go through `.0.borrow()` rather than a direct named-field
        // access against the newtype.
        let source_is_reference_class = self.is_reference_class_type(source);
        let mut entries = Vec::new();
        for mut field in source_fields {
            // A redeclared field is read at the type its level stores it at.
            if let Some(stored) = self.redeclared_field_storage(source, field.name) {
                field.ty = stored;
            }
            let key = self.symbol_name(field.name)?;
            let field_name = self.context.names().field_text(self.mir, field.name);
            let source_value = if source_is_reference_class {
                RustExpr::chain(self.reference_field_read_place(
                    "smelt_struct_value",
                    source,
                    field.name,
                )?)
                .cloned_value()
            } else {
                RustExpr::path(format!("smelt_struct_value.{field_name}")).cloned_value()
            };
            let value = if let Some(Type::Optional(inner, _)) = self.mir.types.get(field.ty) {
                let mapped = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *inner,
                    target_value,
                    scope,
                )?;
                format!(
                    "{source_value}.map_or({}, |smelt_value| {mapped})",
                    self.default_value(target_value)?.delimited(),
                    source_value = source_value.tight(),
                    mapped = mapped.delimited()
                )
            } else {
                self.rendered_at_type(&source_value, field.ty, target_value, scope)?
                    .into_string()
            };
            entries.push(format!("({key:?}.to_owned(), {value})"));
        }

        let constructor = if self.dict_uses_smelt_record(target_key) {
            "SmeltRecord::from"
        } else {
            "::indexmap::IndexMap::from"
        };
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {constructor}([{}]) }}",
            entries.join(", "),
            value_text = value_text.tight()
        ))))
    }

    /// How many type parameters the class named `name` declares.
    ///
    /// `0` for a non-generic class and for a name that is not a generated class
    /// at all, so callers can ask without checking first.
    pub(in crate::emitter) fn class_type_param_count(&self, name: Symbol) -> usize {
        // The parameters the emitted struct DECLARES: a position
        // `generic_elision` dropped is spelled nowhere, so it must not be
        // spelled here either. Same rule as the type renderer's `retain_carried`.
        let declared =
            crate::classes::find_class(self.mir, name).map_or(0, |class| class.type_params.len());
        self.context
            .type_param_elision()
            .retain_carried(name, (0..declared).collect::<Vec<_>>())
            .len()
    }
}
