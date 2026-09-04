//! Place emission helpers.

use super::*;
use crate::emitter::rendered_text_rewrite::cloned_value_text;

impl FunctionEmitter<'_> {
    /// Render a dotted property name as the concrete key type of a dictionary.
    ///
    /// JavaScript permits dotted writes on records whose key domain widened to
    /// `unknown` (for example after adding a symbol-keyed property). Reads and
    /// writes share this conversion so `record.loop` addresses the same entry.
    pub(super) fn dict_field_key_text(
        &self,
        key: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        let field_name = self.symbol_source_name(field)?;
        if self.mir.types.get(key) == Some(&Type::String) {
            return Ok(format!("{field_name:?}.to_owned()"));
        }
        if matches!(
            self.mir.types.get(key),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(key)
        {
            return self.erase_value_text(
                &format!("{field_name:?}.to_owned()"),
                self.type_id(Type::String)?,
            );
        }
        self.default_value(key)
    }

    /// Converts a place to its Rust text representation.
    pub(super) fn place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_value_text(*local),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, value)) = self.mir.types.get(base_ty) {
                    let key_text = self.dict_field_key_text(*key, *field)?;
                    let base_text = self.local_value_text(*base)?;
                    if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                        if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                            return Ok(format!("{base_text}.get(&{key_text}).flatten()"));
                        }
                        return Ok(format!("{base_text}.get(&{key_text}).cloned().flatten()"));
                    }
                    if self.mir.types.get(*key) == Some(&Type::String)
                        && self.type_text_with_impl_trait(*value, false)? == "SmeltUnknown"
                    {
                        // A property the record does not hold reads as
                        // `undefined` in JavaScript, never `null`. The two are
                        // distinguishable under `===`, so answering `Null` here
                        // silently defeats every `value === undefined` guard the
                        // source writes over a dynamic record.
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).unwrap_or(SmeltUnknown::Undefined)"
                        ));
                    }
                    if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).expect(\"missing field\")"
                        ));
                    }
                    return Ok(format!(
                        "{base_text}.get(&{key_text}).cloned().expect(\"missing field\")"
                    ));
                }
                // A `.length` read on a CONCRETE collection (typed list or set)
                // projects to the backing `Vec`/`HashSet` length as an `f64`,
                // matching how list mutation helpers surface `.len() as f64`.
                // Without this arm the read falls through to the raw struct-field
                // fallback (`base.length`), which no collection type has. This
                // mirrors the erased-receiver `TsLength` arm below but stays on
                // the concrete representation instead of erasing.
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::List(_) | Type::Set(_))
                ) && smelt_stdlib::typescript_field_rule(self.symbol_source_name(*field)?)
                    == Some(smelt_stdlib::FieldRule::TsLength)
                {
                    return Ok(format!("({}.len() as f64)", self.local_value_text(*base)?));
                }
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(base_ty)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let field_rule = smelt_stdlib::typescript_field_rule(field_name);
                    let base_text = self.local_value_text(*base)?;
                    // A concrete-union receiver stores a tagged enum; project it
                    // back to `SmeltUnknown` before the erased-object field match.
                    let scrutinee =
                        self.erase_concrete_union_text(&cloned_value_text(&base_text), base_ty);
                    if field_rule == Some(smelt_stdlib::FieldRule::TsLength) {
                        // A callable's `.length` is its declared arity, which the
                        // erasure boundary parks in the function-length registry
                        // (an `Rc<dyn Fn>` has nowhere to carry it). Without this
                        // arm an erased callable answered `null`, and es-toolkit
                        // `rest(func)` — whose default split point is
                        // `func.length - 1` — reshaped every call wrongly.
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), value @ SmeltUnknown::Function(_) => SmeltUnknown::Number({function_length}(&value)), SmeltUnknown::Object(map) => match smelt_get_object_field(&map, \"length\") {{ SmeltUnknown::Undefined | SmeltUnknown::Null if map.contains_key(\"__smelt_call\") => SmeltUnknown::Number({function_length}(&SmeltUnknown::Object(map))), value => value }}, _ => SmeltUnknown::Null }}",
                            function_length =
                                smelt_stdlib::runtime_symbols::function_length::READ,
                        ));
                    }
                    if field_rule == Some(smelt_stdlib::FieldRule::TsSort) {
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::Array(value) => smelt_array_sort_method(value), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"sort\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    // `AbortController`/`AbortSignal` methods are surfaced as
                    // runtime-helper-bound closures that mutate the shared abort
                    // record (see `smelt_abort_method`). Plain data fields
                    // (`signal`, `aborted`, ...) keep the ordinary erased-object
                    // read; the helper only intercepts a method read when the
                    // receiver actually carries an abort marker.
                    if matches!(
                        field_name,
                        "abort"
                            | "addEventListener"
                            | "removeEventListener"
                            | "dispatchEvent"
                            | "throwIfAborted"
                    ) {
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::Object(map) if (map.contains_key(\"__smelt_abortcontroller\") || map.contains_key(\"__smelt_abortsignal\")) && !map.contains_key({field_name:?}) => smelt_abort_method(map, {field_name:?}), SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Undefined }}"
                        ));
                    }
                    // `Function.prototype.apply`/`call` must resolve when the
                    // erased receiver is a `SmeltUnknown::Function` (not just an
                    // object). The plain object field match below returns
                    // `Undefined` for a function receiver, collapsing every
                    // invocation to a null callback (see `partial`/`partialRight`).
                    // The runtime helper binds the callable with the correct
                    // this-dropping/argument-spreading semantics and falls back to
                    // the ordinary field read for object receivers.
                    if matches!(field_name, "apply" | "call") {
                        return Ok(format!(
                            "smelt_function_method({scrutinee}, {field_name:?})"
                        ));
                    }
                    // `Object.prototype.valueOf` exists on EVERY value, so it can
                    // never resolve through the own-field read: a boxed primitive
                    // (`Object(1)`, `new Number(1)`) is a marker object with no own
                    // `valueOf`, and an erased primitive is not an object at all.
                    // Both used to collapse to a null callback, which is what made
                    // `isEqualWith(1, Object(1), noop)` answer `false`. The runtime
                    // helper unwraps the wrapper and still prefers a user-defined
                    // own `valueOf` when the receiver has one.
                    if field_name == "valueOf" {
                        return Ok(format!("smelt_value_of_method({scrutinee})"));
                    }
                    // Every receiver shape through one prelude helper: an object
                    // record, an erased ARRAY (whose `length`, indices and named
                    // side-table properties are all readable), and the
                    // `Object.prototype` sentinel. Inlining an object-only `match`
                    // here answered `undefined` for `arr.x` and for
                    // `Object.prototype.toString`.
                    return Ok(format!(
                        "smelt_get_unknown_field(&{scrutinee}, {field_name:?})"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && (matches!(
                        self.mir.types.get(*inner),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                    ) || self.is_erased_class_type(*inner))
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    if smelt_stdlib::typescript_field_rule(field_name)
                        == Some(smelt_stdlib::FieldRule::TsLength)
                    {
                        return Ok(format!(
                            "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    return Ok(format!(
                        "smelt_get_unknown_field(&{base_text}.clone().unwrap_or(SmeltUnknown::Undefined), {field_name:?})"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(Type::Dict(key_ty, _)) = self.mir.types.get(*inner)
                    && self.mir.types.get(*key_ty) == Some(&Type::String)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    return Ok(format!(
                        "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.get({field_name:?}))"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && self.symbol_source_name(*field)? == "value"
                {
                    let wrapped = self.erase_value_text("value", *inner)?;
                    return Ok(format!(
                        "{}.clone().map_or(SmeltUnknown::Undefined, |value| {wrapped})",
                        self.local_value_text(*base)?
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_regexp_class_symbol(*name)?
                {
                    return self.regexp_field_text(&self.local_value_text(*base)?, *field);
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(kind) = self.match_class_kind(*name)?
                {
                    return self.match_field_text(&self.local_value_text(*base)?, kind, *field);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(fields) = self.structural_record_fields(*inner)
                    && let Some(field_ty) = fields
                        .iter()
                        .find(|candidate| candidate.name == *field)
                        .map(|candidate| candidate.ty)
                {
                    let base_text = self.local_value_text(*base)?;
                    let field_name = sanitize_ident(self.symbol_name(*field)?);
                    return if matches!(self.mir.types.get(field_ty), Some(Type::Optional(_))) {
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    } else {
                        Ok(format!(
                            "{base_text}.as_ref().map(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    };
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
                    return Ok(self.null_value_text());
                }
                if smelt_stdlib::typescript_field_rule(self.symbol_source_name(*field)?)
                    == Some(smelt_stdlib::FieldRule::TsConstructor)
                {
                    return Ok(self.null_value_text());
                }
                // `.apply` / `.call` / `.bind` on a callable-interface value are
                // Function.prototype methods on the *underlying callable*, not
                // named struct fields. The interface struct has no such field
                // (reading `receiver.apply` is an `E0609`), so read the synthetic
                // `__smelt_call` storage slot and erase that callable to a
                // `SmeltUnknown::Function`. The erased-call coercion the caller
                // wraps this read in extracts and invokes it; `.bind` routes the
                // same way and yields a closure over the erased callable. Erasing
                // only the `__smelt_call` field (rather than the whole struct)
                // keeps the callable live — a whole-struct `into_smelt_unknown`
                // erases every function field to `Null`.
                if matches!(self.symbol_source_name(*field)?, "apply" | "call" | "bind")
                    && let Some(call_ty) = self.callable_interface_call_field_ty(base_ty)
                {
                    let call_field = format!("{}.__smelt_call.clone()", self.local_value_text(*base)?);
                    return self.erase_value_text(&call_field, call_ty);
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::String)) {
                    return self.string_field_text(&self.local_value_text(*base)?, *field);
                }
                if let Some(getter) = self.descriptor_getter_text(*base, *field)? {
                    return Ok(getter);
                }
                if self.storage_field_is_function(base_ty, *field) {
                    return Ok(format!(
                        "{}.{}.clone()",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(method_text) = self.class_method_reference_text(
                        &self.local_value_text(*base)?,
                        *name,
                        *field,
                    )?
                {
                    return Ok(method_text);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                {
                    return Ok(self.null_value_text());
                }
                // A dotted read of an UNDECLARED member on an index-signature
                // class (`bag.name` where `name` is not a struct field) is a
                // keyed lookup into the runtime store (issue #84). Declared
                // fields keep their concrete struct access via the fallback
                // below; only names with no matching field route to the store.
                if let Some((_key_ty, value_ty)) = self.class_index_store_types(base_ty)
                    && !self.class_has_named_field(base_ty, *field)
                {
                    let base_text = self.local_value_text(*base)?;
                    let key = self.symbol_source_name(*field)?;
                    let store_text =
                        format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD);
                    let default_value = self.default_value(value_ty)?;
                    let string_key_ty = self.type_id(Type::String)?;
                    let getter = if self.dict_uses_smelt_record(string_key_ty)
                        || self.dict_uses_js_key_map(string_key_ty)
                    {
                        format!("{store_text}.get(&{key:?}.to_owned())")
                    } else {
                        format!("{store_text}.get(&{key:?}.to_owned()).cloned()")
                    };
                    return Ok(format!("{getter}.unwrap_or({default_value})"));
                }
                // A dotted read of a member a CALLABLE-OBJECT record does not
                // declare. JavaScript lets a function own arbitrary properties,
                // and TypeScript accepts such a read wherever the receiver's
                // static shape is widened (or the author suppressed the check —
                // es-toolkit's `flow` spec reads `curried.placeholder` off a
                // `CurriedFunction1`). The struct has no such Rust field, so the
                // plain fallback below emitted `receiver.placeholder`, an E0609
                // in the generated crate. The value's own properties live in the
                // erased callable's property bag, so read the source-spelled key
                // from there and answer `undefined` when it carries none —
                // exactly what JavaScript answers for an absent property.
                if !self.class_has_named_field(base_ty, *field)
                    && self.callable_interface_call_field_ty(base_ty).is_some()
                {
                    let base_text = self.local_value_text(*base)?;
                    let key = self.symbol_source_name(*field)?;
                    return Ok(format!("{base_text}.__smelt_call.smelt_property({key:?})"));
                }
                // A declared field of a reference class lives inside the shared
                // cell. Read it through a narrow `borrow()` and clone the value
                // out so the borrow guard is a short-lived temporary that ends
                // with the statement — never held across a re-entrant call.
                if self.is_reference_class_type(base_ty)
                    && self.class_has_named_field(base_ty, *field)
                {
                    return Ok(format!(
                        "{}.0.borrow().{}.clone()",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                Ok(format!(
                    "{}.{}",
                    self.local_value_text(*base)?,
                    sanitize_ident(self.symbol_name(*field)?)
                ))
            }
            Place::Index {
                base,
                index,
                negative,
            } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_match_class_symbol(*name)?
                {
                    return self.match_index_text(&self.local_value_text(*base)?, index);
                }
                match self.mir.types.get(base_ty) {
                    Some(Type::List(item_ty)) => {
                        // A list index READ only ever calls `.get(..).cloned()`
                        // and `.len()` — both take `&self`, so the receiver must
                        // borrow the backing `Vec` immutably. Using the mutable
                        // form here (`borrow_mut()`) is not just unnecessary: when
                        // `base` is a shared closure capture (`Rc<RefCell<Vec<_>>>`)
                        // the `.len()` inside the normalized-index argument expands
                        // to a SECOND `borrow_mut()` of the same cell, so the single
                        // `arr.get({ ... arr.len() ... })` expression holds two
                        // simultaneous mutable borrows and panics at runtime with
                        // "already borrowed". Two simultaneous shared `borrow()`s
                        // are allowed, matching the sibling string/optional-list
                        // read arms below. The same argument is why the list's own
                        // shared buffer is read through `list_read_text`: the
                        // `.len()` in the index argument takes a second borrow of
                        // that cell while this one is live.
                        let base_text = self.local_value_text(*base)?;
                        let index_text =
                            self.normalized_read_index_text(&format!("{base_text}.len()"), index, *negative)?;
                        let missing = self.element_missing_value_text(*item_ty)?;
                        let read_text = list_read_text(&base_text);
                        // `unwrap_or` takes its argument BY VALUE, so the
                        // out-of-range value is constructed on every read and then
                        // dropped unused on the overwhelmingly common in-range one.
                        // A JavaScript index read is in range almost always, and in
                        // a loop this is per element: `sumBy` built and dropped a
                        // `Default::default()` ten thousand times per call to
                        // answer a question that never came up. `unwrap_or_else`
                        // costs nothing when the element is present.
                        // `element_missing_value_text` is a pure value expression,
                        // so moving it into a closure cannot change what it means.
                        Ok(format!(
                            "{read_text}.get({index_text}).cloned().unwrap_or_else(|| {missing})"
                        ))
                    }
                    Some(Type::Optional(inner_ty))
                        if matches!(self.mir.types.get(*inner_ty), Some(Type::List(_))) =>
                    {
                        let Some(Type::List(item_ty)) = self.mir.types.get(*inner_ty) else {
                            return Ok(self.null_value_text());
                        };
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_read_index_text(
                            &format!("{base_text}.as_ref().map_or(0, SmeltList::len)"),
                            index,
                            *negative,
                        )?;
                        let access =
                            if matches!(self.mir.types.get(*item_ty), Some(Type::Optional(_))) {
                                "values.borrow().get({index_text}).cloned().flatten()"
                            } else {
                                "values.borrow().get({index_text}).cloned()"
                            };
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|values| {})",
                            access.replace("{index_text}", &index_text)
                        ))
                    }
                    Some(Type::Dict(key_ty, value_ty)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        let base_text = self.local_value_text(*base)?;
                        let default_value = self.default_value(*value_ty)?;
                        let value_is_unknownish = matches!(
                            self.mir.types.get(*value_ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        );
                        if (self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty))
                            && value_is_unknownish
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                            ))
                        } else if self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty)
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).unwrap_or({default_value})"
                            ))
                        } else if value_is_unknownish {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                            ))
                        } else {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or({default_value})"
                            ))
                        }
                    }
                    Some(Type::String) => {
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_read_index_text(
                            &format!("{base_text}.chars().count()"),
                            index,
                            *negative,
                        )?;
                        Ok(format!(
                            "{base_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                        ))
                    }
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.unknown_index_text(&self.local_value_text(*base)?, index)
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!("{}.{tuple_index}", self.local_value_text(*base)?))
                    }
                    // A class with an index signature backs keyed reads with a
                    // real store field (issue #84). A non-optional keyed read
                    // (declared value type has no `undefined`) reads the store by
                    // key, defaulting a missing key to the value type's default.
                    Some(Type::Class { .. })
                        if self.class_index_store_types(base_ty).is_some() =>
                    {
                        let (key_ty, value_ty) = self
                            .class_index_store_types(base_ty)
                            .ok_or_else(|| EmitError::new("class index store types missing"))?;
                        let base_text = self.local_value_text(*base)?;
                        let store_text =
                            format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD);
                        let optional_read =
                            self.dict_index_optional_read_text(&store_text, key_ty, index)?;
                        let default_value = self.default_value(value_ty)?;
                        Ok(format!("{optional_read}.unwrap_or({default_value})"))
                    }
                    // No modelled keyed storage on this receiver, so the key
                    // is not a property of it: JavaScript answers `undefined`
                    // for a missing property, which is a DIFFERENT value from
                    // `null` under `===` (see `absent_value_text`).
                    _ => Ok(self.absent_value_text()),
                }
            }
        }
    }

    /// Emits a descriptor getter invocation for one statically known class field.
    pub(super) fn descriptor_getter_text(
        &self,
        base: LocalId,
        field: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some((owner, descriptor)) = self.descriptor_for_field(base_ty, field) else {
            return Ok(None);
        };
        let Some(getter_id) = descriptor.getter else {
            return Err(EmitError::new(
                "materialized descriptor read has no source getter",
            ));
        };
        let getter = self
            .mir
            .functions
            .get(usize::try_from(getter_id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("descriptor getter function is missing"))?;
        let HirOrigin::ClassMethod {
            class: getter_class,
            method: method_symbol,
            ..
        } = getter.origin
        else {
            return Err(EmitError::new(
                "descriptor getter did not lower as a class method",
            ));
        };
        let method_name = sanitize_ident(self.symbol_name(method_symbol)?);
        let base_text = self.local_value_text(base)?;
        if getter_class == owner.name {
            return Ok(Some(format!("{base_text}.{method_name}()")));
        }
        let descriptor_value = self.descriptor_value_text(getter_class, descriptor)?;
        let arguments = getter
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, _)| {
                if index == 0 {
                    cloned_value_text(&base_text)
                } else {
                    "Default::default()".to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "{descriptor_value}.{method_name}({arguments})"
        )))
    }

    /// Emits a descriptor setter statement for one statically known class field.
    pub(super) fn descriptor_setter_statement(
        &self,
        base: LocalId,
        field: Symbol,
        value: &Rvalue,
    ) -> Result<Option<String>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some((owner, descriptor)) = self.descriptor_for_field(base_ty, field) else {
            return Ok(None);
        };
        let Some(setter_id) = descriptor.setter else {
            return Err(EmitError::new(
                "materialized descriptor write has no source setter",
            ));
        };
        let setter = self
            .mir
            .functions
            .get(usize::try_from(setter_id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("descriptor setter function is missing"))?;
        let HirOrigin::ClassMethod {
            class: setter_class,
            method: method_symbol,
            ..
        } = setter.origin
        else {
            return Err(EmitError::new(
                "descriptor setter did not lower as a class method",
            ));
        };
        let write_ty = descriptor
            .write_ty
            .ok_or_else(|| EmitError::new("read-only descriptor cannot be assigned"))?;
        let rendered = self.rvalue_text_for_dest(value, write_ty)?;
        let method_name = sanitize_ident(self.symbol_name(method_symbol)?);
        let base_text = self.local_mut_value_text(base)?;
        if setter_class == owner.name {
            return Ok(Some(format!("{base_text}.{method_name}({rendered});")));
        }
        let descriptor_value = self.descriptor_value_text(setter_class, descriptor)?;
        let mut arguments = setter
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, parameter)| {
                if index == 0 {
                    if self.parameter_needs_mutable_reference_in(setter, *parameter) {
                        format!("&mut {base_text}")
                    } else {
                        cloned_value_text(&base_text)
                    }
                } else {
                    "Default::default()".to_owned()
                }
            })
            .collect::<Vec<_>>();
        if let Some(last) = arguments.last_mut() {
            rendered.clone_into(last);
        }
        Ok(Some(format!(
            "{descriptor_value}.{method_name}({});",
            arguments.join(", ")
        )))
    }

    /// Finds a descriptor through the concrete class inheritance chain.
    pub(super) fn descriptor_for_field(
        &self,
        ty: TypeId,
        field: Symbol,
    ) -> Option<(&MirClass, &MirDescriptor)> {
        let Type::Class { name, .. } = self.mir.types.get(ty)? else {
            return None;
        };
        let class = self.mir.classes.iter().find(|class| class.name == *name)?;
        if let Some(descriptor) = class
            .descriptors
            .iter()
            .find(|descriptor| descriptor.name == field)
        {
            let instance_storage_shadows = !descriptor.data_descriptor
                && crate::classes::effective_class_fields(self.mir, class)
                    .iter()
                    .any(|candidate| candidate.name == field);
            if !instance_storage_shadows {
                return Some((class, descriptor));
            }
        }
        let base = class.base?;
        let base_ty = self
            .mir
            .types
            .all()
            .iter()
            .position(|candidate| {
                matches!(candidate, Type::Class { name: candidate_name, .. } if *candidate_name == base)
            })
            .and_then(|index| u32::try_from(index).ok())
            .map(TypeId)?;
        self.descriptor_for_field(base_ty, field)
    }

    /// Constructs concrete static descriptor state as a Rust value.
    fn descriptor_value_text(
        &self,
        class_symbol: Symbol,
        descriptor: &MirDescriptor,
    ) -> Result<String, EmitError> {
        let descriptor_class = self
            .mir
            .classes
            .iter()
            .find(|candidate| candidate.name == class_symbol)
            .ok_or_else(|| EmitError::new("descriptor class is not materialized"))?;
        let class_name = crate::classes::class_name_text(self.mir, descriptor_class)?;
        let fields = descriptor
            .value_fields
            .iter()
            .map(|field| {
                Ok(format!(
                    "{}: {}",
                    sanitize_ident(self.symbol_name(field.name)?),
                    descriptor_literal_text(&field.value)
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if fields.is_empty() {
            Ok(format!("{class_name}::default()"))
        } else {
            Ok(format!(
                "{class_name} {{ {}, ..Default::default() }}",
                fields.join(", ")
            ))
        }
    }

    /// Converts a place to its Rust text representation for assignment.
    pub(super) fn assignment_place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_mut_value_text(*local),
            Place::Index {
                base,
                index,
                negative,
            } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        // The indexed lvalue is written through `IndexMut`, which
                        // needs a mutable borrow, but the length used to normalize
                        // the index only needs `&self`. Reading the length through
                        // the mutable form put two `borrow_mut()` of the same
                        // shared-capture `RefCell` in one expression. The primary
                        // list-index write (`emit_assign_place_statement`) never
                        // reaches this arm — it binds the index to a temp first;
                        // this lvalue is only consumed directly by block-wrapped
                        // callers such as `collection_clear_text`.
                        let base_mut = self.local_mut_value_text(*base)?;
                        let base_read = self.local_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_read}.len()"), index, *negative)?;
                        Ok(format!("{base_mut}[{index_text}]"))
                    }
                    Some(Type::Dict(key_ty, _)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        Ok(format!(
                            "*{}.get_mut(&{key_text}).expect(\"index out of bounds\")",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!(
                            "{}.{tuple_index}",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists, dicts, and tuples",
                    )),
                }
            }
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                // A declared field of a reference class is written through a
                // narrow `borrow_mut()`. The statement's right-hand side has
                // already been reduced to an operand by MIR temping, so the
                // mutable borrow never spans a re-entrant call. Checked before
                // the structural-record path because a reference class is still
                // record-shaped structurally.
                if self.is_reference_class_type(base_ty)
                    && self.class_has_named_field(base_ty, *field)
                {
                    return Ok(format!(
                        "{}.0.borrow_mut().{}",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                if self.structural_record_fields(base_ty).is_some() {
                    return Ok(format!(
                        "{}.{}",
                        self.local_mut_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                self.place_text(place)
            }
        }
    }

    /// Resolves a statically known tuple index for Rust field access.
    pub(super) fn tuple_index(&self, index: &Operand, len: usize) -> Result<usize, EmitError> {
        let value = match index {
            Operand::Const(Constant::Int(value)) => usize::try_from(*value).ok(),
            _ => None,
        }
        .ok_or_else(|| EmitError::new("tuple index must be a non-negative constant integer"))?;
        if value >= len {
            return Err(EmitError::new("tuple index is out of bounds"));
        }
        Ok(value)
    }

    /// Gets the type of a place.
    /// Converts an element index into a Rust `usize` expression for a WRITE.
    ///
    /// Negative indexes are offset from the collection length. An index that is
    /// still negative after normalization cannot address a slot, so the write
    /// form keeps the panic: silently redirecting the store to some other slot
    /// would corrupt the collection.
    pub(super) fn normalized_index_text(
        &self,
        len_expr: &str,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<String, EmitError> {
        self.normalized_index_text_with_fallback(
            len_expr,
            index,
            negative,
            "usize::try_from(normalized).expect(\"negative index out of bounds\")",
        )
    }

    /// The value a list slot holds when JavaScript would answer `undefined`.
    ///
    /// This is the one "missing element" notion the emitter has, and two places
    /// must agree on it: an out-of-range element READ (`arr[99]`), and the holes
    /// `Array(n)` allocates at construction. If they disagreed, `Array(3)[0]`
    /// and `[][0]` would answer differently for the same element type.
    ///
    /// JS out-of-bounds element access is `undefined`, not `null`. A type
    /// parameter that is in scope for the current generic function is a real
    /// Rust generic, so its missing value is `Default::default()` (a `T`), not
    /// the erased `SmeltUnknown::Undefined` used for genuinely erased element
    /// types. A concrete generated union element is likewise a tagged
    /// `SmeltUnion…`, so its missing value must be a union value
    /// (`default_value` produces one) rather than an erased tag.
    pub(super) fn element_missing_value_text(&self, item_ty: TypeId) -> Result<String, EmitError> {
        let item_is_in_scope_type_param = matches!(
            self.mir.types.get(item_ty),
            Some(Type::TypeParam { name })
                if self.current_function_has_type_param(*name)
        );
        if item_is_in_scope_type_param {
            return self.default_value(item_ty);
        }
        if matches!(
            self.mir.types.get(item_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(item_ty).is_none()
        {
            return Ok("SmeltUnknown::Undefined".to_owned());
        }
        self.default_value(item_ty)
    }

    /// Converts an element index into a Rust `usize` expression for a READ.
    ///
    /// Element reads are total in both source languages: an out-of-range index
    /// answers `undefined`/the missing value, never an error. A positive
    /// out-of-range index already reaches that behavior through `Vec::get`
    /// returning `None`, so a negative index that addresses no slot must reach
    /// it too. Converting the miss to `usize::MAX` (never a valid slot of a live
    /// `Vec`, whose capacity is bounded by `isize::MAX` bytes) makes the
    /// subsequent `get` miss instead of panicking, so both directions of
    /// out-of-range agree.
    ///
    /// Which negative indexes address a slot is `negative`'s question, not this
    /// one's; see [`Self::normalized_index_text_with_fallback`].
    pub(super) fn normalized_read_index_text(
        &self,
        len_expr: &str,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<String, EmitError> {
        self.normalized_index_text_with_fallback(
            len_expr,
            index,
            negative,
            "usize::try_from(normalized).unwrap_or(usize::MAX)",
        )
    }

    /// Shared body of the read/write index normalizers.
    ///
    /// `usize_conversion` is the trailing expression that turns the normalized
    /// `i64` (bound as `normalized`) into a `usize`; it is the only part that
    /// differs between a read (miss) and a write (panic).
    ///
    /// `negative` decides whether a negative subscript is normalized at all.
    /// Python counts back from the end, so `xs[-1]` becomes `len - 1` and names
    /// the last element. JavaScript does not: a negative subscript is a PROPERTY
    /// KEY, so `xs[-1]` addresses no element and reads `undefined` even when the
    /// array is non-empty. Wrapping unconditionally silently answered `xs[-1]`
    /// with `xs[len - 1]` in generated TypeScript — a wrong value, not a crash.
    /// Leaving the index alone lets the same out-of-range machinery below turn
    /// it into the miss both languages already agree on for `xs[len]`.
    fn normalized_index_text_with_fallback(
        &self,
        len_expr: &str,
        index: &Operand,
        negative: NegativeIndex,
        usize_conversion: &str,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        let normalize = match negative {
            NegativeIndex::FromEnd => {
                format!("let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }};")
            }
            // No `len` binding: nothing consults the length when a negative
            // index cannot reach a slot, and emitting it anyway would take a
            // second borrow of the receiver inside its own `get(..)` argument.
            NegativeIndex::OutOfRange => {
                format!("let normalized = {index_text} as i64;")
            }
        };
        Ok(format!("{{ {normalize} {usize_conversion} }}"))
    }

    /// A JS element read that flows into an optional slot keeps its fallibility.
    ///
    /// `arr[i]` has TypeScript type `T` (without `noUncheckedIndexedAccess`),
    /// so a source function such as `last<T>(arr: T[]): T | undefined` lowers to
    /// an infallible read coerced into `Option<T>` — which produced
    /// `Some(Default::default())` for an out-of-range index instead of `None`.
    /// When the coercion target is `Option<..>` the read itself is the natural
    /// `Option` producer, so emit `get(..).cloned()` and let the miss stay a
    /// miss. Returns `None` when the operand is not an element read this rule
    /// applies to, leaving the caller's ordinary coercion in place.
    pub(super) fn optional_element_read_text(
        &self,
        operand: &Operand,
        inner: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Index {
            base,
            index,
            negative,
        })
        | Operand::Move(Place::Index {
            base,
            index,
            negative,
        })) = operand
        else {
            return Ok(None);
        };
        let base_ty = self.local_decl(*base)?.ty;
        // A `Match` receiver has its own keyed-read lowering; leave it alone.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            return Ok(None);
        }
        match self.mir.types.get(base_ty).cloned() {
            Some(Type::List(item_ty)) => {
                let base_text = self.local_value_text(*base)?;
                let index_text =
                    self.normalized_read_index_text(&format!("{base_text}.len()"), index, *negative)?;
                // Read borrow, as in the total list index read above: the
                // `.len()` in the index argument borrows the same shared cell.
                let read = format!(
                    "{}.get({index_text}).cloned()",
                    list_read_text(&base_text)
                );
                if item_ty == inner {
                    return Ok(Some(read));
                }
                // `T[][i]` where the element is itself optional collapses the
                // two layers the same way JS does: a missing slot and a present
                // `undefined` are both `undefined`.
                if self.mir.types.get(item_ty) == Some(&Type::Optional(inner)) {
                    return Ok(Some(format!("{read}.flatten()")));
                }
                let Ok(mapped) = self.value_at_type_text("value", item_ty, inner) else {
                    return Ok(None);
                };
                Ok(Some(format!("{read}.map(|value| {mapped})")))
            }
            Some(Type::String) if self.mir.types.get(inner) == Some(&Type::String) => {
                let base_text = self.local_value_text(*base)?;
                let index_text = self
                    .normalized_read_index_text(&format!("{base_text}.chars().count()"), index, *negative)?;
                Ok(Some(format!(
                    "{base_text}.chars().nth({index_text}).map(|ch| ch.to_string())"
                )))
            }
            _ => Ok(None),
        }
    }

    /// A JS element read that flows into an ERASED slot keeps its fallibility.
    ///
    /// The erased twin of [`Self::optional_element_read_text`], and the two must
    /// agree: an out-of-range element read is `undefined` in JavaScript, so the
    /// optional target answers `None` and the erased target must answer
    /// `SmeltUnknown::Undefined`.
    ///
    /// Without this the read was made TOTAL first and erased afterwards, so the
    /// miss became the element type's own missing value
    /// ([`Self::element_missing_value_text`]) and erased as that value: for
    /// `b: string[]`, `row[i] = b[99]` into an erased `row` stored `''`, and for
    /// `number[]` it stored `0`, where JavaScript stores `undefined`. Emitting
    /// `get(..).cloned().map(erase).unwrap_or(SmeltUnknown::Undefined)` keeps the
    /// miss a miss and erases only the values that were really there.
    ///
    /// This adds no new erasure: the caller has already decided the destination
    /// is `SmeltUnknown`, and the rule only changes WHICH tag a miss produces.
    ///
    /// Returns `None` — leaving the caller's ordinary erase-after-read in place —
    /// when the operand is not an element read this rule applies to, or when the
    /// total read already erases its miss to `SmeltUnknown::Undefined` (an
    /// already-erased element type), so those emissions stay byte-identical.
    pub(super) fn erased_element_read_text(
        &self,
        operand: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Index {
            base,
            index,
            negative,
        })
        | Operand::Move(Place::Index {
            base,
            index,
            negative,
        })) = operand
        else {
            return Ok(None);
        };
        let base_ty = self.local_decl(*base)?.ty;
        // A `Match` receiver has its own keyed-read lowering; leave it alone.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            return Ok(None);
        }
        match self.mir.types.get(base_ty).cloned() {
            Some(Type::List(item_ty)) => {
                // The existing total read is already correct whenever erasing
                // its missing value yields `Undefined` (element types that are
                // themselves erased). Skipping those keeps their output stable.
                let missing = self.element_missing_value_text(item_ty)?;
                if self.erase_value_text(&missing, item_ty)? == "SmeltUnknown::Undefined" {
                    return Ok(None);
                }
                let base_text = self.local_value_text(*base)?;
                let index_text =
                    self.normalized_read_index_text(&format!("{base_text}.len()"), index, *negative)?;
                let erased_value = self.erase_value_text("value", item_ty)?;
                let read_text = list_read_text(&base_text);
                Ok(Some(format!(
                    "{read_text}.get({index_text}).cloned().map(|value| {erased_value}).unwrap_or(SmeltUnknown::Undefined)"
                )))
            }
            Some(Type::String) => {
                let base_text = self.local_value_text(*base)?;
                let index_text = self
                    .normalized_read_index_text(&format!("{base_text}.chars().count()"), index, *negative)?;
                Ok(Some(format!(
                    "{base_text}.chars().nth({index_text}).map(|ch| SmeltUnknown::String(ch.to_string().into())).unwrap_or(SmeltUnknown::Undefined)"
                )))
            }
            _ => Ok(None),
        }
    }

    /// Emit a runtime index read for values whose concrete shape is erased.
    ///
    /// TypeScript generic and unknown receivers may still be strings, arrays,
    /// or objects at runtime. Returning `Null` here hides lowering bugs and
    /// breaks later casts, so the generated Rust dispatches on `SmeltUnknown`
    /// and panics only when the runtime value is not indexable.
    ///
    /// An out-of-range ELEMENT read on a string or array runtime value answers
    /// `SmeltUnknown::Undefined`, not `Null` — the same rule the typed reads use
    /// ([`Self::element_missing_value_text`],
    /// [`Self::erased_element_read_text`]), and the reason `zipWith` on ragged
    /// inputs produced `"3null"` where JavaScript produces `"3undefined"`.
    /// Whether a missing OBJECT PROPERTY should likewise be `Undefined` is a
    /// separate question about property access and is deliberately left alone.
    ///
    /// The OBJECT arm reads through `smelt_get_object_field`, the same helper
    /// the erased STATIC field read uses. In JavaScript `o[k]` and `o.k` are one
    /// operation, so the two spellings have to answer identically; when this
    /// arm carried its own inlined `byte_buffer_element(..).unwrap_or_else(||
    /// values.get(..))` they diverged, and every marker-record rule the static
    /// read grew (`err.name` off `__smelt_error`, `x.constructor`, a `Map`'s
    /// `size` and iteration methods, the global object's builtin constructors)
    /// was invisible to the computed spelling.
    pub(super) fn unknown_index_text(
        &self,
        base_text: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        // `base_text` is usually already an owned temporary (an operand render
        // clones the local it reads), so take an owned copy rather than
        // deep-copying the erased base a second time on every index read.
        let base_text = &cloned_value_text(base_text);
        let index_ty = self.operand_ty(index)?;
        let index_text = self.operand_text(index)?;
        let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
        if matches!(self.mir.types.get(index_ty), Some(Type::String)) {
            return Ok(format!(
                r#"match {base_text} {{
                    SmeltUnknown::String(value) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number(value.chars().count() as f64)
                        }} else {{
                            smelt_key.parse::<usize>().ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string().into()))).unwrap_or(SmeltUnknown::Undefined)
                        }}
                    }}
                    SmeltUnknown::Array(values) => smelt_get_array_field(&values, &{key_text}),
                    SmeltUnknown::Object(values) => smelt_get_object_field(&values, &{key_text}),
                    _ => SmeltUnknown::Undefined,
                }}"#
            ));
        }
        let numeric_index_text = match self.mir.types.get(index_ty) {
            Some(Type::Int | Type::Float) => index_text,
            Some(Type::Bool) => format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}"),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if self.is_erased_class_type(index_ty)
                    || matches!(
                        self.mir.types.get(index_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) =>
            {
                let scrutinee =
                    self.erase_concrete_union_text(&cloned_value_text(&index_text), index_ty);
                format!(
                    "match {scrutinee} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
                )
            }
            _ => "f64::NAN".to_owned(),
        };

        Ok(format!(
            r"match {base_text} {{
                    SmeltUnknown::String(value) => {{
                        let len = value.chars().count() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string().into()))).unwrap_or(SmeltUnknown::Undefined)
                    }}
                    SmeltUnknown::Array(values) => {{
                        let len = values.len() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Undefined)
                    }}
                SmeltUnknown::Object(values) => smelt_get_object_field(&values, &{key_text}),
                _ => SmeltUnknown::Undefined,
            }}"
        ))
    }

    // Unknown/runtime type helpers continue in `unknown.rs`.
}

/// Converts a statically materialized descriptor literal to Rust source.
fn descriptor_literal_text(literal: &smelt_hir::Literal) -> String {
    match literal {
        smelt_hir::Literal::Bool(boolean) => boolean.to_string(),
        smelt_hir::Literal::Int(integer) => integer.to_string(),
        smelt_hir::Literal::Float(number) => {
            if number.is_nan() {
                "f64::NAN".to_owned()
            } else if number.is_infinite() && number.is_sign_positive() {
                "f64::INFINITY".to_owned()
            } else if number.is_infinite() {
                "f64::NEG_INFINITY".to_owned()
            } else {
                format!("{number:?}")
            }
        }
        smelt_hir::Literal::String(text) => format!("{text:?}.to_owned()"),
        smelt_hir::Literal::Symbol(_)
        | smelt_hir::Literal::Undefined
        | smelt_hir::Literal::None => "Default::default()".to_owned(),
    }
}
