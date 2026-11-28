// `serde` impls of the erased value: ECMA-262 `JSON.stringify` and `JSON.parse`.

// @smelt:item impl_serde_Serialize_for_SmeltUnknown when dep_serde_json
// @smelt:if unknown
//~ Emit natural JSON serde support for `SmeltUnknown`.
impl serde::Serialize for SmeltUnknown {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Undefined => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            //~ ECMA-262 `JSON.stringify` renders a number with the
            //~ JavaScript number-to-string algorithm, not Rust's float
            //~ formatting: an integral value has no fraction (`1`, not
            //~ `1.0`), `-0` is `0`, and a non-finite number is `null`
            //~ because JSON has no NaN or Infinity.
            Self::Number(value) => if !value.is_finite() { serializer.serialize_none() } else if *value == value.trunc() && value.abs() < 1e21 { serializer.serialize_i64(*value as i64) } else { serializer.serialize_f64(*value) },
// @smelt:end
// @smelt:if bigint
            //~ `JSON.stringify(1n)` throws: JSON has no bigint.
            Self::BigInt(_) => Err(serde::ser::Error::custom("Do not know how to serialize a BigInt")),
// @smelt:end
// @smelt:if unknown
            Self::String(value) => value.serialize(serializer),
            Self::Symbol(_) => serializer.serialize_none(),
            Self::Array(values) => serde::Serialize::serialize(&values.clone().into_vec(), serializer),
            //~ `JSON.stringify` serializes an object's OWN ENUMERABLE
            //~ properties, in order. That is the same rule `for...in`
            //~ uses, so both read one predicate
            //~ (`smelt_is_for_in_object_key`) rather than keeping two
            //~ lists of internal keys that can drift apart. Three
            //~ consequences, each matching Node:
            //~
            //~ * a HOST OBJECT serializes as `{}` — its state lives in
            //~   internal slots, not in properties, so the marker and
            //~   the slots must not appear (`JSON.stringify(new
            //~   Headers([["a","b"]]))` is `{}`);
            //~ * key order is INSERTION order, which the previous
            //~   `HashMap` collect destroyed;
            //~ * a property whose value is `undefined` is OMITTED, not
            //~   emitted as `null` (`{a: undefined, b: 1}` is `{"b":1}`).
            //~   Inside an ARRAY `undefined` still serializes as `null`,
            //~   which the `Self::Undefined` arm above already does.
            //~ A byte-backed host record serializes as its OWN
            //~ enumerable properties, which for an element-typed view
            //~ are its indices: `JSON.stringify(new Uint8Array([1,2]))`
            //~ is `{"0":1,"1":2}` in Node, not `{}`. Byte storage and a
            //~ `DataView` have no indexed properties of their own and
            //~ stay `{}`, which is the same answer the own-key filter
            //~ below already gives — so one helper
            //~ (`smelt_host_buffer_own_elements`) decides it for every
            //~ byte-backed shape, and `Object.keys`, `for...in` and this
            //~ serializer cannot drift apart.
            Self::Object(values) => { use serde::ser::SerializeMap as _; if let Some(elements) = smelt_host_buffer_own_elements(self) { let mut map = serializer.serialize_map(Some(elements.len()))?; for (index, element) in elements.iter().enumerate() { map.serialize_entry(&index.to_string(), element)?; } return map.end(); } let shape = smelt_for_in_object_shape(values); let store = values.store.borrow(); let included = |entry: &&SmeltFieldEntry<JsString, SmeltUnknown>| !matches!(entry.value, Self::Undefined | Self::Function(_) | Self::Symbol(_)) && smelt_is_for_in_shape_key(shape, &entry.key); let mut map = serializer.serialize_map(Some(store.entries().iter().filter(included).count()))?; for entry in store.entries().iter().filter(included) { map.serialize_entry(&entry.key, &entry.value)?; } map.end() },
            //~ A function or symbol VALUE is not JSON: inside an array
            //~ it is `null` and as a property it is omitted (the Object
            //~ arm's filter below drops it). Serializing the native-code
            //~ string, or a symbol's description, invented data that
            //~ JavaScript never writes.
            Self::Function(_) => serializer.serialize_none(),
            Self::Promise(_) => serializer.serialize_str("[object Promise]"),
        }
    }
}

