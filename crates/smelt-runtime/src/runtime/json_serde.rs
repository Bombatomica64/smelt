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
            Self::Array(values) => serde::Serialize::serialize(&*values.values.borrow(), serializer),
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
    let mut out = String::new();
    smelt_json_write(value, &mut out)?;
    Ok(out)
}

/// Appends `QuoteJSONString(text)`: `"` and `\` escaped, the short escapes
/// for `\b \f \n \r \t`, other control characters and every lone surrogate as
/// a lowercase `\uXXXX`, anything else as itself.
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
fn smelt_json_write(value: &SmeltUnknown, out: &mut String) -> Result<(), &'static str> {
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
        SmeltUnknown::Array(values) => {
            out.push('[');
            for (index, element) in values.values.borrow().iter().enumerate() {
                if index > 0 { out.push(','); }
                smelt_json_write(element, out)?;
            }
            out.push(']');
        }
        //~ Own enumerable properties in insertion order, as in the `Serialize`
        //~ impl: a byte-backed view as its element indices, and a property
        //~ whose value is `undefined`, a function or a symbol omitted.
        SmeltUnknown::Object(values) => {
            out.push('{');
            if let Some(elements) = smelt_host_buffer_own_elements(value) {
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 { out.push(','); }
                    smelt_json_quote(&JsString::from(index.to_string()), out);
                    out.push(':');
                    smelt_json_write(element, out)?;
                }
                out.push('}');
                return Ok(());
            }
            let shape = smelt_for_in_object_shape(values);
            let store = values.store.borrow();
            let mut first = true;
            for entry in store.entries() {
                if matches!(entry.value, SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Symbol(_)) || !smelt_is_for_in_shape_key(shape, &entry.key) { continue; }
                if !first { out.push(','); }
                first = false;
                smelt_json_quote(&entry.key, out);
                out.push(':');
                smelt_json_write(&entry.value, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}
// @smelt:item-end
