//! The `extract` verbs: pulling a typed value back out of `SmeltUnknown`, per
//! type, and the record and call-argument conversions that needs.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits extraction from `SmeltUnknown` into a concrete Rust type.
    ///
    /// JavaScript and Python code often narrows dynamic values through guards
    /// the frontend cannot fully preserve after generic or regex surfaces erase
    /// the shape. Keep primitive extraction total where the source language has
    /// a defined coercion/default instead of turning those paths into generated
    /// Rust panics.
    pub(in crate::emitter) fn extract(&self, value: &Operand, target: TypeId, scope: &RenderScope) -> Result<RustExpr, EmitError> {
        // The primitive-target arms of `extract_rendered` (`Bool`/`Int`/
        // `Float`/`String`) match directly on a `SmeltUnknown` scrutinee. A
        // caller that reaches here with an already-concrete `Option` source whose
        // inner type is itself concrete — e.g. an `Option<String>` array element
        // coerced to a `String` method receiver — must instead go through the
        // general concrete coercion, which unwraps the `Option`. (List/Dict and
        // other targets normalize their source through `into_smelt_unknown`
        // first, so they still accept a non-erased source.) `value_at_type`
        // handles a non-erased `Option` source directly without delegating back
        // to `extract`, so this cannot recurse.
        let source_ty = self.operand_ty(value)?;
        let target_is_primitive = matches!(
            self.mir.types.get(target),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        );
        let source_inner_is_erased = matches!(
            self.mir.types.get(source_ty),
            Some(Type::Optional(inner)) if self.has_dynamic_shape(*inner)
        );
        // The same holds for a FUNCTION target that is the optional's own inner
        // type: a narrowed `Option<Rc<dyn Fn ..>>` unwraps to its function. The
        // erased path below matched it as a `SmeltUnknown`, which an
        // `Option<Rc<dyn Fn>>` is not (E0308), so it never compiled. Other
        // inner types keep the erased extraction and its handling of an absent
        // value (e.g. `Array.from(null)` reaching an iterable extraction).
        let target_is_source_inner = matches!(
            self.mir.types.get(source_ty),
            Some(Type::Optional(inner)) if *inner == target
        ) && matches!(self.mir.types.get(target), Some(Type::Function(_)));
        if (target_is_primitive || target_is_source_inner)
            && matches!(self.mir.types.get(source_ty), Some(Type::Optional(_)))
            && !source_inner_is_erased
        {
            return self.value_at_type(value, target);
        }
        // A bare function value (`Rc<dyn Fn ...>`) crossing into a record/list
        // extraction cannot be fed straight to the `into_smelt_unknown()` arms:
        // the erased-function Rust type does not implement `IntoSmeltUnknown`.
        // Wrap it into a `SmeltUnknown::Function` at the boundary first, then
        // extract from that erased value (was E0599 in `isMatchWith`/`toolkit`).
        if matches!(self.mir.types.get(source_ty), Some(Type::Function(_))) {
            let erased = self.erase(value)?;
            return self.extract_rendered(&erased, target, scope);
        }
        // A RECORD source needs the same normalization for the same reason. The
        // fetch-runtime arm of `extract_rendered` hands its text straight to
        // `SmeltFromUnknown::smelt_from_unknown`, which takes a `SmeltUnknown`,
        // so an object literal flowing into a `Response`/`Headers` position
        // (`{}` at a `Response`-returning slot) reached it as a `SmeltRecord`
        // and produced an E0308 naming neither the source line nor the reason.
        // Erasing here keeps `extract_rendered`'s contract — its text IS an
        // erased value — instead of teaching one of its arms to re-derive the
        // source type it was never given.
        if matches!(self.mir.types.get(source_ty), Some(Type::Dict(_, _)))
            && matches!(self.mir.types.get(target), Some(Type::Class { .. }))
            && self.is_fetch_runtime_class_type(target)?
        {
            let erased = self.erase(value)?;
            return self.extract_rendered(&erased, target, scope);
        }
        let text = self.operand_expr(value)?;
        self.extract_rendered(&text, target, scope)
    }

    /// The erased-record-to-class conversion of `text`, spelled inline.
    ///
    /// Rebuilds the class from the erased object's record (or recovers the
    /// live instance a reference record's view carries), converting each
    /// field with its own nested conversion. `extract_rendered` either
    /// inlines this or, for an outermost conversion, hoists it into one
    /// generated helper per type (`hoisted_record_conversion`).
    fn inline_record_conversion(
        &self,
        text: &RustExpr,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<String>, EmitError> {
        // The string-dict record adapter needs the prelude `String` and
        // `SmeltUnknown` types interned in this program's type table. A
        // program that never uses them (e.g. a concrete class union whose
        // generated `from_smelt_unknown` reconstruction reaches this class
        // member) has neither interned, so fall back to the same default
        // the no-adapter arm below produces rather than failing emission.
        let (Some(string_ty), Some(unknown_ty)) = (
            self.find_type_id(&Type::String),
            self.find_type_id(&Type::Unknown),
        ) else {
            return Ok(None);
        };
        let conversion_entry = helper_reservation::push_entry(&self.record_conversion_stack, target);
        let adapter_result = self.string_dict_record_adapter(
            &RustExpr::path("smelt_record_map"),
            string_ty,
            unknown_ty,
            target,
                            scope,
        );
        drop(conversion_entry);
        if let Some(adapter) = adapter_result? {
            // A reference record's adapter first asks the record view
            // for the live instance it stands for, and that view only
            // carries it when it SHARES the erased object's store;
            // a copied entry list would drop it (`reference_origin`).
            let record_view = if self.is_reference_class_type(target) {
                "values.smelt_shared_record()"
            } else {
                "SmeltRecord::with_id_from_entries(values.id, values.into_iter())"
            };
            // A callable-object struct (`LazyMeta & ((...args) => LazyFn)`,
            // stored as `{ single, __smelt_call }`) is, in JavaScript, a
            // FUNCTION carrying own properties. An erased plain function is
            // therefore a value of it: its `__smelt_call` is the function
            // itself and every other field is the function's own property
            // (absent when never assigned). Defaulting it instead replaced
            // the function with a no-op that answers `null` (remeda's
            // `purryFromLazy` handed every `lazy` through this path, so
            // `pipe` ran a no-op evaluator and every lazy pipe came back
            // empty).
            let function_arm = if let Some(fields) = self.callable_object_record_keys(target)? {
                let entries = fields
                    .iter()
                    .map(|key| {
                        format!(
                            "({key:?}.to_owned(), smelt_get_unknown_field(&smelt_function_value, {key:?}))"
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                self.carrier_format(format_args!(
                    " smelt_function_value @ SmeltUnknown::Function(_) => {{ let smelt_record_map = SmeltRecord::from_iter([(\"{SMELT_CALL}\".to_owned(), smelt_function_value.clone()), {entries}].into_iter().filter(|(_, value)| !matches!(value, SmeltUnknown::Undefined))); {adapter} }},"
                ))
            } else {
                String::new()
            };
            return Ok(Some(self.carrier_format(format_args!(
                "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => {{ let smelt_record_map = {record_view}; {adapter} }},{function_arm} _ => Default::default() }}"
            ))));
        }
        Ok(None)
    }

    /// The source keys of a callable-object struct's data fields (every
    /// field but `__smelt_call`), or `None` when `target` is not a class or
    /// interface with a `__smelt_call` slot.
    fn callable_object_record_keys(&self, target: TypeId) -> Result<Option<Vec<String>>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let fields = if let Some(class) = crate::classes::find_class(self.mir, *name) {
            class.fields.clone()
        } else if let Some(interface) = self.mir.interface_named(*name) {
            crate::classes::effective_interface_fields(self.mir, interface)
        } else {
            return Ok(None);
        };
        let mut has_call = false;
        let mut keys = Vec::new();
        for field in &fields {
            let key = self.symbol_source_name(field.name)?;
            if key == SMELT_CALL {
                has_call = true;
            } else {
                keys.push(key.to_owned());
            }
        }
        Ok(has_call.then_some(keys))
    }

    /// Rebuilds generated record `target` from erased `text` through its own
    /// `SmeltFromUnknown` impl. This is the conversion for a program whose type
    /// table lacks what the string-dict adapter needs; a record default in its
    /// place would drop every field the value holds.
    fn generated_record_from_unknown_expr(&self, text: &RustExpr, target: TypeId) -> Result<RustExpr, EmitError> {
        let target_text = self.rendered_type(target)?;
        Ok(RustExpr::loose(self.carrier_format(format_args!(
            "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
        ))))
    }

    /// `__smelt_from_unknown_record_<n>((text).into_smelt_unknown())`, the call
    /// to the hoisted conversion for `target`, registering its body on first use.
    ///
    /// Returns `None` (keep the inline spelling) when the type's Rust spelling
    /// at this position differs from its module-level spelling (it mentions a
    /// type parameter in scope, which a module-level `fn` cannot name). The
    /// body is rendered in the module-level scope with the conversion stack
    /// holding only `target`, exactly the state an outermost inline
    /// conversion is rendered in, so hoisting changes where the text lives
    /// and not what it does.
    pub(in crate::emitter) fn hoisted_record_conversion(&self, text: &RustExpr, target: TypeId) -> Result<Option<String>, EmitError> {
        let scoped_text = self.rendered_type(target)?.into_string();
        let hoisted = HoistedModuleItemScope::enter(self);
        let module_text = self.rendered_type(target)?.into_string();
        if module_text != scoped_text {
            return Ok(None);
        }
        let existing = self.context.record_conversion_helpers.borrow().get(&module_text).map(|(ordinal, _)| *ordinal);
        let ordinal = if let Some(ordinal) = existing {
            ordinal
        } else {
            // Reserve the helper BEFORE rendering its body: a record that
            // reaches its own type again (`swap(): Pair<T>` inside `Pair<T>`)
            // converts that inner value by calling this same helper, and the
            // recursion guard in `extract_rendered` needs the ordinal to name
            // it. The call is lazy (it runs when `swap` is called), so the
            // generated recursion is finite. The reservation is released on
            // every path that does not commit a body (see `helper_reservation`).
            let reservation = helper_reservation::reserve(
                &self.context.record_conversion_helpers,
                module_text,
            );
            let module_scope = self.render_scope();
            let Some(body) =
                self.inline_record_conversion(&RustExpr::path("smelt_value"), target, &module_scope)?
            else {
                // Nothing to hoist, and the caller's fallback cannot have
                // called this reservation.
                drop(reservation);
                return Ok(None);
            };
            reservation.commit(body)
        };
        drop(hoisted);
        Ok(Some(self.carrier_format(format_args!(
            "{}{ordinal}(({text}).into_smelt_unknown())",
            crate::RECORD_CONVERSION_HELPER_PREFIX
        ))))
    }

    /// Emits checked extraction from an already-rendered `SmeltUnknown` value.
    ///
    /// `scope` is the render position's type-parameter environment, threaded in
    /// by [`Self::rendered_at_type`] (or built by an ordinary emit site with
    /// `self.render_scope()`). The `Type::TypeParam` arm below recovers the
    /// value as the Rust generic exactly when `scope` spells that name, which is
    /// the same query the coercion path and the container annotation ask — so
    /// the recovered entries can no longer carry a `T` the annotation erased,
    /// or vice versa.
    pub(in crate::emitter) fn extract_rendered(
        &self,
        text: &RustExpr,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = text.cloned_value();
        match self.mir.types.get(target) {
            Some(Type::Unknown) => Ok(text.to_owned()),
            // An erased `Date` keeps the marker record; the concrete `SmeltDate`
            // recovers through its own adapter (the runtime-class arm below).
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Date) && self.is_erased_class_type(target) =>
            {
                Ok(text.to_owned())
            }
            // Extracting an erased value into the unit type means the source
            // language discards it (a `=> void` callback ignores whatever it
            // returns; JS does not assert the value is null). Drop it instead of
            // panicking — the old assert turned e.g. `tap(identity)` and a
            // `vi.fn<(x) => void>` transformer into runtime panics.
            Some(Type::None) => Ok(RustExpr::atom(format!("{{ let _ = {smelt_owned_text}; () }}"))),
            // A bigint is extracted the way its scalar siblings below are: an
            // exhaustive match over the tags, keeping the `BigInt` tag's exact
            // value and coercing the primitives a dynamic value can hold
            // exactly as `SmeltFromUnknown for BigInt` does.
            Some(Type::BigInt) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::BigInt(value) => value, SmeltUnknown::Number(value) => smelt_bigint_from_number(value).unwrap_or_default(), SmeltUnknown::String(value) => smelt_bigint_from_string(&value).unwrap_or_default(), SmeltUnknown::Bool(value) => {big}::from(i64::from(value)), SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => {big}::default() }}",
                big = crate::bigint_prelude::BIGINT_TYPE
            )))),
            Some(Type::Bool) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), {smelt_bigint_arm}SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value),")
            )))),
            Some(Type::Float) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {smelt_owned_text} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => smelt_string_to_number(&value), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
            )))),
            Some(Type::Int) => self.extract_int(&smelt_owned_text),
            // Reading an erased value out into a `String` slot is not the JS
            // `String(x)` CONVERSION -- nothing in the source asked for one; the
            // slot's declared type simply says `string`. So absence must not
            // become the conversion's answer `"undefined"`: the slot cannot
            // represent absence at all, and inventing a five-letter string for
            // it makes the erased side disagree with the very same absence
            // written as a literal, which the typed side renders as the type's
            // default (`String::new()`). `Null` has always answered
            // `String::new()` here rather than `"null"` for exactly that
            // reason; `Undefined` was the arm that had drifted, and a
            // `[3, undefined]` expectation therefore compared unequal to the
            // `[3, undefined]` a callee produced. The real conversion lives in
            // `strings.rs` (`String(x)`, template literals, a `replace`
            // replacer's return) and keeps `"undefined"`.
            //
            // The honest representation of an absent `string` is
            // `Optional(String)`; where the declared type refuses it (here a
            // library overload asserting `[number, string][]` for a `zip` that
            // pads with `undefined`) the type's default is the closest the slot
            // can come -- and it is at least the SAME answer on both sides of a
            // comparison.
            Some(Type::String) => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {smelt_owned_text} {{ {smelt_bigint_arm}SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),")
            )))),
            // Iterable-to-list extraction inspects the source through the
            // `SmeltUnknown` variant space (array/string/`Symbol.iterator`). The
            // source expression is usually erased already, but a statically typed
            // `SmeltList<_>` or `Option<SmeltList<_>>` (e.g. `Array.from(arr)`
            // where `arr: ArrayLike<T> | null | undefined`) reaches here too, so
            // normalize through the `IntoSmeltUnknown` boundary adapter first
            // rather than matching `SmeltUnknown::` arms against a non-erased
            // value. The adapter is identity on an existing `SmeltUnknown` and
            // preserves the backing array id, so already-erased callers are
            // unaffected.
            Some(Type::List(_)) if self.list_items_render_as_unknown(target, scope) => {
                Ok(erased_to_list_expr(text, None, &self.carrier_expr(&RustExpr::atom("SmeltUnknown::String(ch.to_string().into())"))))
            }
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::String) => {
                Ok(erased_to_list_expr(
                    text,
                    Some(
                        "if let SmeltUnknown::String(value) = value { value.to_string() } else { value.to_string() }",
                    ),
                    &RustExpr::chain("ch.to_string()"),
                ))
            }
            Some(Type::List(item)) => {
                let item_text = self.extract_rendered(&RustExpr::path("value"), *item, scope)?;
                let char_text =
                    self.extract_rendered(&self.carrier_expr(&RustExpr::atom("SmeltUnknown::String(ch.to_string().into())")), *item, scope)?;
                Ok(erased_to_list_expr(text, Some(&item_text), &char_text))
            }
            Some(Type::Dict(key, item))
                if self.mir.types.get(*key) == Some(&Type::String)
                    && self.mir.types.get(*item) == Some(&Type::Unknown) =>
            {
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(value) => value.smelt_shared_record(), SmeltUnknown::Array(values) => values.own_entries().into_iter().collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| (index.to_string(), SmeltUnknown::String(ch.to_string().into()))).collect(), SmeltUnknown::Function(value) => SmeltRecord::from([(\"{SMELT_CALL}\".to_owned(), SmeltUnknown::Function(value))]), _ => SmeltRecord::new() }}"
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let item_text = self.extract_rendered(&RustExpr::path("value"), *item, scope)?;
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match ({text}).into_smelt_unknown() {{ SmeltUnknown::Object(values) => SmeltRecord::with_id_from_entries(values.id, values.into_iter().map(|(key, value)| (key, {item_text}))), SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), {item_text})).collect(), SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| {{ let value = SmeltUnknown::String(ch.to_string().into()); (index.to_string(), {item_text}) }}).collect(), _ => SmeltRecord::new() }}"
                ))))
            }
            Some(Type::Dict(key, item)) if self.mir.types.get(*key) != Some(&Type::String) => {
                let key_text = self.rendered_at_type(&RustExpr::path("key"), self.type_id(Type::String)?, *key, scope)?;
                let item_text = self.extract_rendered(&RustExpr::path("value"), *item, scope)?;
                if self.dict_uses_js_key_map(*key) {
                    return Ok(RustExpr::loose(self.carrier_format(format_args!(
                        "if let SmeltUnknown::Object(values) = {smelt_owned_text} {{ SmeltJsMap::from_iter(values.into_iter().map(|(key, value)| ({key_text}, {item_text}))) }} else {{ SmeltJsMap::new() }}"
                    ))));
                }
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "if let SmeltUnknown::Object(values) = {smelt_owned_text} {{ values.into_iter().map(|(key, value)| ({key_text}, {item_text})).collect::<::indexmap::IndexMap<_, _>>() }} else {{ ::indexmap::IndexMap::new() }}"
                ))))
            }
            // A source `Map` recovers through `SmeltJsMap`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_map` marker: an erased Map
            // rebuilds its entries (and stable id) from the marker payload, and a
            // plain object falls back to string-keyed entries. This is the
            // inverse of the `SmeltJsMap` erasure adapter, so `unknown -> Map`
            // extraction stays lossless.
            Some(Type::JsMap(_, _)) => {
                let target_text = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))))
            }
            Some(Type::TypeParam { name }) if scope.spells(*name) => {
                let param_name = RustIdent::new(self.symbol_name(*name)?).into_string();
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "<{param_name} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))))
            }
            Some(Type::TypeParam { .. }) => Ok(RustExpr::atom(self.carrier_format(format_args!("({text}).into_smelt_unknown()")))),
            // A concrete union stores a tagged `SmeltUnion…` enum, so an erased
            // `SmeltUnknown` value extracted into that destination must be
            // reconstructed into the matching variant rather than passed through
            // as `SmeltUnknown`. This mirrors the `from_smelt_unknown` boundary
            // `inject_union_value` applies for `Unknown`/`TypeParam` sources;
            // here the source is a wider (non-concrete) union still stored erased.
            Some(Type::Union(_)) if self.concrete_union_members(target).is_some() => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "{}::from_smelt_unknown(({text}).into_smelt_unknown())",
                union::union_name(target)
            )))),
            Some(Type::Never | Type::Union(_)) => Ok(text.to_owned()),
            Some(Type::Optional(inner)) => self.extract_optional(text, *inner, scope),
            Some(Type::Tuple(items)) => self.extract_tuple(&smelt_owned_text, items, scope),
            Some(Type::Class { name, .. }) if self.symbol_name(*name)? == "PropertyKey" => {
                Ok(text.to_owned())
            }
            // The prelude `SmeltFromUnknown` impl is the single inverse of the
            // `SmeltRegExp` erasure adapter, so `source`, `flags` AND the
            // writable `lastIndex` all survive a round trip through erased
            // dataflow (`clone(re)`, `structuredClone`, an `unknown` bag).
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(RustExpr::loose(self.carrier_format(format_args!(
                "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})"
            )))),
            // Both synthetic match-result classes are backed by the concrete
            // `SmeltMatch` Rust type, whose prelude `SmeltFromUnknown` impl is
            // the inverse of its erasure adapter. Without this arm the generic
            // class fallback below answers `Default::default()`, silently
            // turning a round-tripped match into an EMPTY match.
            Some(Type::Class { name, .. }) if self.is_match_class_symbol(*name)? => Ok(RustExpr::loose(self.carrier_format(format_args!(
                "<SmeltMatch as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
            )))),
            // The inverse of the fetch-type erasure. Without this arm the generic
            // class fallback answers `Default::default()`, so a header list or a
            // parameter list that round-tripped through erased dataflow came back
            // EMPTY with no diagnostic.
            Some(Type::Class { .. }) if self.is_fetch_runtime_class_type(target)? => {
                let rust_type = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "<{rust_type} as SmeltFromUnknown>::smelt_from_unknown({smelt_owned_text})"
                ))))
            }
            Some(Type::Class { .. })
                if self.renders_erased(target)? =>
            {
                Ok(text.to_owned())
            }
            Some(Type::Class { .. }) if self.is_erased_class_type(target) => Ok(text.to_owned()),
            Some(Type::Class { .. }) if self.can_extract_unknown_object_record(target) => self.extract_object_record(text, target, scope),
            // A source `Set` recovers through `SmeltJsSet`'s `SmeltFromUnknown`
            // impl, which round-trips the `__smelt_set` marker (rebuilding members
            // and the stable id) and tolerantly accepts a bare array. This is the
            // inverse of the erasure adapter, so an `unknown -> Set` cast (e.g.
            // `data as unknown as Set` in a deep-equality walk) recovers the real
            // members instead of an empty set. `SmeltPrimSet` has the same impl.
            Some(Type::Set(_)) => {
                let target_text = self.rendered_type(target)?;
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "<{target_text} as SmeltFromUnknown>::smelt_from_unknown(({text}).into_smelt_unknown())"
                ))))
            }
            Some(Type::Class { .. }) => {
                Ok(RustExpr::atom("Default::default()"))
            }
            Some(Type::Function(function)) => self.extract_function(target, function, scope, &smelt_owned_text),
            // An already-erased `SmeltUnknown` at a `Type::Future` position is a
            // `SmeltUnknown::Promise` (e.g. `await`ing the result of an erased
            // vitest-mock call, or an `x as Promise<T>` cast). It CAN be recovered:
            // wrap a fresh promise-value handle whose body awaits the erased
            // promise through `smelt_await_flatten` and extracts its settled value
            // to the declared `Output`. Discarding it for `SmeltFuture::resolved`
            // of a default would drop the real resolved value and leave the shared
            // settle state unobserved (breaking, e.g., mock result matchers and
            // chained awaits on the recovered promise).
            Some(Type::Future(output)) => {
                let extracted = self.extract_rendered(&RustExpr::path("smelt_awaited"), *output, scope)?;
                // The erased source is read OUTSIDE the `async move` block. The
                // block is `'static`, so a `{text}` that borrows — a callback
                // parameter passed by shared reference
                // (`callback_param_is_shared_reference`) is the case that reaches
                // here — cannot be named inside it (E0521). Reading it eagerly is
                // also what the source does: the promise value already exists at
                // this point; only the `await` is deferred.
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_erased_future = ({text}).into_smelt_unknown(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_awaited = smelt_await_flatten(smelt_erased_future).await?; Ok::<_, {ERROR_BOX}>({extracted}) }})) }}"
                ))))
            }
            other => Err(EmitError::new(format!(
                "checked extraction from unknown expression `{text}` to {other:?} is not implemented yet"
            ))),
        }
    }

    /// Render the erased argument vector used when a `SmeltUnknown::Function`
    /// is called through a concrete function type.
    ///
    /// Explicit rest metadata controls whether a packed list parameter is spread.
    fn unknown_function_call_args_expr(
        &self,
        function: &FunctionType,
    ) -> Result<RustExpr, EmitError> {
        let mut statements = Vec::new();
        for (index, param_ty) in function.params.iter().enumerate() {
            if function.rest == Some(index)
                && let Some(Type::List(item_ty)) = self.mir.types.get(*param_ty)
            {
                let item_text = if matches!(
                    self.mir.types.get(*item_ty),
                    Some(Type::Unknown | Type::Never | Type::None | Type::TypeParam { .. })
                ) {
                    "value".to_owned()
                } else {
                    self.erase_rendered(&RustExpr::path("value"), *item_ty)?.into_string()
                };
                statements.push(format!(
                    "smelt_call_args.extend(arg{index}.clone().into_iter().map(|value| {item_text}));"
                ));
            } else {
                // `arg{index}` is a binding in the adapter closure, and the
                // erased vector owns its elements. A `&mut T` parameter binds a
                // reference, and so does a by-shared-reference parameter
                // (`callback_param_is_shared_reference`) — both have to be
                // cloned out before they can be pushed, or the vector infers
                // `Vec<&SmeltUnknown>` and every later push mismatches (E0308).
                let arg_text = if function.mutable_params.contains(&index)
                    || self.callback_param_is_shared_reference(function, index, *param_ty)
                {
                    RustExpr::chain(format!("arg{index}.clone()")).ending_in_clone()
                } else {
                    RustExpr::path(format!("arg{index}"))
                };
                // A list parameter whose elements are already `SmeltUnknown`
                // here erases by ALIASING the array, not by rebuilding it. In
                // JavaScript, passing an array where `unknown` is expected hands
                // over the same object: the erased value the callee sees must be
                // the array the caller holds, so a write through either is
                // visible through the other. `From<SmeltList<SmeltUnknown>> for
                // SmeltArray` is exactly that re-wrap — it carries both halves
                // of the reference (the `id` and the shared buffer) — whereas
                // the generic `erase_rendered` path walks the elements and
                // builds a detached copy.
                //
                // `erase_rendered` cannot reach that conclusion on its own:
                // it is handed the raw MIR type, sees a `Type::TypeParam` it
                // cannot prove is `Unknown`, and misses its own
                // `List<Unknown>` fast path. The predicate below is sound only
                // because these `arg{index}` bindings were declared under
                // `TypeSubstitution::erased()` (see `callback_arg_decls` at the
                // caller), which renders every type parameter as
                // `SmeltUnknown`.
                let item_text = if self.erased_param_is_unknown_list(*param_ty) {
                    self.carrier_format(format_args!("SmeltUnknown::Array(({arg_text}).into())"))
                } else {
                    self.erase_rendered(&arg_text, *param_ty)?.into_string()
                };
                statements.push(format!("smelt_call_args.push({item_text});"));
            }
        }
        Ok(RustExpr::atom(format!(
            "{{ let mut smelt_call_args = Vec::new(); {} smelt_call_args }}",
            statements.join(" ")
        )))
    }

    /// Whether `list_ty` is a list whose elements render as `SmeltUnknown` at
    /// `scope`.
    ///
    /// `Type::Unknown` is that outright. A `Type::TypeParam` is too whenever
    /// `scope` does not spell it: `rust_type` resolves such a name to
    /// [`crate::type_substitution::Resolved::Erased`], so `List<T>` is emitted
    /// as `SmeltList<SmeltUnknown>` and the per-element conversion the general
    /// path would run is `into_smelt_unknown()` on a value that already IS a
    /// `SmeltUnknown` — the identity impl. An *in-scope* `T` is a real Rust
    /// generic and must keep going through the general path, which converts each
    /// element through the corresponding inbound or outbound boundary adapter.
    ///
    /// Scope-aware rather than unconditional, and deliberately so: pass the
    /// scope the asked-about type was (or will be) rendered at. A value of the
    /// item being emitted was rendered at `&self.render_scope()`; a coercion
    /// target is rendered at the scope its render position threads, which a
    /// callee argument narrows (`callee_parameter_render_scope`).
    pub(in crate::emitter) fn list_items_render_as_unknown(
        &self,
        list_ty: TypeId,
        scope: &RenderScope,
    ) -> bool {
        let Some(Type::List(item)) = self.mir.types.get(list_ty) else {
            return false;
        };
        match self.mir.types.get(*item) {
            Some(Type::Unknown) => true,
            Some(Type::TypeParam { name }) => !scope.spells(*name),
            _ => false,
        }
    }

    /// Whether `param_ty` is a list whose elements are `SmeltUnknown` once the
    /// erased substitution is applied.
    ///
    /// `Type::Unknown` is that outright; a `Type::TypeParam` is too, because
    /// `TypeSubstitution::erased()` renders every type parameter as
    /// `SmeltUnknown` regardless of what the enclosing item declares. Only use
    /// this where the surrounding declaration was itself rendered under that
    /// substitution — elsewhere a type parameter may stand for a concrete Rust
    /// generic, and treating it as `Unknown` would be wrong. The scope-aware
    /// question is [`Self::list_items_render_as_unknown`].
    fn erased_param_is_unknown_list(&self, param_ty: TypeId) -> bool {
        let Some(Type::List(item)) = self.mir.types.get(param_ty) else {
            return false;
        };
        matches!(
            self.mir.types.get(*item),
            Some(Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Return whether `Option<T>` stores erased values that can carry explicit
    /// JavaScript singleton tags.
    ///
    /// This is only true when the `Option`'s payload is still stored as an erased
    /// `SmeltUnknown` (a bare `unknown`, an unscoped type parameter, or an erased
    /// host union), because such a payload can itself hold a `SmeltUnknown::Null`
    /// / `SmeltUnknown::Undefined` singleton without collapsing the `Option`. A
    /// *concrete* union inner (`Optional(string | RegExp)` lowered to
    /// `Option<SmeltUnion…>`) does **not** qualify: the tagged enum has no arm for
    /// `undefined`, so wrapping an erased `undefined` in `Some(from_smelt_unknown(…))`
    /// would fabricate a bogus member and lose the `undefined`. Those must fall
    /// through to the nullish-guarded extraction so `null`/`undefined` become
    /// `None` (fixes remeda `truncate`'s `separator?: string | RegExp` default).
    /// Convert an `Option<source>` to an `Option<target>` inner value by inner
    /// value.
    ///
    /// A present ERASED value can itself be `null` or `undefined` (an element
    /// read off an `unknown[]` holding `null`), and in JavaScript that value
    /// is the absent case of a `T | null | undefined` slot. Mapping it through
    /// the inner conversion instead made `null` the empty string and an
    /// `undefined` record an empty record. So an erased source collapses its
    /// nullish values to `None`, unless the target's inner type keeps the
    /// erased singletons itself.
    pub(in crate::emitter) fn optional_value_map_expr(
        &self,
        value_text: &RustExpr,
        source_inner: TypeId,
        target_inner: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        let mapped_value = self.rendered_at_type(&RustExpr::path("value"), source_inner, target_inner, scope)?;
        if self.inner_holds_erased_nullish(source_inner)
            && !self.optional_inner_preserves_erased_singletons(target_inner)
        {
            return Ok(RustExpr::postfix_on(&value_text, self.carrier_format(format_args!(
                "{value_text}.and_then(|value| match value {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some({mapped_value}) }})"
            ))));
        }
        Ok(RustExpr::postfix_on(&value_text, format!("{value_text}.map(|value| {mapped_value})")))
    }

    /// Whether a value of `ty` is stored erased (`SmeltUnknown`), and so can be
    /// a run-time `null` or `undefined`: `unknown`, an erased type parameter,
    /// or a union kept on the dynamic carrier. A concrete `SmeltUnion…` enum
    /// has no such arm.
    fn inner_holds_erased_nullish(&self, ty: TypeId) -> bool {
        self.concrete_union_members(ty).is_none() && self.has_dynamic_shape(ty)
    }

    pub(in crate::emitter) fn optional_inner_preserves_erased_singletons(&self, inner: TypeId) -> bool {
        if self.concrete_union_members(inner).is_some() {
            return false;
        }
        self.has_dynamic_shape(inner)
    }

    /// An asserted cast of an erased value to `target`, then to the destination.
    pub(in crate::emitter) fn unknown_cast_expr(
        &self,
        unknown_value: &Operand,
        target: TypeId,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let source = self.operand_ty(unknown_value)?;
        let value_text = self.operand_expr(unknown_value)?;
        if let Some(collected) =
            self.generator_items_expr(&value_text, source, target)?
        {
            return self.rendered_at_type(&collected, target, dest_ty, &self.render_scope());
        }
        if let Some(projected) =
            self.project_union_value(&value_text, source, target)?
        {
            return self.rendered_at_type(&projected, target, dest_ty, &self.render_scope());
        }
        let cast_text = if self.renders_erased(target)? {
            self.erase(unknown_value)?
        } else {
            self.extract(unknown_value, target, &self.render_scope())?
        };
        self.rendered_at_type(&cast_text, target, dest_ty, &self.render_scope())
    }
}

/// Whether an expression is cheap and side-effect-free to evaluate more than
/// once. Bare locals and dotted field reads (`x`, `foo.bar.baz`) qualify; any
/// expression containing a call (`(`) may run side effects or move its inputs
/// and must be bound to a temporary before being read twice.
fn is_trivial_reeval_expr(text: &RustExpr) -> bool {
    !text.contains('(')
}

/// Emits the ONE arm set that rebuilds a typed `SmeltList` from an erased value.
///
/// Every element type funnels through here, so the set of sources a lowering
/// accepts does not depend on which element type it happens to carry. It used to:
/// the `SmeltUnknown`-element and `String`-element forms iterated a source string
/// into characters while the general form panicked on one, so the same JavaScript
/// value converted fine in `groupBy`'s adapter and blew up in `map`'s. The arms
/// mirror what JavaScript's iteration protocol accepts: nullish (an empty list by
/// design, not as `Array.from` does), an array, a string (by character), a host byte
/// buffer, an `arguments` object, a `Map`/`Set`, and finally any object exposing
/// `Symbol.iterator`.
///
/// `item_text` converts one erased element bound to `value`; `None` means the
/// element type IS `SmeltUnknown`, so elements pass through unconverted.
/// `char_text` converts one `char` bound to `ch` for the string arm.
///
/// # Why the array arm aliases when `item_text` is `None`
///
/// A JavaScript array is a reference value, and `SmeltList<SmeltUnknown>` and
/// `SmeltArray` model that identically: an `id` plus a *shared*
/// `Rc<RefCell<Vec<SmeltUnknown>>>`. When no element conversion is needed the
/// two representations are byte-identical, so extracting an erased array into a
/// typed list is a re-wrap of the SAME array, not the construction of a new one.
/// Rebuilding the element vector (`values.into_vec()` into a fresh `Rc`) kept
/// the `id` but detached the storage, producing a half reference: a stale
/// snapshot wearing the live array's identity. A write through the typed handle
/// — `arr.push(x)` inside a callback that received the array being iterated —
/// then went to a dead copy and was invisible through the erased value, even
/// though JavaScript says both names denote one object.
///
/// It is also where the time went. Every crossing of an erased boundary paid an
/// O(n) memcpy plus n `SmeltUnknown` clones and n drops; a library that routes
/// each call through a runtime dispatcher (so the boundary is crossed once per
/// callback invocation) turned an O(n) operation into O(n^2).
///
/// When `item_text` is `Some(..)` the elements genuinely have to be converted,
/// so that arm still rebuilds — but it keeps `values.id`, exactly as before.
/// Every non-array arm builds a list that did not exist in the source program,
/// so it mints a fresh identity through `SmeltList::new`.
pub(in crate::emitter) fn erased_to_list_expr(text: &RustExpr, item_text: Option<&str>, char_text: &RustExpr) -> RustExpr {
        // `text` is usually already an owned temporary (an operand render clones the local
        // it reads), so take an owned copy rather than deep-copying it a second time.
        let smelt_owned_text = text.cloned_value();
    // Converts a `Vec<SmeltUnknown>`-producing expression into the element type,
    // then wraps it in a list with a FRESH JS reference identity. Only the array
    // arm has a source identity to carry over.
    let convert = |elements: &str| match item_text {
        None => format!("SmeltList::new({elements})"),
        Some(item) => format!(
            "SmeltList::new({elements}.into_iter().map(|value| {item}).collect::<Vec<_>>())"
        ),
    };
    let byte_buffer_elements = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENTS;
    let arguments_elements = smelt_stdlib::runtime_symbols::host::ARGUMENTS_ELEMENTS;
    let array_arm = match item_text {
        // The aliasing re-wrap: same identity, same buffer, no copy.
        None => "SmeltList::with_storage(values.id, values.storage())".to_owned(),
        Some(item) => format!(
            "{{ let smelt_list_id = values.id; let smelt_list = SmeltList::with_id(smelt_list_id, values.into_iter().map(|value| {item}).collect::<Vec<_>>()); smelt_array_props_adopt(smelt_list_id, &smelt_list.storage()); smelt_list }}"
        ),
    };
    let string_arm =
        format!("SmeltList::new(value.chars().map(|ch| {char_text}).collect::<Vec<_>>())");
    let bytes_arm = convert("smelt_bytes");
    let arguments_arm = convert("smelt_args");
    let map_arm = convert("pairs.into_vec()");
    let set_arm = convert("members.into_vec()");
    let iterator_arm =
        convert("smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null))");
    RustExpr::atom(format!(
        "{{ let smelt_src = {smelt_owned_text}.into_smelt_unknown(); \
         match smelt_src {{ \
         SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), \
         SmeltUnknown::Array(values) => {array_arm}, \
         SmeltUnknown::String(value) => {string_arm}, \
         SmeltUnknown::Object(value) => \
         if let Some(smelt_bytes) = {byte_buffer_elements}(&SmeltUnknown::Object(value.clone())) {{ {bytes_arm} }} \
         else if let Some(smelt_args) = {arguments_elements}(&value) {{ {arguments_arm} }} \
         else if let Some(SmeltUnknown::Array(pairs)) = value.get(\"{SMELT_MAP}\") {{ {map_arm} }} \
         else if let Some(SmeltUnknown::Array(members)) = value.get(\"{SMELT_SET}\") {{ {set_arm} }} \
         else {{ match value.get(\"{SMELT_SYMBOL_ITERATOR}\") {{ \
         Some(SmeltUnknown::Function(iterator)) => {iterator_arm}, \
         _ => panic!(\"unknown is not iterable\") }} }}, \
         _ => panic!(\"unknown is not iterable\") }} }}"
    ))
}

impl FunctionEmitter<'_> {
    /// Extraction at an optional type.
    fn extract_optional(
        &self,
        text: &RustExpr,
        inner: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        if self.optional_inner_preserves_erased_singletons(inner) {
            // The slot keeps an explicit `null` as a present
            // `Some(SmeltUnknown::Null)`, but `undefined` IS the absent
            // case: `None`, as every other producer of such a slot
            // spells it (a missing record key reads `None`). Wrapping
            // it in `Some` made an absent erased property read present
            // (`!!(value as { then?: Function })?.then` on a number).
            if is_trivial_reeval_expr(text) {
                let inner_text = self.extract_rendered(text, inner, scope)?;
                return Ok(RustExpr::loose(format!(
                    "if smelt_unknown_is_undefined(&{text}) {{ None }} else {{ Some({inner_text}) }}"
                )));
            }
            let inner_text =
                self.extract_rendered(&RustExpr::path("smelt_optional_source"), inner, scope)?;
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_optional_source = {text}; if smelt_unknown_is_undefined(&smelt_optional_source) {{ None }} else {{ Some({inner_text}) }} }}"
            )));
        }
        // The nullish guard and the `Some(...)` arm both read the source,
        // so a side-effecting `text` (e.g. an erased `.call(..)` that
        // consumes its argument list) would be evaluated twice and move
        // its inputs. Bind non-trivial sources to a single temporary so
        // the value is produced exactly once; a bare identifier is cheap
        // to re-read and keeps generated output stable.
        if is_trivial_reeval_expr(text) {
            let inner_text = self.extract_rendered(text, inner, scope)?;
            return Ok(RustExpr::loose(format!(
                "if smelt_unknown_is_nullish(&{text}) {{ None }} else {{ Some({inner_text}) }}"
            )));
        }
        let inner_text = self.extract_rendered(&RustExpr::path("smelt_optional_source"), inner, scope)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_optional_source = {text}; if smelt_unknown_is_nullish(&smelt_optional_source) {{ None }} else {{ Some({inner_text}) }} }}"
        )))
    }

    /// Extraction at a function type.
    fn extract_function(
        &self,
        target: TypeId,
        function: &FunctionType,
        scope: &RenderScope,
        smelt_owned_text: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        if self.is_erased_unknown_rest_function(function) && !function.may_throw {
            let length = function
                .required_params
                .unwrap_or_else(|| function.rest.unwrap_or(function.params.len()));
            // Reading a value that holds no function is legal; only calling
            // it throws, so the adapter answers a callable that does.
            let default_callback = "SmeltErasedFunction { callback: ::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| smelt_value_not_callable()), length: 0.0, object: None }";
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_value = {smelt_owned_text}; let smelt_function = match smelt_value.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"{SMELT_CALL}\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ SmeltErasedFunction {{ callback: ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| (smelt_function)(smelt_args).unwrap_or_else(|error| smelt_panic_throw(error))), length: {length}.0, object: match smelt_value {{ SmeltUnknown::Object(object) => Some(object), _ => None }} }} }} else {{ {default_callback} }} }}"
            ))));
        }
        // The callback slot and its return type render in `scope`, the
        // same environment the return value is extracted in below, so a
        // type parameter the scope spells (a generic union enum's own
        // parameter inside its `from_smelt_unknown`) stays spelled.
        let target_text = self.rust_type(target, false, &scope.substitution())?.into_string();
        let rendered_return = self.rust_type(function.return_ty, false, &scope.substitution())?;
        let return_ty = rendered_return.as_str();
        // The adapter closure is cast to `target_text`, so its
        // PARAMETER types render in the same `scope`: a parameter the
        // scope spells (`&T`) must be bound as `&T`, or the closure
        // does not coerce to the slot (E0631). Each argument is then
        // erased through `IntoSmeltUnknown`, which every spelled
        // parameter's bound provides.
        let params = self
            .callback_arg_decls(function, &scope.substitution(), MutablePrefix::Apply)?
            .join(", ");
        let args = self.unknown_function_call_args_expr(function)?;
        let call_text = if function.may_throw {
            format!("(smelt_function)({args})?")
        } else {
            format!(
                "(smelt_function)({args}).unwrap_or_else(|error| smelt_panic_throw(error))"
            )
        };
        let converted_return_text = if let Some(Type::Future(item)) =
            self.mir.types.get(function.return_ty)
        {
            drop(self.rendered_type(*item)?);
            // JavaScript `await` flattens promise chains: an erased
            // callable adapted to a promise-returning signature may
            // itself return an erased promise (e.g. a Vitest
            // `mockResolvedValue`/`mockRejectedValue` mock, or an erased
            // async function), and awaiting the adapter must resolve —
            // or reject as `Err` — through that inner promise instead of
            // stringifying/extracting the promise value itself.
            // `smelt_await_flatten` is an identity pass-through for every
            // non-promise value, so already-settled plain results are
            // unchanged; the flattened value is then extracted to the
            // declared promise item type as before.
            let converted_item = self.extract_rendered(&RustExpr::path("smelt_flattened"), *item, scope)?;
            self.carrier_format(format_args!(
                "SmeltFuture::from_future(Box::pin(async move {{ let smelt_flattened = smelt_await_flatten((smelt_result).into_smelt_unknown()).await?; Ok::<_, {ERROR_BOX}>({converted_item}) }}))"
            ))
        } else if rendered_return.shape() == RustShape::Erased {
            "smelt_result".to_owned()
        } else {
            self.extract_rendered(&RustExpr::path("smelt_result"), function.return_ty, scope)?.into_string()
        };
        let return_text = if function.may_throw {
            format!("Ok::<_, {ERROR_BOX}>({converted_return_text})")
        } else {
            converted_return_text
        };
        // Reading a value that holds no function is legal; only calling it
        // throws, so the adapter answers a callable that does.
        let default_callback = format!(
            "{{ let smelt_default_callback: {target_text} = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ smelt_value_not_callable() }}); smelt_default_callback }}"
        );
        // A callable OBJECT whose `__smelt_call` is the erasure of a
        // typed callable restores that callable, and the object is then
        // recorded against the restored callback so erasing it again
        // answers the object with its properties. Recorded against the
        // restored callable ITSELF, the entry would own a path back to
        // its own key (object -> `__smelt_call` adapter -> captured
        // origin) and could never be swept (`callable_registry`), so the
        // object is recorded against a fresh forwarding callable
        // instead: the same JavaScript function (it is linked to the
        // same identity below), and an allocation only the program holds.
        let forwarded_args = (0..function.params.len())
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "{{ let smelt_source_value = {smelt_owned_text}; let smelt_function = match smelt_source_value.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"{SMELT_CALL}\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: {target_text} = if let Some(smelt_original) = smelt_restore_function_origin::<{target_text}>(&smelt_function) {{ if matches!(smelt_source_value, SmeltUnknown::Object(_)) {{ ::std::rc::Rc::new(move |{params}| (smelt_original)({forwarded_args})) }} else {{ smelt_original }} }} else {{ ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ let smelt_result = {call_text}; {return_text} }}) }}; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback }} else {{ {default_callback} }} }}"
        ))))
    }

    /// Extraction at an integer.
    fn extract_int(&self, smelt_owned_text: &RustExpr) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "match {smelt_owned_text} {{ SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }}, SmeltUnknown::String(value) => smelt_string_to_number(&value) as i64, SmeltUnknown::Bool(value) => if value {{ 1_i64 }} else {{ 0_i64 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => 0_i64 }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value) as i64,")
        ))))
    }

    /// Extraction at a tuple.
    fn extract_tuple(
        &self,
        smelt_owned_text: &RustExpr,
        items: &[TypeId],
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        let items_text = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let value = RustExpr::chain(self.carrier_format(format_args!(
                    "smelt_tuple_values.get({index}).cloned().unwrap_or(SmeltUnknown::Null)"
                )));
                self.extract_rendered(&value, *item, scope)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let tuple_text = if items.len() == 1 {
            format!("({items_text},)")
        } else {
            format!("({items_text})")
        };
        Ok(RustExpr::loose(self.carrier_format(format_args!(
            "if let SmeltUnknown::Array(smelt_tuple_values) = {smelt_owned_text} {{ {tuple_text} }} else {{ panic!(\"unknown is not tuple\") }}"
        ))))
    }

    /// Extraction of a record from an erased object.
    fn extract_object_record(
        &self,
        text: &RustExpr,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // Converting to a record type while already converting to it:
        // a field reaches its own type (`swap(): Pair<T>`). Inlining
        // again would never end, so call the hoisted helper, which
        // `hoisted_record_conversion` reserves before rendering its
        // body. Answering `Default::default()` here dropped the inner
        // value (`p.swap()` read `0 0`).
        if self.record_conversion_stack.borrow().contains(&target) {
            return match self.hoisted_record_conversion(text, target)? {
                Some(call) => Ok(RustExpr::from(call)),
                None => self.generated_record_from_unknown_expr(text, target),
            };
        }
        if self.record_conversion_stack.borrow().is_empty()
            && let Some(call) = self.hoisted_record_conversion(text, target)?
        {
            return Ok(RustExpr::chain(call));
        }
        match self.inline_record_conversion(text, target, scope)? {
            Some(conversion) => Ok(RustExpr::from(conversion)),
            None => self.generated_record_from_unknown_expr(text, target),
        }
    }
}