// @smelt:end
// @smelt:item-end

// @smelt:item impl_serde_Deserialize_de_for_SmeltUnknown when dep_serde_json & unknown
impl<'de> serde::Deserialize<'de> for SmeltUnknown {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
        let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
        Ok(smelt_unknown_from_json_value(value))
    }
}

fn smelt_unknown_from_json_value(value: serde_json::Value) -> SmeltUnknown {
    match value {
        serde_json::Value::Null => SmeltUnknown::Null,
        serde_json::Value::Bool(value) => SmeltUnknown::Bool(value),
        serde_json::Value::Number(value) => SmeltUnknown::Number(value.as_f64().unwrap_or_default()),
        serde_json::Value::String(value) => SmeltUnknown::String(value.into()),
        serde_json::Value::Array(values) => SmeltUnknown::Array(values.into_iter().map(smelt_unknown_from_json_value).collect()),
        serde_json::Value::Object(values) => SmeltUnknown::Object(SmeltObject::new(values.into_iter().map(|(key, value)| (key, smelt_unknown_from_json_value(value))).collect())),
    }
}

// @smelt:item-end

// `JSON.stringify` of an erased value: the same ECMA-262 rules as the
// `Serialize` impl above, written straight to text so a string (value or key)
// is quoted over its UTF-16 code units. A lone surrogate is written as its
// lowercase `\uXXXX` escape, which serde's UTF-8 data model cannot carry.
// @smelt:item smelt_json_stringify when dep_serde_json & unknown
/// `JSON.stringify(value)`: the JSON text, or the `TypeError` message for a
/// value JSON cannot hold (a bigint).
fn smelt_json_stringify(value: &SmeltUnknown) -> Result<String, &'static str> {
    smelt_json_stringify_with(value, &SmeltUnknown::Undefined)
}

/// `JSON.stringify(value, replacer)`. The replacer is ECMA-262's: a function
/// called for every property with its holder as `this` (`key`, `value`) and
/// whose answer is serialized instead; an array of strings and numbers, the
/// only keys an object serializes, in that order; anything else is ignored.
#[allow(dead_code)]
fn smelt_json_stringify_with(value: &SmeltUnknown, replacer: &SmeltUnknown) -> Result<String, &'static str> {
    let replacer = SmeltJsonReplacer::of(replacer);
    let holder = SmeltUnknown::Object(SmeltObject::new(Vec::from([(String::new(), value.clone())])));
    let value = smelt_json_property(&replacer, &holder, "", value.clone());
    let mut out = String::new();
    smelt_json_write(&value, &replacer, &mut out)?;
    Ok(out)
}

/// The `replacer` argument of `JSON.stringify`, read once.
enum SmeltJsonReplacer {
    /// No replacer (or one that is neither a function nor an array).
    None,
    /// The property allowlist of a replacer array, deduplicated, in order.
    Keys(Vec<String>),
    /// A replacer function.
    Function(SmeltDynCallable),
}

impl SmeltJsonReplacer {
    /// Reads a source replacer: a function, an array of keys (each string
    /// or number, others skipped, as `JSON.stringify` does), or nothing.
    fn of(replacer: &SmeltUnknown) -> Self {
        match replacer {
            SmeltUnknown::Function(function) => Self::Function(function.clone()),
            SmeltUnknown::Array(keys) => {
                let mut list: Vec<String> = Vec::new();
                for key in keys {
                    let key = match key { SmeltUnknown::String(text) => text.to_string(), SmeltUnknown::Number(number) => smelt_number_to_string(number), _ => continue };
                    if !list.contains(&key) { list.push(key); }
                }
                Self::Keys(list)
            }
            _ => Self::None,
        }
    }
}

