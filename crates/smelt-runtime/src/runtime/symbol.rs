// `SmeltSymbol` and `SmeltPropKey`: JavaScript symbols and property keys as
// typed values.
//
// A TypeScript `symbol` used to reach generated Rust only as the erased
// `SmeltUnknown::Symbol` tag, so every `const s = Symbol('x')`, every
// `PropertyKey` and every record holding a symbol was `SmeltUnknown`. These two
// types carry them typed:
//
// * `SmeltSymbol` is a symbol value. It wraps the same opaque spelling the
//   erased tag carries (`Symbol(d)@<site>`, `Symbol.for(d)`, `Symbol.iterator`),
//   so erasing and extracting is a move of one `Rc<str>` and a symbol keeps its
//   identity across the boundary: two symbols are `===` exactly when their
//   spellings are equal, which is the rule the erased `js_strict_eq` applies.
// * `SmeltPropKey` is a JavaScript property key: what `ToPropertyKey` makes
//   of any value used as a key. A number becomes its canonical string, so
//   `r[1]` and `r['1']` name one entry; a symbol stays a symbol and never meets
//   the string of its own description. The type lives in its own module with a
//   private representation, so the ONLY way to build one is the conversion
//   (`SmeltToPropKey`): a record keyed by it cannot be handed a raw key.

// @smelt:item SmeltSymbol when symbol
/// A JavaScript `symbol` value; see the family notes for its identity rule.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SmeltSymbol(::std::rc::Rc<str>);

impl SmeltSymbol {
    //~ The symbol a frontend spelling names.
    fn from_spelling(spelling: &str) -> Self { Self(spelling.into()) }
    //~ Each `Symbol(d)` site counts its evaluations per thread. The first keeps
    //~ the site's spelling, which is what a key folded from the site at compile
    //~ time names; later ones append `#<n>`, which no fold produces, so they
    //~ are distinct symbols with the same description.
    //~ A `Symbol(d)` evaluation at the site `base` names: a fresh symbol.
    fn unique(base: Self) -> Self { thread_local! { static SMELT_SYMBOL_EVALUATIONS: ::std::cell::RefCell<::std::collections::HashMap<::std::rc::Rc<str>, u64>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); } let count = SMELT_SYMBOL_EVALUATIONS.with(|counts| { let mut counts = counts.borrow_mut(); let count = counts.entry(base.0.clone()).or_insert(0); let seen = *count; *count += 1; seen }); if count == 0 { base } else { Self(format!("{}#{count}", base.0).into()) } }
    //~ The symbol an erased `SmeltUnknown::Symbol` payload names.
    const fn from_shared(spelling: ::std::rc::Rc<str>) -> Self { Self(spelling) }
    //~ The opaque spelling this symbol is identified by.
    fn spelling(&self) -> &str { &self.0 }
    //~ `Symbol(d)@<site>` is a unique symbol, `Symbol.for(d)` a registry one,
    //~ and every other spelling (`Symbol.iterator`, a key decoded from an
    //~ erased object) is its own description. A unique symbol created without
    //~ an argument is spelled with an empty description, so `Symbol()` and
    //~ `Symbol('')` both answer `undefined` here.
    //~ `symbol.description`: the text the symbol was created with.
    fn description(&self) -> Option<JsString> { smelt_symbol_description(&self.0).map(JsString::from) }
    //~ `String(symbol)` / `symbol.toString()`: `Symbol(<description>)`.
    fn to_js_string(&self) -> JsString { JsString::from(smelt_symbol_to_string(&self.0)) }
    //~ The string key an ERASED object stores this symbol's property under.
    fn storage_key(&self) -> String { smelt_symbol_property_key(&self.0) }
}

