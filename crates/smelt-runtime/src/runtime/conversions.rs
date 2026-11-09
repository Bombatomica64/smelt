// `IntoSmeltUnknown`, `SmeltFromUnknown` and the extension traits over them.

// @smelt:item IntoSmeltUnknown when unknown
pub trait IntoSmeltUnknown {
    fn into_smelt_unknown(self) -> SmeltUnknown;
}

//~ A callback parameter passed by shared reference (see
//~ `callback_param_is_shared_reference`) is still erased back into a JS array
//~ by the erased-callback adapters, and that walk yields `&SmeltUnknown`
//~ rather than an owned value. Erasing a borrowed value clones it, which the
//~ erasure would have done anyway when it built the `SmeltArray`; the saving
//~ is on the ARGUMENT, which no longer deep-copies the list per element.
impl IntoSmeltUnknown for &SmeltUnknown { fn into_smelt_unknown(self) -> SmeltUnknown { self.clone() } }

impl IntoSmeltUnknown for SmeltUnknown {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        self
    }
}

impl IntoSmeltUnknown for bool {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Bool(self)
    }
}

impl IntoSmeltUnknown for f64 {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Number(self)
    }
}

impl IntoSmeltUnknown for i64 {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Number(self as f64)
    }
}

impl IntoSmeltUnknown for String {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::String(self.into())
    }
}

impl IntoSmeltUnknown for JsString {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::String(self)
    }
}

impl IntoSmeltUnknown for () {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Null
    }
}

//~ Erasing a typed list yields an identity-bearing `SmeltUnknown::Array`,
//~ carrying the list's reference id and erasing each element in turn.
impl<T: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltList<T> { fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray::with_id(self.id(), self.into_vec().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())) } }

pub trait SmeltFromUnknown {
    /// Whether converting a value of this type may convert a generated record,
    /// so a container tracks its element index for the error path.
    const HOLDS_RECORD: bool = false;
    fn smelt_from_unknown(value: SmeltUnknown) -> Self;
}

impl SmeltFromUnknown for SmeltUnknown {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        value
    }
}

impl SmeltFromUnknown for bool {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), /*@if bigint*/SmeltUnknown::BigInt(value) => !::num_traits::Zero::is_zero(&value), /*@end*/SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }
    }
}

impl SmeltFromUnknown for f64 {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value { SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get("__smelt_date") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }, /*@if bigint*/SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value), /*@end*/SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }
    }
}

impl SmeltFromUnknown for i64 {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value { SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get("__smelt_date") { Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }, /*@if bigint*/SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value) as i64, /*@end*/SmeltUnknown::String(value) => smelt_js_string_to_number(&value) as i64, SmeltUnknown::Bool(value) => if value { 1_i64 } else { 0_i64 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => 0_i64 }
    }
}

impl SmeltFromUnknown for String {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value { SmeltUnknown::String(value) => value.to_string_lossy(), SmeltUnknown::Symbol(value) => smelt_symbol_to_string(&value), /*@if bigint*/SmeltUnknown::BigInt(value) => value.to_string(), /*@end*/SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }
    }
}

impl SmeltFromUnknown for JsString {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value { SmeltUnknown::String(value) => value, other => Self::from(<String as SmeltFromUnknown>::smelt_from_unknown(other)) }
    }
}

//~ The mirror of `IntoSmeltUnknown for ()`: Rust `()` is the collapsed
//~ JavaScript nullish type (`null` and `undefined` both lower to
//~ `Type::None`), so any erased value un-erases to it. Needed when a
//~ generic is instantiated at an all-nullish element type
//~ (`uniq([null, undefined])` binds `T = ()`).
impl SmeltFromUnknown for () {
    fn smelt_from_unknown(_value: SmeltUnknown) -> Self {
        
    }
}

//~ Un-erasing a `SmeltUnknown` back into statically typed collections is the
//~ mirror of the `IntoSmeltUnknown` impls above: a returned generic value
//~ (e.g. `identity<T>(x): T` inferred at `SmeltRecord<String, String>`) is
//~ erased to `SmeltUnknown` inside the generic body and must be converted
//~ back element-wise at the typed binding site. Each impl reuses the source
//~ container's reference id so round-tripping preserves JS identity, and
//~ converts each element/entry through the element type's own
//~ `SmeltFromUnknown`. A non-matching `SmeltUnknown` variant yields an empty
//~ container (the JS-coercion fallback, matching the scalar impls above).
impl<T: SmeltFromUnknown + 'static> SmeltFromUnknown for SmeltList<T> { const HOLDS_RECORD: bool = T::HOLDS_RECORD; fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Array(array) => { let id = array.id; let list = if T::HOLDS_RECORD { let _smelt_conv_list = smelt_conv_enter_list(); Self::with_id(id, array.into_vec().into_iter().map(|item| { smelt_conv_next_item(); T::smelt_from_unknown(item) }).collect()) } else { SmeltList::with_id(id, array.into_vec().into_iter().map(T::smelt_from_unknown).collect()) }; smelt_array_props_adopt(id, &list.storage()); list }, _ => SmeltList::new(Vec::new()) } } }

