//! Structural record adapters, behind the coercion seam: a value of one record
//! shape rebuilt field by field as another, or as or from a string-keyed dict.

use super::*;
use crate::generic_bindings::substitute_existing_type;

impl FunctionEmitter<'_> {
    /// Returns whether `source` can be coerced before wrapping into `Option`.
    pub(super) fn can_coerce_to_optional_inner(&self, source: TypeId, inner: TypeId) -> bool {
        self.has_dynamic_shape(inner)
            || self.structural_record_adapter_available(source, inner)
            || self.string_dict_record_adapter_available(source, inner)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(inner)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (Some(Type::List(_)), Some(Type::Tuple(_)))
                    | (Some(Type::List(_)), Some(Type::List(_)))
                    | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
                    | (Some(Type::Function(_)), Some(Type::Function(_)))
            )
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

    /// Returns true when a string-keyed dictionary can fill a structural record.
    fn string_dict_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        let Some(Type::Dict(source_key, _)) = self.mir.types.get(source) else {
            return false;
        };
        self.mir.types.get(*source_key) == Some(&Type::String)
            && self.is_structural_record_adapter_target(target)
            && self
                .structural_record_fields(target)
                .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns whether an erased object can be safely expanded into a record.
    ///
    /// Expansion depth is guarded while rendering so callback-bearing records
    /// may retain nested option bags without infinitely expanding cyclic shapes.
    pub(super) fn can_extract_unknown_object_record(&self, target: TypeId) -> bool {
        self.structural_record_fields(target)
            .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns matching source/target fields for a structural record adapter.
    fn structural_record_adapter_fields(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Option<Vec<(Option<MirField>, MirField)>> {
        if source == target || !self.is_structural_record_adapter_target(target) {
            return None;
        }
        let source_fields = self.structural_record_fields(source)?;
        let target_fields = self.structural_record_fields(target)?;
        if target_fields.is_empty() {
            return None;
        }
        let field_key = |field: &MirField| self.symbol_name(field.name).ok().map(sanitize_ident);
        // A DOWNCAST: the target declares every source field and adds more
        // (`base as Derived` where `interface Derived extends Base`). `tsc`
        // only accepts an assertion between comparable types, and a target that
        // is not assignable from the source must then be assignable TO it, so
        // this shape reaches codegen only through an assertion. At runtime the
        // added properties are absent until the source assigns them
        // (`handlerSet.params = {}` right after the cast); the adapter fills
        // them with their default value, the non-optional spelling of absent.
        let is_downcast = !source_fields.is_empty()
            && source_fields.iter().all(|source_field| {
                target_fields
                    .iter()
                    .any(|target_field| field_key(target_field) == field_key(source_field))
            });
        let mut adapted_fields = Vec::new();
        for target_field in target_fields {
            let source_field = source_fields
                .iter()
                .find(|field| field_key(field) == field_key(&target_field))
                .cloned();
            if source_field.is_none()
                && !is_downcast
                && !self.is_virtual_method_storage_field(target, target_field.name)
                && !self.is_interface_method_slot_bound_by_source(source, target, target_field.name)
                && !matches!(self.mir.types.get(target_field.ty), Some(Type::Optional(_)))
            {
                return None;
            }
            adapted_fields.push((source_field, target_field));
        }
        Some(adapted_fields)
    }

    /// Returns whether a nominal Rust storage type may receive field-wise values.
    ///
    /// Interface records always use structural assignment. Class storage is
    /// also adapted when a source value exposes every destination field, which
    /// covers TypeScript base-class references emitted as concrete Rust
    /// structs.
    fn is_structural_record_adapter_target(&self, target: TypeId) -> bool {
        matches!(self.mir.types.get(target), Some(Type::Class { .. }))
    }

    /// Emits a field-wise adapter between structurally compatible record types.
    pub(in crate::emitter) fn structural_record_adapter(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
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
            return Ok(Some(RustExpr::postfix_on(&value_text, format!("{value_text}.clone()")).ending_in_clone()));
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
        // Field-wise adaptation recurses through each field's own coercion, and
        // a callable interface's `__smelt_call` field is a function whose return
        // value can be the record again (es-toolkit's
        // `CurriedFunction1<T1, R>` returns `CurriedFunction1<T1, R>`), so this
        // pair can be requested again from inside its own expansion. Truncate
        // that cycle with the target's default value rather than `None`: the
        // callers that reach an adapter here have already committed to a
        // record-shaped destination, and handing them the *source* value back
        // would be ill-typed (`CurriedFunction2` where `CurriedFunction1` is
        // expected). A default is the same information the pre-existing
        // function-typed fallbacks produce for an unresolvable callable, and it
        // keeps the generated crate compiling.
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
        let target_name = sanitize_ident(self.symbol_name(*name)?);
        let mut field_text = Vec::new();
        for (source_field_match, target_field) in adapted_fields {
            let field_name = sanitize_ident(self.symbol_name(target_field.name)?);
            let mut value_at_slot_abi = false;
            let value = if let Some(value) =
                self.virtual_method_storage_field_expr(source, target, target_field.name)?
            {
                value
            } else if let Some(value) = match &source_field_match {
                Some(source_field) => {
                    self.receiver_bound_field_slot_expr(source, target, source_field, &target_field, scope)?
                }
                None => None,
            } {
                value
            } else if let Some(source_field) = source_field_match {
                let source_field_name = sanitize_ident(self.symbol_name(source_field.name)?);
                // A reference-class source keeps its fields inside the shared
                // `Rc<RefCell<Inner>>` cell, so they are read through it.
                let source_base = if self.is_reference_class_type(source) {
                    RustExpr::chain("smelt_struct_value.0.borrow()")
                } else {
                    RustExpr::path("smelt_struct_value")
                };
                let source_value = RustExpr::postfix_on(&source_base, format!("{source_base}.{source_field_name}.clone()")).ending_in_clone();
                let adapted =
                    self.rendered_at_type(&source_value, source_field.ty, target_field.ty, scope)?;
                value_at_slot_abi = adapted == source_value
                    && self.record_slot_read_keeps_slot_abi(source, &source_field, target, &target_field);
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
            field_text.push(format!("{field_name}: {value}"));
        }
        self.push_phantom_field(&mut field_text, *name, args.len());
        // A reference-class target is a handle newtype over its `Inner` record,
        // so the field-wise value is built as the record and wrapped in a fresh
        // shared cell (a struct literal against the tuple newtype was E0560).
        // This is the structural conversion between two distinct classes TS
        // accepts because they expose the same members (Hono's `basePath`
        // returns the base class, handed to a parameter typed as the subclass
        // that adds no members); it copies the members into a new instance.
        if self.is_reference_class_type(target) {
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_struct_value = {value_text}.clone(); {target_name}(::std::rc::Rc::new(::std::cell::RefCell::new({target_name}Inner {{ {} }}))) }}",
                field_text.join(", ")
            ))));
        }
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", ")
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
            .record_adapter_helpers
            .borrow()
            .get(&module)
            .map(|(ordinal, _)| *ordinal);
        let ordinal = if let Some(ordinal) = existing {
            ordinal
        } else {
            // Reserved before the body renders (the cycle calls it); released
            // unless a body is committed (see `helper_reservation`).
            let reservation =
                helper_reservation::reserve(&self.context.record_adapter_helpers, module);
            let body = {
                // The helper's body is a fresh module-level item: the
                // expansions in progress at this call site are not its own.
                let _suspended = helper_reservation::suspend(&self.type_expansion_stack);
                let module_scope = self.render_scope();
                self.structural_record_adapter(&RustExpr::path("smelt_value"), source, target, &module_scope)
            };
            let Some(body) = body? else {
                return Ok(None);
            };
            reservation.commit(body.into_string())
        };
        drop(hoisted);
        Ok(Some(format!(
            "{}{ordinal}({value_text})",
            crate::RECORD_ADAPTER_HELPER_PREFIX
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
            let field_name = sanitize_ident(self.symbol_name(target_field.name)?);
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
                None if matches!(self.mir.types.get(target_field.ty), Some(Type::Optional(_))) => {
                    RustExpr::path("None")
                }
                None => return Ok(None),
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        let target_name = sanitize_ident(self.symbol_name(*name)?);
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        // The source is read once per field, so it is bound once: `value_text`
        // may be a call, and re-evaluating it per field would both duplicate
        // its effects and move any by-value argument more than once.
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_host_value = {value_text}; {target_name} {{ {} }} }}",
            field_text.join(", ")
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
        if self.mir.types.get(source_key) != Some(&Type::String)
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

        let target_name = sanitize_ident(self.symbol_name(*name)?);
        // The target class/interface may be parameterized by type params (e.g.
        // `CurriedFunction1<T1, _>`). Those names are only legal to spell in the
        // adapter body when they are actually in scope for the function being
        // emitted. Inside a generic class member or generic free function the
        // enclosing signature declares them, so a field default may keep the
        // generic shape. At a non-generic call site (e.g. a spec function) the
        // same name is unresolvable and must erase to `SmeltUnknown` instead of
        // printing a dangling `T1` (was E0425). Intersect the target's type-param
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
            let field_key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(field_key);
            // A JavaScript member read falls through to the prototype when the
            // own property is absent, which is what `smelt_get_object_field`
            // already does for the erased-object path. A record projection is
            // the same read against the same value, so it consults the same
            // `__smelt_proto:` slot — otherwise an erased class instance
            // projected into an interface-shaped record loses exactly the
            // methods the object path can still see, and each missing member
            // silently becomes a fabricated default callback.
            let prototype_fallback = if source_is_erased {
                format!(
                    ".or_else(|| smelt_record_map.get({proto_key:?})).or_else(|| smelt_record_map.get({method_key:?}))",
                    proto_key = format!("{SMELT_PROTO}:{field_key}"),
                    method_key = format!(
                        "{prefix}{field_key}",
                        prefix = crate::class_proto::METHOD_KEY_PREFIX,
                    ),
                )
            } else {
                String::new()
            };
            let lookup_text = if field_key.contains('_') {
                let mut camel = String::new();
                let mut upper_next = false;
                for ch in field_key.chars() {
                    if ch == '_' {
                        upper_next = true;
                    } else if upper_next {
                        camel.push(ch.to_ascii_uppercase());
                        upper_next = false;
                    } else {
                        camel.push(ch);
                    }
                }
                format!(
                    "smelt_record_map.get({field_key:?}).or_else(|| smelt_record_map.get({camel:?})){prototype_fallback}"
                )
            } else {
                format!("smelt_record_map.get({field_key:?}){prototype_fallback}")
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
            } else if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty)
                && self.record_value_misses_concrete_union(source_value, *inner)
            {
                // The record's values cannot be this field's type, so the key
                // is absent from any value the source holds.
                RustExpr::path("None")
            } else if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                if self.can_render_dict_value_as(source_value, *inner) {
                    let mapped = self.rendered_at_type(&RustExpr::path("value"), source_value, *inner, scope)?;
                    if source_is_erased
                        && !matches!(self.mir.types.get(*inner), Some(Type::Unknown))
                    {
                        // A present `null`/`undefined` is the field's ABSENT
                        // arm, not a value to convert: `score: number | null`
                        // read off `{ score: null }` is `None`, where mapping
                        // it through the number conversion answered `NaN`.
                        RustExpr::postfix_on(&lookup_value, format!(
                            "{lookup_value}.and_then(|value| if smelt_unknown_is_nullish(&value) {{ None }} else {{ Some({mapped}) }})"
                        ))
                    } else {
                        RustExpr::postfix_on(&lookup_value, format!("{lookup_value}.map(|value| {mapped})"))
                    }
                } else if self.can_render_dict_value_as(source_value, field.ty) {
                    // The dictionary value is already optional-shaped, so the
                    // `get` produced an `Option<Option<_>>`; flatten it instead of
                    // discarding the field. Without this arm the projection
                    // answered `None` for a value that was actually present.
                    let mapped = self.rendered_at_type(&RustExpr::path("value"), source_value, field.ty, scope)?;
                    RustExpr::postfix_on(&lookup_value, format!("{lookup_value}.map_or(None, |value| {mapped})"))
                } else {
                    RustExpr::path("None")
                }
            } else if self.can_render_dict_value_as(source_value, field.ty) {
                let mapped = self.rendered_at_type(&RustExpr::path("value"), source_value, field.ty, scope)?;
                RustExpr::postfix_on(&lookup_value, format!(
                    "{lookup_value}.map_or({}, |value| {mapped})",
                    self.default_value_with_scoped_type_params(
                        field.ty,
                        &TypeSubstitution::lexical_subset(&scoped_type_params),
                    )?
                ))
            } else {
                self.default_value_with_scoped_type_params(
                    field.ty,
                    &TypeSubstitution::erased(),
                )?
            };
            // A callable slot keeps the ABI its DECLARATION gave it, whatever
            // the record was instantiated at (`emitter::record_slot_abi`).
            let value = self.record_field_value_at_slot_abi(value.into_string(), target, &field)?;
            field_text.push(format!("{field_name}: {value}"));
        }
        self.push_phantom_field(&mut field_text, *name, args.len());
        // A reference class is a `Rc<RefCell<Inner>>` newtype, not a named-field
        // struct. The record round-trip must mint a fresh shared cell around the
        // reconstructed inner record rather than emit a struct literal against a
        // tuple struct (was E0560/E0609).
        //
        // A record view that came from an erasure of this reference record
        // carries the live handle, and JavaScript narrowing yields that SAME
        // object -- so the rebuild is only the answer for a view with no origin
        // (a literal, a JSON value, a spread copy). See `reference_origin`.
        if self.context.is_reference_class(*name) {
            let rebuild = RustExpr::postfix_on(&target_name, format!(
                "{target_name}(::std::rc::Rc::new(::std::cell::RefCell::new({target_name}Inner {{ {} }})))",
                field_text.join(", ")
            ));
            // Only an erased-valued `SmeltRecord` view can share an erased
            // object's store; a concretely typed dictionary never carries one.
            let body = if source_is_erased && self.dict_uses_smelt_record(source_key) {
                crate::reference_origin::restore_or_rebuild_expr("smelt_record_map", &rebuild)
            } else {
                rebuild
            };
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_record_map = {value_text}.clone(); {body} }}"
            ))));
        }
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_record_map = {value_text}.clone(); {target_name} {{ {} }} }}",
            field_text.join(", ")
        ))))
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
        if self.mir.types.get(target_key) != Some(&Type::String) {
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
        // access against the newtype (was E0609).
        let source_is_reference_class = self.is_reference_class_type(source);
        let mut entries = Vec::new();
        for field in source_fields {
            let key = self.symbol_name(field.name)?;
            let field_name = sanitize_ident(key);
            let source_value = if source_is_reference_class {
                RustExpr::chain(format!("smelt_struct_value.0.borrow().{field_name}.clone()")).ending_in_clone()
            } else {
                RustExpr::chain(format!("smelt_struct_value.{field_name}.clone()")).ending_in_clone()
            };
            let value = if let Some(Type::Optional(inner)) = self.mir.types.get(field.ty) {
                let mapped = self.rendered_at_type(&RustExpr::path("value"), *inner, target_value, scope)?;
                format!(
                    "{source_value}.map_or({}, |value| {mapped})",
                    self.default_value(target_value)?
                )
            } else {
                self.rendered_at_type(&source_value, field.ty, target_value, scope)?.into_string()
            };
            entries.push(format!("({key:?}.to_owned(), {value})"));
        }

        let constructor = if self.dict_uses_smelt_record(target_key) {
            "SmeltRecord::from"
        } else {
            "::std::collections::HashMap::from"
        };
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_struct_value = {value_text}.clone(); {constructor}([{}]) }}",
            entries.join(", ")
        ))))
    }

    /// Whether a string-keyed dictionary's value type can populate a record field.
    ///
    /// The sibling emitter modules use this to ask "would the field-wise adapter
    /// have something to put here?" without emitting the adapter — see
    /// `union::record_member_accepts_source_fields`.
    pub(in crate::emitter) fn dict_value_fits_field(&self, value: TypeId, field: TypeId) -> bool {
        self.can_render_dict_value_as(value, field)
    }

    /// Returns whether a dictionary value can be meaningfully assigned to a field.
    fn can_render_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        self.can_render_non_function_dict_value_as(source, target)
            || self.can_adapt_rendered_function_value(source, target)
    }

    /// Whether a record whose values are `source` can never hold a value of
    /// the field type `target`, because `target` is a union emitted as a
    /// concrete enum and `source` is none of its arms.
    ///
    /// Projecting `{ method: 'HEAD' }` (`Record<string, string>`) into
    /// `RequestInit` reaches `headers: Record | Headers | [string, string][]`;
    /// a string is not a header list, so the key is absent for every value the
    /// record can hold. An erased or union-typed source is still projected at
    /// run time.
    fn record_value_misses_concrete_union(&self, source: TypeId, target: TypeId) -> bool {
        source != target
            && !matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && self
                .concrete_union_members(target)
                .is_some_and(|members| !members.contains(&source))
    }

    /// Returns whether a non-callback dictionary value can populate a field.
    pub(super) fn can_render_non_function_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            // An optional field accepts the inner type (the `get` already supplies
            // the outer `Option`) or an absent value. It must ALSO accept a source
            // that is itself optional-shaped — the checks below reach that through
            // `source == target` — so fall through rather than returning here. The
            // early return dropped every field whose dictionary value carried the
            // same `Option<T>` the field declares: an object literal
            // `{ edges: ['leading'] }` projected into an interface whose `edges` is
            // `Array<...> | undefined` silently became `edges: None`.
            if self.can_render_non_function_dict_value_as(source, *inner)
                || matches!(self.mir.types.get(source), Some(Type::None))
            {
                return true;
            }
        }
        source == target
            || self.structural_record_adapter_available(source, target)
            || self.string_dict_record_adapter_available(source, target)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    // A Bun `int32` field the frontend proved every literal
                    // store into (`lowering::int32`).
                    | (Some(Type::Float), Some(Type::FixedInt(_)))
                    | (
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)),
                        _
                    )
                    | (
                        _,
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
            )
            || self.is_erased_class_type(source)
            || self.is_erased_class_type(target)
    }

    /// Returns the generated fields for a concrete class/interface storage type.
    pub(in crate::emitter) fn structural_record_fields(&self, ty: TypeId) -> Option<Vec<MirField>> {
        let Some(Type::Class { name, args }) = self.mir.types.get(ty) else {
            return None;
        };
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            let fields = crate::classes::effective_interface_fields(self.mir, interface);
            return Some(self.substitute_record_field_type_params(
                &interface.type_params,
                args,
                fields,
            ));
        }
        crate::classes::find_class(self.mir, *name)
            .map(|class| {
                self.substitute_record_field_type_params(
                    &class.type_params,
                    args,
                    crate::classes::effective_class_fields(self.mir, class),
                )
            })
    }

    /// How many type parameters the class named `name` declares.
    ///
    /// `0` for a non-generic class and for a name that is not a generated class
    /// at all, so callers can ask without checking first.
    pub(in crate::emitter) fn class_type_param_count(&self, name: Symbol) -> usize {
        // The parameters the emitted struct DECLARES: a position
        // `generic_elision` dropped is spelled nowhere, so it must not be
        // spelled here either (`Context_1::<SmeltUnknown, ..>::default()` for a
        // `Context_1` whose three parameters were all elided was E0107 in the
        // Hono crate). Same rule as the type renderer's `retain_carried`.
        let declared = crate::classes::find_class(self.mir, name)
            .map_or(0, |class| class.type_params.len());
        self.context
            .type_param_elision()
            .retain_carried(name, (0..declared).collect::<Vec<_>>())
            .len()
    }

    /// Substitute concrete class/interface arguments into structural fields.
    ///
    /// Generated storage structs keep their Rust generic parameters, but
    /// adapter emission usually works with an instantiated type such as
    /// `MatchFnResult<f64>`. Field reads must therefore use the instantiated
    /// payload type instead of the declaration-time type parameter.
    pub(in crate::emitter) fn substitute_record_field_type_params(
        &self,
        type_params: &[smelt_hir::TypeParamDef],
        args: &[TypeId],
        fields: Vec<MirField>,
    ) -> Vec<MirField> {
        // A parameter the reference leaves off takes its declared default, as
        // the type renderer does (see the `Type::Class` arm of `rust_type`),
        // so a field's type matches the struct the reference renders as.
        let substitutions = type_params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                args.get(index)
                    .copied()
                    .or(if args.is_empty() { None } else { param.default })
                    .map(|arg| (param.name, arg))
            })
            .collect::<HashMap<_, _>>();
        if substitutions.is_empty() {
            return fields;
        }
        fields
            .into_iter()
            .map(|mut field| {
                field.ty = substitute_existing_type(self.mir, field.ty, &substitutions);
                field
            })
            .collect()
    }

    /// Returns whether a generated storage type is an interface-shaped record.
    pub(in crate::emitter) fn is_interface_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. })
                if self
                    .mir
                    .interfaces
                    .iter()
                    .any(|interface| interface.name == *name)
        )
    }

    /// Returns true when `source` can be field-wise adapted to `target`.
    ///
    /// TypeScript option bags are structurally compatible, but generated Rust
    /// structs are nominal. This predicate intentionally only enables the
    /// adapter when the destination is an emitted interface record; ordinary
    /// class-to-class conversion still keeps Rust's nominal identity.
    pub(in crate::emitter) fn structural_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        self.structural_record_adapter_fields(source, target)
            .is_some()
            // A modeled host class carries its members behind accessors rather
            // than as generated fields, so the field-pairing answer above is
            // `None` for one; it is still structurally assignable to a record
            // whose every declared member it exposes. Kept in lock-step with
            // `host_class_to_record_adapter`, which emits that conversion.
            || self.host_class_record_adapter_available(source, target)
    }

    /// Whether every member a target record declares can be read off a host value.
    ///
    /// The availability half of `host_class_to_record_adapter`: an
    /// optional target field the source does not expose is absent, any other
    /// missing member means the conversion does not exist.
    fn host_class_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        if source == target || !self.exposes_host_members(source).unwrap_or(false) {
            return false;
        }
        let Some(target_fields) = self.structural_record_fields(target) else {
            return false;
        };
        if target_fields.is_empty() {
            return false;
        }
        target_fields.iter().all(|field| {
            let Ok(member) = self.symbol_source_name(field.name) else {
                return false;
            };
            match self.host_class_member_read(&RustExpr::path("smelt_host_value"), source, member) {
                Ok(Some(_)) => true,
                Ok(None) => matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))),
                Err(_) => false,
            }
        })
    }
}