/// The value `SerializeJSONProperty` serializes for `value`, the property
/// `key` of `holder`: a `Date` is its ISO text (`Date.prototype.toJSON`), an
/// object with a `toJSON` method is that method's answer (called with the
/// object as `this` and `key`), and then a replacer function's answer (called
/// with `holder` as `this`).
fn smelt_json_property(replacer: &SmeltJsonReplacer, holder: &SmeltUnknown, key: &str, value: SmeltUnknown) -> SmeltUnknown {
    let mut value = value;
    if let SmeltUnknown::Object(object) = &value {
// @smelt:if date
        if let Some(SmeltUnknown::Number(time)) = object.get("__smelt_date") {
            value = if time.is_finite() { SmeltUnknown::String(value.to_iso_string().as_str().into()) } else { SmeltUnknown::Null };
        } else if let Some(SmeltUnknown::Function(to_json)) = object.get("toJSON").or_else(|| object.get("__smelt_proto:toJSON")).or_else(|| object.get("__smelt_method:toJSON")) {
// @smelt:else
//=         if let Some(SmeltUnknown::Function(to_json)) = object.get("toJSON").or_else(|| object.get("__smelt_proto:toJSON")).or_else(|| object.get("__smelt_method:toJSON")) {
// @smelt:end
// @smelt:if this_channel
            let _smelt_this_guard = smelt_push_this(SmeltUnknown::Object(object.clone()));
// @smelt:end
            value = to_json(Vec::from([SmeltUnknown::String(key.into())])).unwrap_or_else(|error| smelt_panic_throw(error));
        }
    }
    if let SmeltJsonReplacer::Function(function) = replacer {
// @smelt:if this_channel
        let _smelt_this_guard = smelt_push_this(holder.clone());
// @smelt:else
//=         let _ = holder;
// @smelt:end
        value = function(Vec::from([SmeltUnknown::String(key.into()), value])).unwrap_or_else(|error| smelt_panic_throw(error));
    }
    value
}

//~ Appends `QuoteJSONString(text)`: `"` and `\` escaped, the short escapes
//~ for `\b \f \n \r \t`, other control characters and every lone surrogate as
//~ a lowercase `\uXXXX`, anything else as itself.
fn smelt_json_quote(text: &JsStr, out: &mut String) {
    use ::std::fmt::Write as _;
    out.push('"');
    for decoded in text.chars_lossless() {
        match decoded {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok(ch) if u32::from(ch) < 0x20 => { let _ = write!(out, "\\u{:04x}", u32::from(ch)); }
            Ok(ch) => out.push(ch),
            Err(lone) => { let _ = write!(out, "\\u{lone:04x}"); }
        }
    }
    out.push('"');
}