impl<K: SmeltFromUnknown + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltRecord<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => SmeltRecord::with_id_from_entries(object.id, object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key.into())), V::smelt_from_unknown(value)))), _ => SmeltRecord::with_id_from_entries(smelt_next_object_id(), ::std::iter::empty()) } } }

//~ Un-erase a `Map`. A `__smelt_map` marker object restores the original
//~ entries (from the `[[k, v], ...]` pair array) and the source `id`, so the
//~ erasure round-trip preserves JS identity. A plain object (no marker) still
//~ decodes as string-keyed entries — the "Map and Record share Dict
//~ internally" tolerance — and any other value yields an empty map.
impl<K: SmeltFromUnknown + SmeltJsKeyEq + Clone, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltJsMap<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => { if let Some(SmeltUnknown::Array(pairs)) = object.get("__smelt_map") { let mut map = SmeltJsMap { id: object.id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::new())) }; for pair in pairs.into_vec() { if let SmeltUnknown::Array(entry) = pair { let mut entry = entry.into_vec().into_iter(); if let (Some(key), Some(value)) = (entry.next(), entry.next()) { map.insert(K::smelt_from_unknown(key), V::smelt_from_unknown(value)); } } } map } else { object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key.into())), V::smelt_from_unknown(value))).collect() } }, _ => SmeltJsMap::default() } } }

//~ Un-erase a `Set`. A `__smelt_set` marker object restores the original
//~ members (from the members array) and the source `id`, so the erasure
//~ round-trip preserves JS identity — mirrors the `SmeltJsMap` decode. The
//~ bare-`Array` arm is the tolerant back-compat boundary: an erased value
//~ that is a plain array (e.g. produced outside this stage's marker path,
//~ or a genuine dynamic-interop array coerced to a `Set`) still decodes as
//~ set members via SameValueZero insert. Any other value yields an empty set.
impl<T: SmeltFromUnknown> SmeltFromUnknown for Option<T> { const HOLDS_RECORD: bool = T::HOLDS_RECORD; fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Null | SmeltUnknown::Undefined => None, other => Some(T::smelt_from_unknown(other)) } } }
impl<T: SmeltFromUnknown + Clone + IntoSmeltUnknown> SmeltFromUnknown for SmeltJsSet<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => { if let Some(SmeltUnknown::Array(members)) = object.get("__smelt_set") { let mut set = SmeltJsSet::with_id(object.id); for member in members.into_vec() { set.insert(T::smelt_from_unknown(member)); } set } else { SmeltJsSet::default() } }, SmeltUnknown::Array(members) => { let mut set = SmeltJsSet::new(); for member in members.into_vec() { set.insert(T::smelt_from_unknown(member)); } set }, _ => SmeltJsSet::default() } } }

