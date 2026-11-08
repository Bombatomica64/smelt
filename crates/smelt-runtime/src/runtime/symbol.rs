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
    /// The symbol a frontend spelling names.
    fn from_spelling(spelling: &str) -> Self { Self(spelling.into()) }
    /// The symbol an erased `SmeltUnknown::Symbol` payload names.
    fn from_shared(spelling: ::std::rc::Rc<str>) -> Self { Self(spelling) }
    /// The opaque spelling this symbol is identified by.
    fn spelling(&self) -> &str { &self.0 }
    //~ `Symbol(d)@<site>` is a unique symbol, `Symbol.for(d)` a registry one,
    //~ and every other spelling (`Symbol.iterator`, a key decoded from an
    //~ erased object) is its own description. A unique symbol created without
    //~ an argument is spelled with an empty description, so `Symbol()` and
    //~ `Symbol('')` both answer `undefined` here.
    /// `symbol.description`: the text the symbol was created with.
    fn description(&self) -> Option<JsString> { smelt_symbol_description(&self.0).map(JsString::from) }
    /// `String(symbol)` / `symbol.toString()`: `Symbol(<description>)`.
    fn to_js_string(&self) -> JsString { JsString::from(smelt_symbol_to_string(&self.0)) }
    /// The string key an ERASED object stores this symbol's property under.
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
mod smelt_prop_key {
    use super::*;

    /// A JavaScript property key: the result of `ToPropertyKey`.
    #[derive(Clone, PartialEq, Eq, Hash)]
    pub struct SmeltPropKey(SmeltPropKeyRepr);

    /// The two kinds of property key.
    #[derive(Clone, PartialEq, Eq, Hash)]
    enum SmeltPropKeyRepr { String(JsString), Symbol(SmeltSymbol) }

    impl SmeltPropKey {
        /// The key when it is a string key.
        pub fn as_string(&self) -> Option<&JsString> { match &self.0 { SmeltPropKeyRepr::String(text) => Some(text), SmeltPropKeyRepr::Symbol(_) => None } }
        /// The key when it is a symbol key.
        pub fn as_symbol(&self) -> Option<&SmeltSymbol> { match &self.0 { SmeltPropKeyRepr::Symbol(symbol) => Some(symbol), SmeltPropKeyRepr::String(_) => None } }
        /// The key as the value `Reflect.ownKeys` hands back: a string or a symbol.
        pub fn to_unknown(&self) -> SmeltUnknown { match &self.0 { SmeltPropKeyRepr::String(text) => SmeltUnknown::String(text.clone()), SmeltPropKeyRepr::Symbol(symbol) => symbol.clone().into_smelt_unknown() } }
        /// The string an ERASED object stores this key under: a string key is
        /// itself and a symbol key its reserved `__smelt_symbol` spelling.
        pub fn storage_key(&self) -> JsString { match &self.0 { SmeltPropKeyRepr::String(text) => text.clone(), SmeltPropKeyRepr::Symbol(symbol) => JsString::from(symbol.storage_key()) } }
        /// The inverse of [`Self::storage_key`]: a key read off an erased object.
        pub fn from_storage_key(key: &JsString) -> Self { let text = key.to_string_lossy(); match smelt_own_symbol_key_value(&text) { Some(SmeltUnknown::Symbol(spelling)) => Self(SmeltPropKeyRepr::Symbol(SmeltSymbol::from_shared(spelling))), _ => Self(SmeltPropKeyRepr::String(key.clone())) } }
        /// The array index this key names, when it is a canonical one.
        fn array_index(&self) -> Option<u32> { match &self.0 { SmeltPropKeyRepr::String(text) => text.smelt_array_index(), SmeltPropKeyRepr::Symbol(_) => None } }
        /// Where JavaScript's own-key order puts this key: array indices first
        /// by value, then string keys, then symbol keys, each of the last two
        /// in insertion order.
        pub fn own_key_rank(&self) -> (u8, u32) { match (&self.0, self.array_index()) { (_, Some(index)) => (0, index), (SmeltPropKeyRepr::String(_), None) => (1, 0), (SmeltPropKeyRepr::Symbol(_), None) => (2, 0) } }
    }

    impl ::std::fmt::Debug for SmeltPropKey { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { match &self.0 { SmeltPropKeyRepr::String(text) => write!(formatter, "{:?}", text.to_string_lossy()), SmeltPropKeyRepr::Symbol(symbol) => write!(formatter, "[{symbol:?}]") } } }
    impl ::std::fmt::Display for SmeltPropKey { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { match &self.0 { SmeltPropKeyRepr::String(text) => formatter.write_str(&text.to_string_lossy()), SmeltPropKeyRepr::Symbol(symbol) => write!(formatter, "{symbol}") } } }
    //~ Only a placeholder for a slot codegen declares before its first write.
    impl Default for SmeltPropKey { fn default() -> Self { Self(SmeltPropKeyRepr::String(JsString::new())) } }
    impl SmeltJsKeyEq for SmeltPropKey { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
    impl IntoSmeltUnknown for SmeltPropKey { fn into_smelt_unknown(self) -> SmeltUnknown { self.to_unknown() } }
    impl SmeltFromUnknown for SmeltPropKey { fn smelt_from_unknown(value: SmeltUnknown) -> Self { value.smelt_to_prop_key() } }

    /// JavaScript's `ToPropertyKey`: the one constructor of [`SmeltPropKey`].
    pub trait SmeltToPropKey {
        /// The property key this value names.
        fn smelt_to_prop_key(&self) -> SmeltPropKey;
    }