/// Appends the JSON text of `value` (see `smelt_json_stringify`).
fn smelt_json_write(value: &SmeltUnknown, replacer: &SmeltJsonReplacer, out: &mut String) -> Result<(), &'static str> {
    match value {
        SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Function(_) => out.push_str("null"),
        SmeltUnknown::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        //~ A number is written by the `Serialize` impl, the one place its
        //~ rule (no fraction when integral, `null` when non-finite) lives.
        SmeltUnknown::Number(_) => out.push_str(&serde_json::to_string(value).map_err(|_unrepresentable| "JSON serialization failed")?),
// @smelt:if bigint
        SmeltUnknown::BigInt(_) => return Err("Do not know how to serialize a BigInt"),
// @smelt:end
        SmeltUnknown::String(text) => smelt_json_quote(text, out),
        SmeltUnknown::Promise(_) => out.push_str("\"[object Promise]\""),
        //~ Each element is a property of the array (its index the key); one
        //~ with no JSON text (`undefined`, a function, a symbol) is `null`.
        SmeltUnknown::Array(values) => {
            out.push('[');
            for (index, element) in values.iter().enumerate() {
                if index > 0 { out.push(','); }
                let element = smelt_json_property(replacer, value, &index.to_string(), element);
                smelt_json_write(&element, replacer, out)?;
            }
            out.push(']');
        }
        //~ Own enumerable properties in insertion order (or the replacer
        //~ array's keys, in its order), as in the `Serialize` impl: a
        //~ byte-backed view as its element indices, and a property whose value
        //~ is `undefined`, a function or a symbol omitted. A stored accessor
        //~ (an object literal's `get`) is read through its getter.
        SmeltUnknown::Object(values) => {
            out.push('{');
            if let Some(elements) = smelt_host_buffer_own_elements(value) {
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 { out.push(','); }
                    smelt_json_quote(&JsString::from(index.to_string()), out);
                    out.push(':');
                    smelt_json_write(element, replacer, out)?;
                }
                out.push('}');
                return Ok(());
            }
            let shape = smelt_for_in_object_shape(values);
            let entries: Vec<(JsString, SmeltUnknown)> = 'entries: {
                if let SmeltJsonReplacer::Keys(keys) = replacer {
                    break 'entries keys.iter().filter_map(|key| values.get(key.as_str()).map(|entry| (JsString::from(key.as_str()), entry))).collect();
                }
// @smelt:if live_views
                //~ A view of a typed record is written from the record's properties
                //~ as they are now. Serializing only reads them, so the view's
                //~ cached entries are not refreshed for it (`live_data`).
                if let Some(entries) = values.store.live_data() {
                    break 'entries entries.into_iter().filter(|(key, _)| smelt_is_for_in_shape_key(shape, key)).collect();
                }
                values.store.sync();
// @smelt:end
                let store = values.store.borrow();
                store.entries().iter().filter(|entry| smelt_is_for_in_shape_key(shape, &entry.key)).map(|entry| (entry.key.clone(), entry.value.clone())).collect()
            };
            let mut first = true;
            for (key, entry) in entries {
                let entry = smelt_resolve_accessor(entry, || value.clone());
                let entry = smelt_json_property(replacer, value, &key.to_string_lossy(), entry);
                if matches!(entry, SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Symbol(_)) { continue; }
                if !first { out.push(','); }
                first = false;
                smelt_json_quote(&key, out);
                out.push(':');
                smelt_json_write(&entry, replacer, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}
// @smelt:item-end

// @smelt:item smelt_json_stringify_value when json_stringify_value & dep_serde_json & unknown
/// `JSON.stringify(value)` where the value may have no JSON text: `undefined`,
/// a function or a symbol at the top level answers `undefined` (`None`), as
/// ECMA-262 `SerializeJSONProperty` does; inside an object or array such a
/// value is omitted or `null`, which `smelt_json_stringify` already writes.
#[allow(dead_code)]
fn smelt_json_stringify_value(value: &SmeltUnknown) -> Result<Option<String>, &'static str> {
    smelt_json_stringify_value_with(value, &SmeltUnknown::Undefined)
}

/// [`smelt_json_stringify_value`] with a `replacer` (see
/// `smelt_json_stringify_with`): the top-level property is serialized after
/// `toJSON` and the replacer have answered for it.
#[allow(dead_code)]
fn smelt_json_stringify_value_with(value: &SmeltUnknown, replacer: &SmeltUnknown) -> Result<Option<String>, &'static str> {
    let reader = SmeltJsonReplacer::of(replacer);
    let holder = SmeltUnknown::Object(SmeltObject::new(Vec::from([(String::new(), value.clone())])));
    let answered = smelt_json_property(&reader, &holder, "", value.clone());
    match &answered {
        SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Symbol(_) => Ok(None),
        SmeltUnknown::Object(object) if object.contains_key("__smelt_call") => Ok(None),
        _ => { let mut out = String::new(); smelt_json_write(&answered, &reader, &mut out)?; Ok(Some(out)) }
    }
}
// @smelt:item-end
