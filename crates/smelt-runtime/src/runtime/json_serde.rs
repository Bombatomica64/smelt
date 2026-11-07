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
            Self::String(value) => serializer.serialize_str(&value.to_string_lossy()),
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