impl ::std::fmt::Debug for SmeltSymbol { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.to_js_string().to_string()) } }
impl ::std::fmt::Display for SmeltSymbol { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.to_js_string().to_string()) } }
//~ Only a placeholder for a slot codegen declares before its first write; no
//~ source expression produces it.
impl Default for SmeltSymbol { fn default() -> Self { Self::from_spelling("Symbol()@0") } }
impl IntoSmeltUnknown for SmeltSymbol { fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Symbol(self.0) } }
impl IntoSmeltUnknown for &SmeltSymbol { fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Symbol(self.0.clone()) } }
//~ A checked cast: an erased value that is not a symbol is a type error the
//~ source's own annotation ruled out, and it keeps the placeholder rather
//~ than inventing a symbol from a string.
impl SmeltFromUnknown for SmeltSymbol { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Symbol(spelling) => Self(spelling), _ => Self::default() } } }
impl SmeltJsKeyEq for SmeltSymbol { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
impl SmeltJsStrictEq for SmeltSymbol { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }

// @smelt:item-end

// @smelt:item SmeltPropKey when symbol
//~ The module is the seal: `SmeltPropKeyRepr` and the tuple field are
//~ private to it, so code outside can read a key but only `SmeltToPropKey`
//~ (and the erased-object decoding, which is the inverse of an encoding this
//~ module owns) can make one.
#[allow(clippy::wildcard_imports)]
mod smelt_prop_key {
    use super::*;

    //~ A JavaScript property key: the result of `ToPropertyKey`.
    #[derive(Clone, PartialEq, Eq, Hash)]
    pub struct SmeltPropKey(SmeltPropKeyRepr);

    //~ The two kinds of property key.
    #[derive(Clone, PartialEq, Eq, Hash)]
    enum SmeltPropKeyRepr { String(JsString), Symbol(SmeltSymbol) }

    impl SmeltPropKey {
        //~ The key when it is a string key.
        pub const fn as_string(&self) -> Option<&JsString> { match &self.0 { SmeltPropKeyRepr::String(text) => Some(text), SmeltPropKeyRepr::Symbol(_) => None } }
        //~ The key when it is a symbol key.
        pub const fn as_symbol(&self) -> Option<&SmeltSymbol> { match &self.0 { SmeltPropKeyRepr::Symbol(symbol) => Some(symbol), SmeltPropKeyRepr::String(_) => None } }
        //~ The key as the value `Reflect.ownKeys` hands back: a string or a symbol.
        pub fn to_unknown(&self) -> SmeltUnknown { match &self.0 { SmeltPropKeyRepr::String(text) => SmeltUnknown::String(text.clone()), SmeltPropKeyRepr::Symbol(symbol) => symbol.clone().into_smelt_unknown() } }
        //~ The string an ERASED object stores this key under: a string key is
        //~ itself and a symbol key its reserved `__smelt_symbol` spelling.
        pub fn storage_key(&self) -> JsString { match &self.0 { SmeltPropKeyRepr::String(text) => text.clone(), SmeltPropKeyRepr::Symbol(symbol) => JsString::from(symbol.storage_key()) } }
        //~ A string only names a symbol when it carries the reserved prefix, so
        //~ the common key never leaves the borrowed check.
        //~ The inverse of [`Self::storage_key`]: a key read off an erased object,
        //~ or a string slot holding a symbol key in its reserved spelling.
        pub fn from_storage_key(key: &JsString) -> Self { if !key.eq_str_prefix("__smelt_symbol") { return Self(SmeltPropKeyRepr::String(key.clone())); } match smelt_own_symbol_key_value(&key.to_string_lossy()) { Some(SmeltUnknown::Symbol(spelling)) => Self(SmeltPropKeyRepr::Symbol(SmeltSymbol::from_shared(spelling))), _ => Self(SmeltPropKeyRepr::String(key.clone())) } }
    }

    impl ::std::fmt::Debug for SmeltPropKey { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { match &self.0 { SmeltPropKeyRepr::String(text) => write!(formatter, "{:?}", text.to_string_lossy()), SmeltPropKeyRepr::Symbol(symbol) => write!(formatter, "[{symbol:?}]") } } }
    impl ::std::fmt::Display for SmeltPropKey { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { match &self.0 { SmeltPropKeyRepr::String(text) => formatter.write_str(&text.to_string_lossy()), SmeltPropKeyRepr::Symbol(symbol) => write!(formatter, "{symbol}") } } }
    //~ Only a placeholder for a slot codegen declares before its first write.
    impl Default for SmeltPropKey { fn default() -> Self { Self(SmeltPropKeyRepr::String(JsString::new())) } }
    impl SmeltJsKeyEq for SmeltPropKey { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
    //~ Deliberately no `IntoSmeltUnknown`/`SmeltFromUnknown` here: those are
    //~ what `SmeltJsMap<K, _>`'s MAP adapters require of `K`, and a record
    //~ keyed by property keys is an object, not a Map. Without them the two
    //~ record adapters below are the only erasure a property-key record has.
    //~ A key crosses through `to_unknown` and `smelt_to_prop_key`.