    impl SmeltToPropKey for SmeltPropKey { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.clone() } }
    impl SmeltToPropKey for JsString { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey(SmeltPropKeyRepr::String(self.clone())) } }
    impl SmeltToPropKey for str { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey(SmeltPropKeyRepr::String(JsString::from(self))) } }
    impl SmeltToPropKey for String { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.as_str().smelt_to_prop_key() } }
    impl SmeltToPropKey for SmeltSymbol { fn smelt_to_prop_key(&self) -> SmeltPropKey { SmeltPropKey(SmeltPropKeyRepr::Symbol(self.clone())) } }
    //~ A number key is its canonical `Number::toString`: `1.0` is `"1"`, `-0`
    //~ is `"0"`, `1e21` is `"1e+21"`.
    impl SmeltToPropKey for f64 { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self).smelt_to_prop_key() } }
    impl SmeltToPropKey for i32 { fn smelt_to_prop_key(&self) -> SmeltPropKey { self.to_string().smelt_to_prop_key() } }
    impl SmeltToPropKey for i64 { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self as f64).smelt_to_prop_key() } }
    impl SmeltToPropKey for usize { fn smelt_to_prop_key(&self) -> SmeltPropKey { smelt_number_to_string(*self as f64).smelt_to_prop_key() } }
    impl SmeltToPropKey for bool { fn smelt_to_prop_key(&self) -> SmeltPropKey { (if *self { "true" } else { "false" }).smelt_to_prop_key() } }
    //~ An absent optional key is `undefined`, whose property key is the
    //~ string `"undefined"`.
    impl<T: SmeltToPropKey> SmeltToPropKey for Option<T> { fn smelt_to_prop_key(&self) -> SmeltPropKey { match self { Some(value) => value.smelt_to_prop_key(), None => "undefined".smelt_to_prop_key() } } }
    impl<T: SmeltToPropKey + ?Sized> SmeltToPropKey for &T { fn smelt_to_prop_key(&self) -> SmeltPropKey { (**self).smelt_to_prop_key() } }
    //~ An erased key: a symbol stays a symbol, everything else is its string.
    //~ `smelt_property_key` already is `ToString` for every other tag, but it
    //~ answers the reserved spelling for a symbol, which is the erased
    //~ storage form and not a key, so the symbol arm is taken first.
    impl SmeltToPropKey for SmeltUnknown { fn smelt_to_prop_key(&self) -> SmeltPropKey { match self { SmeltUnknown::Symbol(spelling) => SmeltSymbol::from_shared(spelling.clone()).smelt_to_prop_key(), SmeltUnknown::String(text) => text.smelt_to_prop_key(), SmeltUnknown::Null => "null".smelt_to_prop_key(), other => smelt_property_key(other.clone()).smelt_to_prop_key() } } }
}
pub use smelt_prop_key::*;

//~ The record operations a property-key record needs beyond `SmeltJsMap`'s own:
//~ its own-key order, and its two crossings of the erased boundary.

/// A property-key record's entries in JavaScript's own-key order: array
/// indices ascending, then string keys, then symbol keys, each of the last two
/// in insertion order (`sort_by_key` is stable).
fn smelt_prop_key_entries<V: Clone>(map: &SmeltJsMap<SmeltPropKey, V>) -> Vec<(SmeltPropKey, V)> { let mut entries: Vec<(SmeltPropKey, V)> = map.iter().collect(); entries.sort_by_key(|(key, _)| key.own_key_rank()); entries }

//~ The representation markers an erased object carries (inherited members,
//~ prototype methods, class provenance) are copied in verbatim when one is
//~ read into a property-key record; like `smelt_own_js_map_entries`, the own
//~ string keys leave them out.
/// `Object.entries`: the string-keyed entries, in own-key order.
fn smelt_prop_key_string_entries<V: Clone>(map: &SmeltJsMap<SmeltPropKey, V>) -> Vec<(JsString, V)> { smelt_prop_key_entries(map).into_iter().filter_map(|(key, value)| key.as_string().filter(|text| { let text = text.to_string_lossy(); !text.starts_with("__smelt_proto:") && !text.starts_with("__smelt_method:") && text != "__smelt_class" }).cloned().map(|text| (text, value))).collect() }

/// `Object.getOwnPropertySymbols`: the symbol keys, in insertion order.
fn smelt_prop_key_symbols<V: Clone>(map: &SmeltJsMap<SmeltPropKey, V>) -> Vec<SmeltSymbol> { map.iter().filter_map(|(key, _)| key.as_symbol().cloned()).collect() }

/// A property-key record from an erased object's own entries.
fn smelt_prop_key_record_from_unknown<V>(value: SmeltUnknown, mut item: impl FnMut(SmeltUnknown) -> V) -> SmeltJsMap<SmeltPropKey, V> { match value { SmeltUnknown::Object(values) => values.into_iter().map(|(key, value)| (SmeltPropKey::from_storage_key(&key), item(value))).collect(), SmeltUnknown::Array(values) => values.into_vec().into_iter().enumerate().map(|(index, value)| (index.smelt_to_prop_key(), item(value))).collect(), _ => SmeltJsMap::new() } }

/// A property-key record as an erased object, symbol keys under their reserved
/// storage spelling.
fn smelt_prop_key_record_to_unknown<V: Clone>(map: &SmeltJsMap<SmeltPropKey, V>, mut item: impl FnMut(V) -> SmeltUnknown) -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::new(smelt_prop_key_entries(map).into_iter().map(|(key, value)| (key.storage_key(), item(value))).collect())) }

// @smelt:item-end
