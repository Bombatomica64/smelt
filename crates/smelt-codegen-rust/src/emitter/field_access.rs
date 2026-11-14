//! Reading a named field: the storage and host-class rules for its type and
//! expression, class method references, and match-result and regexp members.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Returns the static type produced by a field read helper.
    pub(super) fn field_access_type(
        &self,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<TypeId, EmitError> {
        if let Some(Type::Dict(_, value)) = self.mir.types.get(receiver_ty) {
            return Ok(*value);
        }
        if let Some((read_ty, _)) = self.concrete_union_member_reads(receiver_ty, field)? {
            return Ok(read_ty);
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(receiver_ty)
        {
            return self.type_id(Type::Unknown);
        }
        // A `String` receiver's builtin fields have concrete Rust types. The
        // text and the type are decided together by `string_field_read` so a
        // caller coercing the read cannot treat an already concrete value as an
        // erased `SmeltUnknown` (e.g. `.length` is an `i64`, not a
        // `SmeltUnknown::Number`).
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::String | Type::PyStr)
        ) {
            return Ok(self.string_field_read(&RustExpr::loose(""), field)?.1);
        }
        if let Some(property) = self.host_data_property(receiver_ty, field)? {
            return self.type_id(match property.ty {
                smelt_stdlib::HostPrimitive::Number => Type::Float,
                smelt_stdlib::HostPrimitive::Bool => Type::Bool,
                smelt_stdlib::HostPrimitive::String => Type::String,
                _ => {
                    return Err(EmitError::new(
                        "host data property of an unmodeled primitive type",
                    ));
                }
            });
        }
        // Likewise for a concrete `RegExp` receiver (see `regexp_field_expr`).
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return match smelt_stdlib::regexp_member(self.symbol_name(field)?) {
                Some(kind) => self.regexp_member_ty(kind),
                None => self.type_id(Type::Unknown),
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
        let Some(Type::Class { name, args }) = self.mir.types.get(receiver_ty) else {
            return self.type_id(Type::Unknown);
        };
        // The declared field type is written in the shape's own parameters
        // (`next?: Cell<T>`); a read off `Cell<string>` has that type with
        // `T := string`. Returning the declared `T` made the read look erased
        // and coerced a concrete `JsString` from `SmeltUnknown`.
        let (params, declared) = if let Some(class) = crate::classes::find_class(self.mir, *name) {
            (
                class
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect::<Vec<_>>(),
                crate::classes::effective_class_fields(self.mir, class)
                    .into_iter()
                    .find(|class_field| class_field.name == field)
                    .map(|class_field| class_field.ty),
            )
        } else if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            (
                interface
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect(),
                interface
                    .fields
                    .iter()
                    .find(|item| item.name == field)
                    .map(|item| item.ty),
            )
        } else {
            return self.type_id(Type::Unknown);
        };
        let Some(declared_ty) = declared else {
            return self.type_id(Type::Unknown);
        };
        if params.is_empty() || params.len() != args.len() {
            return Ok(declared_ty);
        }
        let substitutions: HashMap<Symbol, TypeId> =
            params.into_iter().zip(args.iter().copied()).collect();
        Ok(self.substitute_type_params_late(declared_ty, &substitutions))
    }

    /// `ty` with each type parameter in `substitutions` replaced, interning
    /// any new composite late (the emitter's interner is otherwise frozen).
    pub(super) fn substitute_type_params_late(
        &self,
        ty: TypeId,
        substitutions: &HashMap<Symbol, TypeId>,
    ) -> TypeId {
        let Some(kind) = self.mir.types.get(ty) else {
            return ty;
        };
        if let Type::TypeParam { name } = kind {
            return substitutions.get(name).copied().unwrap_or(ty);
        }
        let Some(rewritten) = kind
            .try_map_children(|child| Some(self.substitute_type_params_late(child, substitutions)))
        else {
            return ty;
        };
        if rewritten == *kind {
            return ty;
        }
        self.mir.types.intern_late(rewritten)
    }

    /// The modeled host data property `field` names on a `receiver_ty`
    /// receiver, if any (`smelt_stdlib::host_data_property`): a value the
    /// runtime type COMPUTES, so it is read through an accessor, never a field.
    pub(super) fn host_data_property(
        &self,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<Option<smelt_stdlib::HostDataProperty>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Ok(None);
        };
        // A source class of the same name is that class, with its own members.
        if self.mir.has_class(*name) {
            return Ok(None);
        }
        let Some(class) = smelt_stdlib::typescript_stdlib_class(self.symbol_name(*name)?) else {
            return Ok(None);
        };
        Ok(smelt_stdlib::host_data_property(
            class,
            self.symbol_name(field)?,
        ))
    }

    /// The expression reading `field` off a receiver rendered as `receiver_text`
    /// of type `receiver_ty`: a host data property, a keyed read on a text-keyed
    /// dictionary (absent keys following the value type), or a struct field.
    pub(super) fn field_access_expr(
        &self,
        receiver_text: &RustExpr,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        if let Some(property) = self.host_data_property(receiver_ty, field)? {
            return Ok(host_data_property_read(receiver_text, &property));
        }
        if let Some(Type::Dict(key, value)) = self.mir.types.get(receiver_ty)
            && self.mir.types.get(*key).is_some_and(Type::is_text)
        {
            let field_name = self.symbol_source_name(field)?;
            // The lookup of a source property name: a JavaScript record or map
            // is keyed by `JsString`, which a `&str` does not borrow as.
            let js_keyed = self.mir.types.get(*key) == Some(&Type::String);
            let lookup = if js_keyed && self.dict_uses_smelt_record(*key) {
                format!("get_text({field_name:?})")
            } else if js_keyed {
                format!("get(&*JsString::from({field_name:?}))")
            } else {
                format!("get({field_name:?})")
            };
            if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                if self.dict_uses_smelt_record(*key) {
                    return Ok(receiver_text.postfix(format!(".{lookup}.flatten()")));
                }
                return Ok(receiver_text.postfix(format!(".{lookup}.cloned().flatten()")));
            }
            // Same rule as the plain place read (`emitter::place`): a record
            // whose values are erased reads a key it does not hold as
            // `undefined`. An optional-chained read (`options?.times`) of an
            // optional property reaches here, and must not panic on the absent
            // key the source type explicitly allows.
            if self.renders_erased(*value)? {
                let cloned = if self.dict_uses_smelt_record(*key) {
                    ""
                } else {
                    ".cloned()"
                };
                return Ok(receiver_text.postfix(format!(
                    ".{lookup}{cloned}.unwrap_or({})",
                    self.absent_value()?.delimited()
                )));
            }
            if self.dict_uses_smelt_record(*key) {
                return Ok(receiver_text.postfix(format!(".{lookup}.expect(\"missing field\")")));
            }
            return Ok(
                receiver_text.postfix(format!(".{lookup}.cloned().expect(\"missing field\")"))
            );
        }
        if let Some(text) =
            self.concrete_union_member_read_expr(receiver_text, receiver_ty, field)?
        {
            return Ok(text);
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
                self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        "{receiver_text}.clone().into_smelt_unknown()",
                        receiver_text = receiver_text.tight()
                    ),
                )
            } else {
                receiver_text.to_owned().into_string()
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
                return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "match {scrutinee} {{ SmeltUnknown::Object(smelt_map) if smelt_host_method(&smelt_map, {field_name:?}).is_some() => smelt_host_method(&smelt_map, {field_name:?}).unwrap_or(SmeltUnknown::Undefined), SmeltUnknown::Object(smelt_map) => match smelt_map.get({field_name:?}).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(mut smelt_getter) if smelt_getter.contains_key(\"{SMELT_GET}\") => match smelt_getter.remove(\"{SMELT_GET}\") {{ Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)), _ => SmeltUnknown::Null }}, value => value }}, _ => SmeltUnknown::Null }}"
                ))));
            }
            // Every receiver shape through the ONE prelude read chain
            // (`smelt_get_unknown_field`), exactly as the place-expression path
            // in `place.rs` already does. Inlining an object-only `match` here
            // made `v.k` and `v[k]` disagree the moment the erased receiver was
            // not a record: an erased ARRAY carries `length`, its element
            // indices and its identity-keyed named properties, and a regex match
            // result IS such an array — so `match.groups` read as a constant
            // `null` on this path while the same read through `place.rs`
            // answered correctly. The `Object.prototype` sentinel, a function's
            // own property bag and the marker records (`Map.size`, boxed
            // primitives, builtin namespaces) come along for free, and a genuine
            // miss is `undefined` — the JavaScript answer — rather than `null`.
            //
            // `receiver_text` may already be a borrow (an `.as_ref().map(…)`
            // closure parameter) or an owned value, and the helper takes
            // `&SmeltUnknown`. `Borrow::borrow` is the one spelling that accepts
            // both — `impl Borrow<T> for T` and `impl Borrow<T> for &T` — with
            // the target fixed by the helper's parameter type, so neither shape
            // needs its own emission.
            return Ok(RustExpr::chain(format!(
                "smelt_get_unknown_field(::std::borrow::Borrow::borrow(&{scrutinee}), {field_name:?})"
            )));
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::String | Type::PyStr)
        ) {
            return self.string_field_expr(receiver_text, field);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return self.regexp_field_expr(receiver_text, field);
        }
        if self.is_url_search_params_class_type(receiver_ty)? {
            return self.url_search_params_field_expr(receiver_text, field);
        }
        // A `SmeltMatch`/`MatchGroups` receiver reached through an optional chain
        // (e.g. `withoutSeparator?.groups.result`) must keep its typed match
        // accessor: a named-group read routes through `named_group_owned` rather
        // than falling through to raw struct field access, which has no such
        // field on `SmeltMatch`.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && let Some(kind) = self.match_class_kind(*name)?
        {
            return self.match_field_expr(receiver_text, kind, field);
        }
        if self.storage_field_is_function(receiver_ty, field) {
            return Ok(receiver_text
                .postfix(format!(
                    ".{}",
                    self.context.names().field_text(self.mir, field)
                ))
                .cloned_value());
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Err(EmitError::new(format!(
                "optional field codegen requires a class or string-keyed dict receiver, got {} for field `{}`",
                Self::type_text_for(self.mir, receiver_ty)
                    .unwrap_or_else(|_error| format!("{receiver_ty:?}")),
                self.symbol_source_name(field).unwrap_or_default()
            )));
        };
        if let Some(method_text) = self.class_method_reference_expr(receiver_text, *name, field)? {
            return Ok(method_text);
        }
        // A REFERENCE class is an `Rc<RefCell<Inner>>` handle, so its fields are
        // reached through the handle, never as tuple-struct fields of the handle
        // itself. The ordinary place path already knows this
        // (`place_is_reference_class_field`); this path did not, so a field read
        // through an optional chain on such a class emitted
        // `handle.field.clone()` and the generated crate failed to compile
        // (E0609). It was unreachable until an assignment through an
        // optional-typed receiver stopped being rejected in MIR.
        if self.is_reference_class_type(receiver_ty)
            && self.class_has_named_field(receiver_ty, field)
        {
            return Ok(receiver_text
                .postfix(format!(
                    ".0.borrow().{}",
                    self.context.names().field_text(self.mir, field)
                ))
                .cloned_value());
        }
        Ok(receiver_text
            .postfix(format!(
                ".{}",
                self.context.names().field_text(self.mir, field)
            ))
            .cloned_value())
    }

    /// Emits a bound closure for class method references such as `this.set`.
    ///
    /// TypeScript methods are callable values when read from an instance. Rust
    /// methods are not stored fields, so generated code captures a cloned
    /// receiver and forwards callback invocations through the inherent method.
    pub(super) fn class_method_reference_expr(
        &self,
        receiver_text: &RustExpr,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
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
            return self.abstract_class_method_reference_expr(class, method);
        };
        let method_name = self.method_rust_name(method)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let args = function
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, param)| {
                let param_ty = self.function_local_decl(function, *param)?.ty;
                let item = RustExpr::chain(self.carrier_format(
                    self.carrier()?,
                    format_args!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)"),
                ));
                self.rendered_at_type(&item, unknown_ty, param_ty, &self.render_scope())
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let call = RustExpr::chain(format!("smelt_receiver.{method_name}({})", args.join(", ")));
        let returned = if function.can_throw && !function.is_generator {
            RustExpr::atom(format!("{call}?", call = call.tight()))
        } else {
            call
        };
        let wrapped_returned = self.erase_rendered(&returned, function.return_ty)?;
        let receiver_clone = if receiver_text.is_receiver() {
            self.self_struct_clone_expr(class)?
        } else {
            receiver_text.cloned_value()
        };
        // Two reads of one method are `===` in JavaScript, because the method
        // lives once on the prototype. The receiver capture makes each read a
        // fresh allocation, so link it to the method's canonical identity (see
        // `class_proto::method_identity_expr`) instead of leaving reference
        // equality to compare two unrelated addresses.
        let Some(identity) = crate::class_proto::method_identity_expr(self.mir, function)? else {
            return Ok(Some(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_receiver = {receiver_clone}; SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, {ERROR_BOX}>({wrapped_returned}))) }}", receiver_clone = receiver_clone.delimited(), wrapped_returned = wrapped_returned.delimited()
            )))));
        };
        Ok(Some(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let smelt_receiver = {receiver_clone}; let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, {ERROR_BOX}>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, {ERROR_BOX}>({wrapped_returned})); smelt_link_function_identity_key(&smelt_method, {identity}); SmeltUnknown::Function(smelt_method) }}", receiver_clone = receiver_clone.delimited(), wrapped_returned = wrapped_returned.delimited(), identity = identity.delimited()
        )))))
    }

    /// Emits an erased callable placeholder for abstract method signatures.
    ///
    /// Abstract methods have no body to emit, but source code can still pass the
    /// method reference through generic callback storage on an abstract base.
    /// The concrete override is a later virtual-dispatch problem; this keeps the
    /// abstract slice type-checkable without inventing a stored field.
    fn abstract_class_method_reference_expr(
        &self,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(class_item) = crate::classes::find_class(self.mir, class) else {
            return Ok(None);
        };
        if !class_item
            .abstract_methods
            .iter()
            .any(|candidate| candidate.name == method)
        {
            return Ok(None);
        }
        Ok(Some(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!("SmeltUnknown::Function(::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Null)))")))))
    }

    /// Emits a concrete clone of `self` without relying on generic `Clone` bounds.
    fn self_struct_clone_expr(&self, class: Symbol) -> Result<RustExpr, EmitError> {
        // A REFERENCE class has exactly one field -- the `Rc<RefCell<Inner>>`
        // handle -- so the field-by-field rebuild below cannot spell it
        // (`struct PatternRouter<SmeltUnknown> has no field named _routes`,
        // E0560/E0609 in the router slice). `self.clone()` is also the only
        // spelling with the right MEANING: a method read off an object is bound
        // to THAT object, and cloning the handle shares its cell, where a
        // rebuild would hand the closure a copy whose mutations no one sees.
        // This is the same receiver capture `class_proto` uses for the
        // prototype table.
        if self.context.is_reference_class(class) {
            return Ok(RustExpr::path("self").cloned_value());
        }
        let Some(class_item) = crate::classes::find_class(self.mir, class) else {
            return Ok(RustExpr::loose("*self").cloned_value());
        };
        let mut class_name = crate::classes::class_name_text(self.context, class_item)?;
        // Only the parameters the emitted struct declares are spelled: an
        // elided position has no slot to fill (see `crate::generic_elision`).
        let carried_params = self.context.type_param_elision().retain_carried(
            class_item.name,
            class_item.type_params.iter().collect::<Vec<_>>(),
        );
        if !carried_params.is_empty() {
            self.context.note_carrier_use();
            let args = carried_params
                .iter()
                .map(|_| "SmeltUnknown")
                .collect::<Vec<_>>()
                .join(", ");
            class_name = format!("{class_name}::<{args}>");
        }
        let mut fields = crate::classes::effective_class_fields(self.mir, class_item)
            .into_iter()
            .map(|field| {
                let name = self.context.names().field_text(self.mir, field.name);
                Ok(format!("{name}: self.{name}.clone()"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if !carried_params.is_empty() {
            fields.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        Ok(RustExpr::loose(format!(
            "{class_name} {{ {} }}",
            fields.join(", ")
        )))
    }

    /// Emits JavaScript RegExp-like metadata fields for regex strings.
    ///
    /// The frontend currently represents regex literals as pattern strings.
    /// Remeda tests still read fields such as `.source` and `.global` after a
    /// clone, so these field reads need to stay on the string representation
    /// instead of becoming Rust struct projection.
    pub(super) fn string_field_expr(
        &self,
        receiver_text: &RustExpr,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        Ok(self.string_field_read(receiver_text, field)?.0)
    }

    /// A `String` receiver's builtin field read, as Rust text PAIRED with the
    /// static type of that text.
    ///
    /// The two must be decided together, or the text says `i64` while the type
    /// lookup says `Unknown`, and a caller coercing the read runs the erased
    /// `SmeltUnknown` ToString match over an `i64` expression. Every reader of
    /// either half goes through here: `string_field_expr`,
    /// [`Self::field_access_type`], `place_ty`, and the erasing coercion path.
    ///
    /// `.length` is a JavaScript Number, so its natural Rust spelling depends on
    /// which numeric type the program actually interned: the type table is fixed
    /// before emission and cannot be extended, and naming a type it does not
    /// hold is what the divergence above was made of. `Int` is preferred when
    /// present (it is what a character count is), then `Float`; a program that
    /// interned neither — one that only ever stringifies the length — gets the
    /// runtime-tagged number, which is a real boundary rather than a
    /// convenience: there is no numeric type in that program to carry it.
    pub(super) fn string_field_read(
        &self,
        receiver_text: &RustExpr,
        field: Symbol,
    ) -> Result<(RustExpr, TypeId), EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        Ok(match self.symbol_name(field)? {
            "source" => (receiver_text.cloned_value(), self.type_id(Type::String)?),
            "global" | "ignoreCase" | "ignore_case" | "multiline" => {
                match self.existing_type_id(Type::Bool) {
                    Some(bool_ty) => (RustExpr::path("false"), bool_ty),
                    None => (
                        self.carrier_expr(
                            self.carrier()?,
                            &RustExpr::atom("SmeltUnknown::Bool(false)"),
                        ),
                        unknown_ty,
                    ),
                }
            }
            "constructor" => (
                self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Null")),
                unknown_ty,
            ),
            "length" => {
                let count = self.string_len_text(&receiver_text.parenthesized_if_needed());
                if let Some(int_ty) = self.existing_type_id(Type::Int) {
                    (RustExpr::atom(format!("({count} as i64)")), int_ty)
                } else if let Some(float_ty) = self.existing_type_id(Type::Float) {
                    (RustExpr::atom(format!("({count} as f64)")), float_ty)
                } else {
                    (
                        RustExpr::atom(self.carrier_format(
                            self.carrier()?,
                            format_args!("SmeltUnknown::Number({count} as f64)"),
                        )),
                        unknown_ty,
                    )
                }
            }
            // A property a string does not have reads `undefined`.
            _ => (
                self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Undefined")),
                unknown_ty,
            ),
        })
    }

    /// Emits JavaScript RegExp metadata field reads.
    pub(super) fn regexp_field_expr(
        &self,
        receiver_text: &RustExpr,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        Ok(match self.symbol_name(field)? {
            "source" => RustExpr::chain(format!(
                "JsString::from(&*{receiver_text}.source)",
                receiver_text = receiver_text.tight()
            )),
            "flags" => RustExpr::chain(format!(
                "JsString::from(&*{receiver_text}.flags)",
                receiver_text = receiver_text.tight()
            )),
            "global" => receiver_text.postfix(".has_flag('g')"),
            "ignoreCase" | "ignore_case" => receiver_text.postfix(".has_flag('i')"),
            "multiline" => receiver_text.postfix(".has_flag('m')"),
            "sticky" => receiver_text.postfix(".has_flag('y')"),
            "unicode" => receiver_text.postfix(".has_flag('u')"),
            "dotAll" | "dot_all" => receiver_text.postfix(".has_flag('s')"),
            "hasIndices" | "has_indices" => receiver_text.postfix(".has_flag('d')"),
            "unicodeSets" | "unicode_sets" => receiver_text.postfix(".has_flag('v')"),
            "lastIndex" | "last_index" => {
                // Parenthesize the cast: the read may be followed by a postfix
                // `.clone()` (or a comparison), and a bare `... as f64.clone()`
                // mis-parses the `.clone()` as part of the cast target type.
                RustExpr::atom(format!(
                    "(*{receiver_text}.last_index.borrow() as f64)",
                    receiver_text = receiver_text.tight()
                ))
            }
            "constructor" => RustExpr::atom(
                "SmeltUnknown::Object(SmeltObject::new(Vec::<(JsString, _)>::new()))",
            ),
            // A property a RegExp does not have reads `undefined`.
            _ => self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Undefined")),
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
    pub(super) fn match_field_expr(
        &self,
        receiver_text: &RustExpr,
        kind: smelt_stdlib::StdlibClass,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        let field_name = self.symbol_source_name(field)?;
        match kind {
            smelt_stdlib::StdlibClass::MatchGroups => {
                Ok(receiver_text.postfix(format!(".named_group_owned({field_name:?})")))
            }
            _ => Ok(match field_name {
                "index" => receiver_text.postfix(".index()"),
                "length" => receiver_text.postfix(".length()"),
                "input" => receiver_text.postfix(".input_owned()"),
                // `match.groups` is the same underlying `SmeltMatch` value; a
                // subsequent named-group read resolves through the
                // `MatchGroups` branch above.
                "groups" => receiver_text.cloned_value(),
                "indices" => receiver_text.postfix(".indices()"),
                // A property a match does not have reads `undefined`.
                _ => self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Undefined")),
            }),
        }
    }

    /// Emits a numbered capture-group read against a concrete `SmeltMatch`.
    ///
    /// `match[n]` reads the n-th numbered group (entry 0 is the whole match) as
    /// an owned `Option<String>` — `None` for a group that did not participate,
    /// matching JavaScript `undefined`.
    pub(super) fn match_index_expr(
        &self,
        receiver_text: &RustExpr,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_expr(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(receiver_text.postfix(format!(
            ".group_owned({index_text} as usize)",
            index_text = index_text.as_cast_operand()
        )))
    }

    /// Emits an `Option<T>`-returning keyed read against a `Dict`-typed store.
    ///
    /// Shared by optional dict index reads and by class index-signature store
    /// reads: both read a value by key from a `Dict(key, value)` and
    /// yield `Option<value>` where a missing key is `None` (JavaScript
    /// `undefined`). String keys coerce the index through
    /// `property_key_to_string_expr`; a `SmeltRecord`/`SmeltJsMap` store already
    /// returns an owned `Option`, while a plain `HashMap` needs `.cloned()`.
    ///
    /// When the store's VALUE type is itself optional the result is flattened,
    /// because TypeScript does not distinguish a missing key from a key holding
    /// `undefined` — both read as `undefined` — so HIR collapses
    /// `Optional(Optional(T))` to `Optional(T)` and the emitted read must
    /// collapse with it. Without the flatten this helper answered
    /// `Option<Option<T>>` into a destination declared `Option<T>`: hono's
    /// `MatcherMap<T> = Record<string, Matcher<T>>` read as `matchers[method]`
    /// (`src/matcher.rs:37`, `:69`) is that shape. The place-read path in
    /// `place.rs` has always flattened here, so passing the value type is what
    /// stops the two spellings of one read from disagreeing.
    pub(super) fn dict_index_optional_read_expr(
        &self,
        store_text: &RustExpr,
        key_ty: TypeId,
        value_ty: TypeId,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let key_text = if self.mir.types.get(key_ty).is_some_and(Type::is_text) {
            let index_ty = self.operand_ty(index)?;
            if index_ty == key_ty {
                self.value_at_type(index, key_ty)?
            } else {
                self.property_key_to_string_expr(&self.operand_expr(index)?, index_ty)?
            }
        } else {
            self.value_at_type(index, key_ty)?
        };
        let flatten = if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
            ".flatten()"
        } else {
            ""
        };
        if self.dict_uses_smelt_record(key_ty) || self.dict_uses_js_key_map(key_ty) {
            Ok(RustExpr::opaque(format!(
                "{store_text}.get(&{key_text}){flatten}",
                store_text = store_text.tight(),
                key_text = key_text.tight()
            )))
        } else {
            Ok(RustExpr::opaque(format!(
                "{store_text}.get(&{key_text}).cloned(){flatten}",
                store_text = store_text.tight(),
                key_text = key_text.tight()
            )))
        }
    }
}

/// The read of a host data property off `receiver_text`: its accessor call,
/// with a text property (a URL part, a status text, an encoding name), which
/// the host type answers as a UTF-8 `String`, taken as a JavaScript string.
pub(super) fn host_data_property_read(
    receiver_text: &RustExpr,
    property: &smelt_stdlib::HostDataProperty,
) -> RustExpr {
    let call = format!(
        "{receiver_text}.{}()",
        property.accessor,
        receiver_text = receiver_text.tight()
    );
    if property.ty == smelt_stdlib::HostPrimitive::String {
        return RustExpr::chain(format!("JsString::from({call})"));
    }
    RustExpr::postfix_on(receiver_text, call)
}