    //~ JavaScript's `ToPropertyKey`: the one constructor of [`SmeltPropKey`].
    pub trait SmeltToPropKey {
        //~ The property key this value names.
        fn smelt_to_prop_key(&self) -> SmeltPropKey;
    }

    impl SmeltToPropKey for SmeltPropKey { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.clone() } }
    //~ A string is its own key, except Smelt's reserved symbol spelling: a
    //~ statically folded `{ [K]: v }` key, and a symbol held in a `keyof T`
    //~ (`string`) slot, are stored under it, and both name the symbol.
    impl SmeltToPropKey for JsString { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey::from_storage_key(self) } }
    impl SmeltToPropKey for str { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey::from_storage_key(&JsString::from(self)) } }
    impl SmeltToPropKey for String { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.as_str().smelt_to_prop_key() } }
    impl SmeltToPropKey for SmeltSymbol { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey(SmeltPropKeyRepr::Symbol(self.clone())) } }
    //~ A number key is its canonical `Number::toString`: `1.0` is `"1"`, `-0`
    //~ is `"0"`, `1e21` is `"1e+21"`.
    impl SmeltToPropKey for f64 { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self).smelt_to_prop_key() } }
    impl SmeltToPropKey for i32 { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.to_string().smelt_to_prop_key() } }
    #[allow(clippy::cast_precision_loss)]
    impl SmeltToPropKey for i64 { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self as f64).smelt_to_prop_key() } }
    #[allow(clippy::cast_precision_loss)]
    impl SmeltToPropKey for usize { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self as f64).smelt_to_prop_key() } }
    impl SmeltToPropKey for bool { fn smelt_to_prop_key(&self) -> SmeltPropKey { (if *self { "true" } else { "false" }).smelt_to_prop_key() } }
    //~ An absent optional key is `undefined`, whose property key is the
    //~ string `"undefined"`.
    impl<T: SmeltToPropKey> SmeltToPropKey for Option<T> { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.as_ref().map_or_else(|| "undefined".smelt_to_prop_key(), SmeltToPropKey::smelt_to_prop_key) } }
    impl<T: SmeltToPropKey + ?Sized> SmeltToPropKey for &T { fn smelt_to_prop_key(&self) -> SmeltPropKey { (**self).smelt_to_prop_key() } }
    //~ An erased key: a symbol stays a symbol, everything else is its string.
    //~ `smelt_property_key` already is `ToString` for every other tag, but it
    //~ answers the reserved spelling for a symbol, which is the erased
    //~ storage form and not a key, so the symbol arm is taken first.
    impl SmeltToPropKey for SmeltUnknown { fn smelt_to_prop_key(&self) -> SmeltPropKey { match self { Self::Symbol(spelling) => SmeltSymbol::from_shared(spelling.clone()).smelt_to_prop_key(), Self::String(text) => text.smelt_to_prop_key(), Self::Null => "null".smelt_to_prop_key(), other => smelt_property_key(other.clone()).smelt_to_prop_key() } } }

    //~ A property-key record IS a JavaScript object, so it is backed by the
    //~ object field store itself: a `SmeltRecord<JsString, V>` keyed by each
    //~ key's storage spelling (a string key is itself, a symbol key its
    //~ reserved `__smelt_symbol` spelling). That store already keeps array
    //~ indices first in ascending order and the other strings in insertion
    //~ order; the symbol keys are moved after every string key when the
    //~ record is enumerated, which is the whole of the own-key order.
    //~ Sharing the store is what keeps a record a REFERENCE value across the
    //~ erased boundary: an `unknown`-valued record erases to a second handle
    //~ on the same store (`share_unknown`), so a write through either handle
    //~ is seen through the other, as remeda's `clone` relies on when it
    //~ pushes a record into its reference list before filling it.
    //~ The field is private to this module, so every key operation takes a
    //~ `SmeltPropKey` and none can skip `ToPropertyKey`.
    //~ A record keyed by JavaScript property keys: `Record<PropertyKey, V>`,
    //~ or `Record<K, V>` over a type parameter `K`.
    //~ `id` is the record's own object id, a field as on every record
    //~ container, so `===` reads it the same way; it never changes.
    pub struct SmeltPropKeyRecord<V> { pub id: usize, record: SmeltRecord<JsString, V> }

    //~ `Clone` is another handle on the same object, as for every record.
    impl<V> Clone for SmeltPropKeyRecord<V> { fn clone(&self) -> Self { Self::of(self.record.clone()) } }
    impl<V> Default for SmeltPropKeyRecord<V> { fn default() -> Self { Self::new() } }

    //~ Whether a stored key is one of JavaScript's own keys: Smelt's
    //~ representation markers (`__smelt_proto:`, `__smelt_method:`,
    //~ `__smelt_class`, `__smelt_call`, ...) are not, a symbol's reserved
    //~ spelling is.
    fn smelt_prop_key_is_own(key: &JsString) -> bool { !key.smelt_is_internal_key() || key.eq_str_prefix("__smelt_symbol") }

    //~ The writers take `&mut self` and an owned key because they are the
    //~ `SmeltJsMap` surface the emitter spells (`map.insert(key, value)` on a
    //~ `let mut`), which this record answers in its place.
    #[allow(clippy::needless_pass_by_ref_mut, clippy::needless_pass_by_value, reason = "the SmeltJsMap writer surface the emitter spells")]
    impl<V> SmeltPropKeyRecord<V> {
        //~ An empty record with a fresh object id.
        pub fn new() -> Self { Self::of(SmeltRecord::new()) }
        //~ The record over `record`'s store.
        const fn of(record: SmeltRecord<JsString, V>) -> Self { Self { id: record.id, record } }
        //~ Whether `key` is an own key of the record.
        pub fn contains_key(&self, key: &SmeltPropKey) -> bool { self.record.contains_key(&key.storage_key()) }
        //~ Store `value` under `key`, answering the value it replaced.
        pub fn insert(&mut self, key: SmeltPropKey, value: V) -> Option<V> { self.record.insert(key.storage_key(), value) }
        //~ Delete `key`, answering the value it held.
        pub fn remove(&mut self, key: &SmeltPropKey) -> Option<V> { self.record.remove(&key.storage_key()) }
        //~ Delete every own key, keeping the object's identity.
        pub fn clear(&mut self) { let keys: Vec<JsString> = self.record.keys().collect(); for key in keys { self.record.remove(&key); } }
        //~ Insert each entry in turn.
        pub fn extend<I: IntoIterator<Item = (SmeltPropKey, V)>>(&mut self, entries: I) { for (key, value) in entries { self.insert(key, value); } }
        //~ A record from an erased object's own entries (or an array's
        //~ elements by index), each value recovered by `item`: a copy, since
        //~ the values change representation.
        pub fn from_unknown_with(value: SmeltUnknown, mut item: impl FnMut(SmeltUnknown) -> V) -> Self { match value { SmeltUnknown::Object(values) => Self::of(SmeltRecord::with_id_from_entries(values.id, values.into_iter().map(|(key, value)| (key, item(value))))), SmeltUnknown::Array(values) => values.into_vec().into_iter().enumerate().map(|(index, value)| (index.smelt_to_prop_key(), item(value))).collect(), _ => Self::new() } }
        //~ The value at `key`, inserting `default()` first when it is absent.
        pub fn entry_or_insert(&mut self, key: SmeltPropKey, default: impl FnOnce() -> V) -> ::std::cell::RefMut<'_, V> { self.record.entry_or_insert(key.storage_key(), default) }
    }

    impl<V: Clone> SmeltPropKeyRecord<V> {
        //~ The value at `key`.
        pub fn get(&self, key: &SmeltPropKey) -> Option<V> { self.record.get(&key.storage_key()) }
        //~ The own entries in JavaScript's own-key order: array indices
        //~ ascending, then string keys, then symbol keys, each of the last two
        //~ in insertion order.
        pub fn iter(&self) -> ::std::vec::IntoIter<(SmeltPropKey, V)> { let (mut strings, symbols): (Vec<_>, Vec<_>) = self.record.iter().filter(|(key, _)| smelt_prop_key_is_own(key)).map(|(key, value)| (SmeltPropKey::from_storage_key(&key), value)).partition(|(key, _)| key.as_string().is_some()); strings.extend(symbols); strings.into_iter() }
        //~ The own keys, in own-key order.
        pub fn keys(&self) -> ::std::vec::IntoIter<SmeltPropKey> { self.iter().map(|(key, _)| key).collect::<Vec<_>>().into_iter() }
        //~ The own values, in own-key order.
        pub fn values(&self) -> ::std::vec::IntoIter<V> { self.iter().map(|(_, value)| value).collect::<Vec<_>>().into_iter() }
        //~ The number of own keys.
        pub fn len(&self) -> usize { self.record.keys().filter(smelt_prop_key_is_own).count() }
        //~ Whether the record has no own key.
        pub fn is_empty(&self) -> bool { self.len() == 0 }
        //~ `Object.entries`: the string-keyed own entries, in own-key order.
        pub fn string_entries(&self) -> Vec<(JsString, V)> { self.iter().filter_map(|(key, value)| key.as_string().cloned().map(|text| (text, value))).collect() }
        //~ `Object.getOwnPropertySymbols`: the symbol keys, in insertion order.
        pub fn symbols(&self) -> Vec<SmeltSymbol> { self.iter().filter_map(|(key, _)| key.as_symbol().cloned()).collect() }
        //~ `for...in`: the own string keys in own-key order, then the
        //~ inherited members an erased `Object.create(proto)` object carries as
        //~ `__smelt_proto:<key>` entries, unprefixed and not repeating an own
        //~ key.
        pub fn for_in_keys(&self) -> Vec<JsString> { let mut keys: Vec<JsString> = self.string_entries().into_iter().map(|(key, _)| key).collect(); let mut seen: ::std::collections::HashSet<JsString> = keys.iter().cloned().collect(); for key in self.record.keys() { if let Some(inherited) = key.to_string_lossy().strip_prefix("__smelt_proto:") { let inherited = JsString::from(inherited); if seen.insert(inherited.clone()) { keys.push(inherited); } } } keys }
        //~ The record as an erased object with its own id, each value erased
        //~ by `item`: a copy, since the values change representation.
        pub fn to_unknown_with(&self, mut item: impl FnMut(V) -> SmeltUnknown) -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::with_id(self.record.id, self.record.iter().map(|(key, value)| (key, item(value))).collect())) }
    }

    impl SmeltPropKeyRecord<SmeltUnknown> {
        //~ The record as an erased object: a second handle on the SAME
        //~ store, so the erased value and the record stay one object.
        pub fn share_unknown(&self) -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::from_unknown_record(self.record.clone())) }
        //~ An erased object read as a record: a second handle on the SAME
        //~ store. Any other value has no own entries but an array's elements,
        //~ which are copied under their index keys.
        pub fn from_shared_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => Self::of(object.smelt_shared_record()), other => Self::from_unknown_with(other, |value| value) } }
    }

    impl<V> FromIterator<(SmeltPropKey, V)> for SmeltPropKeyRecord<V> { fn from_iter<I: IntoIterator<Item = (SmeltPropKey, V)>>(entries: I) -> Self { let mut record = Self::new(); record.extend(entries); record } }
    impl<V, const N: usize> From<[(SmeltPropKey, V); N]> for SmeltPropKeyRecord<V> { fn from(entries: [(SmeltPropKey, V); N]) -> Self { entries.into_iter().collect() } }
    impl<V: Clone> IntoIterator for SmeltPropKeyRecord<V> { type Item = (SmeltPropKey, V); type IntoIter = ::std::vec::IntoIter<(SmeltPropKey, V)>; fn into_iter(self) -> Self::IntoIter { self.iter() } }
    impl<V: Clone + PartialEq> PartialEq for SmeltPropKeyRecord<V> { fn eq(&self, other: &Self) -> bool { self.len() == other.len() && self.iter().all(|(key, value)| other.get(&key).is_some_and(|found| found == value)) } }
    impl<V: Clone + Eq> Eq for SmeltPropKeyRecord<V> {}
    impl<V: Clone + ::std::fmt::Debug> ::std::fmt::Debug for SmeltPropKeyRecord<V> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltPropKeyRecord").field("id", &self.record.id).field("entries", &self.iter().collect::<Vec<_>>()).finish() } }
    //~ The erasure adapters copy, since `V` changes representation; the
    //~ emitter erases an `unknown`-valued record through `share_unknown`
    //~ instead, which keeps it one object.
    impl<V: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltPropKeyRecord<V> { fn into_smelt_unknown(self) -> SmeltUnknown { self.to_unknown_with(IntoSmeltUnknown::into_smelt_unknown) } }
    impl<V: SmeltFromUnknown> SmeltFromUnknown for SmeltPropKeyRecord<V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { Self::from_unknown_with(value, V::smelt_from_unknown) } }
}
pub use smelt_prop_key::*;

// @smelt:item-end