//~ Un-erase a TUPLE: the exact inverse of `IntoSmeltUnknown for (A, B)`
//~ below, which encodes a pair as a two-element `SmeltUnknown::Array`.
//~
//~ Without this, a generic whose argument is a tuple had no way back from
//~ the runtime carrier: a `T` recovered as `<T as
//~ SmeltFromUnknown>::smelt_from_unknown(..)` (the recovery arm the
//~ render scope selects when the position CAN spell the parameter — see
//~ `blocker-logs/hono-h42-erasure-substitution.md`) failed to compile the
//~ moment `T` was instantiated at a tuple, which is what Hono's
//~ `Router<[unknown, RouterRoute]>` is. The recovery rule was general;
//~ the trait's coverage of the type it recovers INTO was not, and a
//~ missing impl is not a reason to erase the value instead.
//~
//~ Arity mirrors `IntoSmeltUnknown` exactly: adding an arity to one side
//~ without the other would make the round trip asymmetric, which is the
//~ defect this closes.
//~
//~ A non-array value, or an array shorter than the tuple, fills the
//~ missing slots from `Undefined` and lets each element type apply its
//~ own JS coercion — the same tolerant fallback every container impl
//~ above takes, rather than a panic on a shape the source already
//~ declared.
impl<A: SmeltFromUnknown, B: SmeltFromUnknown> SmeltFromUnknown for (A, B) { fn smelt_from_unknown(value: SmeltUnknown) -> Self { let mut items = match value { SmeltUnknown::Array(values) => values.into_vec().into_iter(), _ => Vec::new().into_iter() }; (A::smelt_from_unknown(items.next().unwrap_or(SmeltUnknown::Undefined)), B::smelt_from_unknown(items.next().unwrap_or(SmeltUnknown::Undefined))) } }
//~ The one-element tuple (`[string]`, an object-shaped alias's
//~ `buffer: [string]` field) is the same rule at arity one.
impl<A: SmeltFromUnknown> SmeltFromUnknown for (A,) { fn smelt_from_unknown(value: SmeltUnknown) -> Self { let mut items = match value { SmeltUnknown::Array(values) => values.into_vec().into_iter(), _ => Vec::new().into_iter() }; (A::smelt_from_unknown(items.next().unwrap_or(SmeltUnknown::Undefined)),) } }

trait SmeltIntoF64 {
    fn smelt_into_f64(self) -> f64;
}

impl SmeltIntoF64 for f64 {
    fn smelt_into_f64(self) -> f64 {
        self
    }
}

impl SmeltIntoF64 for SmeltUnknown {
    fn smelt_into_f64(self) -> f64 {
        match self {
            SmeltUnknown::Number(value) => value,
            _ => 0.0,
        }
    }
}

impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Option<T> {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        self.map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown)
    }
}

impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Vec<T> {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Array(self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())
    }
}

impl<T: IntoSmeltUnknown + Eq + ::std::hash::Hash> IntoSmeltUnknown for ::std::collections::HashSet<T> {
    /// Erase JavaScript Set values as iterable array-like values.
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let mut values = self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>();
        values.sort_by_key(smelt_unknown_stable_hash_key);
        SmeltUnknown::Array(values.into())
    }
}

trait SmeltArrayExt<T> {
    /// Return the first JavaScript-style index of a value, or -1 when absent.
    fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq;
}

impl<T> SmeltArrayExt<T> for Vec<T> {
    /// Implement `Array.prototype.indexOf` for generated Rust vectors.
    fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq {
        self.iter().position(|value| value == &needle).map_or(SmeltUnknown::Number(-1.0), |index| SmeltUnknown::Number(index as f64))
    }
}

trait SmeltUnknownArrayExt {
    /// Concatenate erased array values with JavaScript-style spreading.
    fn concat(self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>;
}

impl SmeltUnknownArrayExt for Vec<SmeltUnknown> {
    /// Implement the generated rest-argument concat helper for erased arrays.
    fn concat(mut self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown> {
        self.extend(other);
        self
    }
}

trait SmeltUnitExt {
    /// Return false for generated calls against unreachable unit placeholders.
    fn includes<T>(&self, _needle: T) -> bool;
}

impl SmeltUnitExt for () {
    /// Keep unreachable JavaScript membership calls type-checkable.
    fn includes<T>(&self, _needle: T) -> bool {
        false
    }
}

impl<A: IntoSmeltUnknown> IntoSmeltUnknown for (A,) {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Array(vec![self.0.into_smelt_unknown()].into())
    }
}

impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B) {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Array(vec![self.0.into_smelt_unknown(), self.1.into_smelt_unknown()].into())
    }
}

impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash, T: IntoSmeltUnknown {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Object(SmeltObject::new(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), /*@if bigint*/SmeltUnknown::BigInt(value) => value.to_string(), /*@end*/SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => "null".to_owned(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))
    }
}

// @smelt:if dep_indexmap
impl<K, T> IntoSmeltUnknown for ::indexmap::IndexMap<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash, T: IntoSmeltUnknown {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Object(SmeltObject::new(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), /*@if bigint*/SmeltUnknown::BigInt(value) => value.to_string(), /*@end*/SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => "null".to_owned(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))
    }
}
// @smelt:end
impl<K, T> IntoSmeltUnknown for SmeltRecord<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, T: IntoSmeltUnknown + Clone {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Object(SmeltObject::with_id(self.id, self.iter().into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), /*@if bigint*/SmeltUnknown::BigInt(value) => value.to_string(), /*@end*/SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => "null".to_owned(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))
    }
}

// @smelt:item-end
