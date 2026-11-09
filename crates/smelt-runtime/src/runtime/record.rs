// `SmeltRecord`: a string-keyed record that keeps its JavaScript object identity.

// @smelt:item SmeltRecord when record
#[derive(Debug)]
pub struct SmeltRecord<K, V> {
    id: usize,
// @smelt:if reference_origins
    store: ::std::rc::Rc<SmeltStoreCell<K, V>>,
// @smelt:else
//=     store: ::std::rc::Rc<::std::cell::RefCell<SmeltFieldStore<K, V>>>,
// @smelt:end
}

// @smelt:item-end

// @smelt:item impl_Clone_for_SmeltRecord_K_V when record
impl<K, V> Clone for SmeltRecord<K, V> {
    fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } }
}

// @smelt:item-end

// @smelt:item impl_serde_Serialize_for_SmeltRecord_K_V when dep_serde_json & record
impl<K, V> serde::Serialize for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash + Clone + serde::Serialize, V: serde::Serialize {
    //~ Serialize record entries in JavaScript insertion order.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let store = self.store.borrow();
        let mut map = serde::Serializer::serialize_map(serializer, Some(store.len()))?;
        for entry in store.entries() { serde::ser::SerializeMap::serialize_entry(&mut map, &entry.key, &entry.value)?; }
        serde::ser::SerializeMap::end(map)
    }
}

//~ The inverse: a JSON object read into a record field of a struct
//~ that derives `Deserialize` (`idp?: Record<string, string>` on an
//~ interface parsed with `JSON.parse`). Entries go through `insert`,
//~ so the record's key order is JavaScript's own-key order — the
//~ order `JSON.parse` would give the object.
impl<'de, K, V> serde::Deserialize<'de> for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey + serde::Deserialize<'de>, V: serde::Deserialize<'de> {
    //~ Deserialize a JSON object into a record, entry by entry.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SmeltRecordVisitor<K, V>(::std::marker::PhantomData<(K, V)>);
        impl<'de, K, V> serde::de::Visitor<'de> for SmeltRecordVisitor<K, V> where K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey + serde::Deserialize<'de>, V: serde::Deserialize<'de> {
            type Value = SmeltRecord<K, V>;
            fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("an object") }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let record = SmeltRecord::new();
                while let Some((key, value)) = map.next_entry::<K, V>()? { record.insert(key, value); }
                Ok(record)
            }
        }
        deserializer.deserialize_map(SmeltRecordVisitor(::std::marker::PhantomData))
    }
}

// @smelt:item-end

// @smelt:item SmeltOwnedOptionCloned when record
trait SmeltOwnedOptionCloned<T> {
    //~ Return owned optional values unchanged when generated shared lookup code calls `.cloned()`.
    fn cloned(self) -> Option<T>;
}

impl<T> SmeltOwnedOptionCloned<T> for Option<T> {
    fn cloned(self) -> Option<T> { self }
}

//~ JavaScript own-property key order (`OrdinaryOwnPropertyKeys`) is NOT
//~ plain insertion order: keys that spell a canonical array index come
//~ first in ascending numeric order, and only then do the remaining
//~ string keys follow in insertion order. `SmeltRecord`/`SmeltObject`
//~ therefore keep `entries` (the own-key-ordered vector in `SmeltFieldStore`) in that canonical order at INSERT time — one
//~ ordered structure, so `keys()`/`iter()`/`serialize` can read the order
//~ as a fact instead of re-deriving (or, as they once did, re-sorting) it.
/// Return the canonical array index a JavaScript property key spells.
//~
//~ Only the canonical decimal spelling of a `u32` below `u32::MAX` is an
//~ array index, so `"01"`, `"+1"`, `"1.0"` and `"-1"` stay string keys.
fn smelt_canonical_array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 { return None; }
    if key.len() > 1 && key.starts_with('0') { return None; }
    if !key.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

/// A key of an erased JavaScript object or record.
//~
//~ Generated code only ever instantiates records with `String` keys; the
//~ trait exists so the shared ordering logic can ask a generic `K` whether
//~ it spells an array index without the containers hard-coding `String`,
//~ and so a lookup can hash a borrowed `str` key the same way the stored
//~ `String` was hashed. Both impls must agree on `smelt_key_hash` and on
//~ `smelt_key_fingerprint`, which is why both delegate to the shared
//~ `smelt_field_hash_bytes`/`smelt_field_fingerprint` functions.
pub trait SmeltPropertyKey {
    fn smelt_array_index(&self) -> Option<u32>;
    fn smelt_key_hash(&self) -> u64;
    //~ The stored-entry tie-break for this key; see `smelt_field_fingerprint`.
    fn smelt_key_fingerprint(&self) -> u32;
    //~ Whether this key is in the runtime's internal `__smelt` namespace.
    fn smelt_is_internal_key(&self) -> bool;
}

impl SmeltPropertyKey for str { fn smelt_array_index(&self) -> Option<u32> { smelt_canonical_array_index(self) } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(self.as_bytes()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(self.as_bytes()) } #[inline] fn smelt_is_internal_key(&self) -> bool { self.starts_with("__smelt") } }
impl SmeltPropertyKey for String { fn smelt_array_index(&self) -> Option<u32> { smelt_canonical_array_index(self) } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(self.as_bytes()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(self.as_bytes()) } #[inline] fn smelt_is_internal_key(&self) -> bool { self.starts_with("__smelt") } }
// @smelt:if js_string
//~ A JavaScript string key hashes its code units' bytes, which `JsString`
//~ and the borrowed `JsStr` share, so a `&JsStr` lookup finds a stored key.
impl SmeltPropertyKey for JsStr { fn smelt_array_index(&self) -> Option<u32> { if self.len() > 10 || !self.units().iter().all(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit)) { return None; } smelt_canonical_array_index(&self.to_string_lossy()) } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(self.byte_view()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(self.byte_view()) } #[inline] fn smelt_is_internal_key(&self) -> bool { self.eq_str_prefix("__smelt") } }
impl SmeltPropertyKey for JsString { fn smelt_array_index(&self) -> Option<u32> { (**self).smelt_array_index() } #[inline] fn smelt_key_hash(&self) -> u64 { (**self).smelt_key_hash() } #[inline] fn smelt_key_fingerprint(&self) -> u32 { (**self).smelt_key_fingerprint() } #[inline] fn smelt_is_internal_key(&self) -> bool { (**self).smelt_is_internal_key() } }
// @smelt:end
// @smelt:if js_key_order
// @smelt:else
//~ A Python dict keyed by `int` or `bool` is a record too. Neither spells a
//~ JavaScript array index, and both hash their bytes the way a string key does.
//= impl SmeltPropertyKey for i64 { fn smelt_array_index(&self) -> Option<u32> { None } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(&self.to_le_bytes()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(&self.to_le_bytes()) } #[inline] fn smelt_is_internal_key(&self) -> bool { false } }
//= impl SmeltPropertyKey for bool { fn smelt_array_index(&self) -> Option<u32> { None } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(&[u8::from(*self)]) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(&[u8::from(*self)]) } #[inline] fn smelt_is_internal_key(&self) -> bool { false } }
// @smelt:end

//~ In a Python program (`js_key_order` off) every key appends: a Python dict
//~ keeps plain insertion order.
/// Return the position a newly inserted key takes in a JavaScript own-key order.
//~
//~ `entries` keeps its array-index keys as a sorted leading run, so the slot
//~ is found with two binary searches: one for the end of that run, one for
//~ the ascending slot inside it. A non-index key appends. Appending keys in
//~ ascending index order therefore stays linear overall, never quadratic.
fn smelt_js_key_order_position<K: SmeltPropertyKey, V>(entries: &[SmeltFieldEntry<K, V>], key: &K) -> usize {
// @smelt:if js_key_order
    let Some(index) = key.smelt_array_index() else { return entries.len() };
    let indexed = entries.partition_point(|entry| entry.key.smelt_array_index().is_some());
    entries[..indexed].partition_point(|entry| entry.key.smelt_array_index().is_some_and(|existing| existing < index))
// @smelt:else
    //~ A Python program's dicts keep plain insertion order: a key that spells
    //~ an array index is not moved to the front, as JavaScript moves it.
//=     let _ = key;
//=     entries.len()
// @smelt:end
}

impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> SmeltRecord<K, V> {
// @smelt:if reference_origins
    fn new() -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(SmeltStoreCell::new(SmeltFieldStore::default())) } }
// @smelt:else
//=     fn new() -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::default())) } }
// @smelt:end
// @smelt:if reference_origins
    fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, store: ::std::rc::Rc::new(SmeltStoreCell::new(SmeltFieldStore::default())) }; record.extend(iter); record }
// @smelt:else
//=     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::default())) }; record.extend(iter); record }
// @smelt:end
    fn len(&self) -> usize { self.store.borrow().len() }
    fn contains_key<Q>(&self, key: &Q) -> bool where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized { self.store.borrow().contains_key(key) }
    fn insert(&self, key: K, value: V) -> Option<V> { self.store.borrow_mut().insert(key, value) }
    fn remove<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized { self.store.borrow_mut().remove(key) }
    fn get<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized, V: Clone { self.store.borrow().get(key).cloned() }
    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> where V: Clone { self.store.borrow().entries().iter().map(|entry| (entry.key.clone(), entry.value.clone())).collect::<Vec<_>>().into_iter() }
    fn keys(&self) -> ::std::vec::IntoIter<K> { self.store.borrow().entries().iter().map(|entry| entry.key.clone()).collect::<Vec<_>>().into_iter() }
    fn values(&self) -> ::std::vec::IntoIter<V> where V: Clone { self.store.borrow().entries().iter().map(|entry| entry.value.clone()).collect::<Vec<_>>().into_iter() }
    fn extend<I: IntoIterator<Item = (K, V)>>(&self, iter: I) { for (key, value) in iter { self.insert(key, value); } }
    fn clear(&self) { *self.store.borrow_mut() = SmeltFieldStore::default(); }
// @smelt:if js_key_order
// @smelt:else
    //~ Remove and return the last entry (Python `dict.popitem()`); only a
    //~ Python program can ask for it.
//=     fn pop_last(&self) -> Option<(K, V)> { let key = self.store.borrow().entries().last()?.key.clone(); let value = self.remove(&key)?; Some((key, value)) }
// @smelt:end
    //~ The default is a CLOSURE, not a value. A JavaScript accumulator loop
    //~ (`groupBy`, `countBy`, `uniqBy`) calls this once per element and the key
    //~ is already present for all but the first, so a by-value default built an
    //~ empty `SmeltList` — two heap allocations — for every element and threw it
    //~ away. Building it only on the absent path costs nothing.
    //~
    //~ The `SmeltRecord` twin of `SmeltJsMap::entry_or_insert`: borrow the
    //~ stored value for in-place mutation instead of copying it out and back.
    //~ `insert` already maintains the JavaScript own-key order, so routing the
    //~ absent-key case through it keeps `keys()`/`iter()` ordering identical to
    //~ the copy-back form this replaces. Takes `&self` because a record is a
    //~ reference value with interior mutability, exactly like `insert`.
    fn entry_or_insert(&self, key: K, default: impl FnOnce() -> V) -> ::std::cell::RefMut<'_, V> {
        let missing = !self.store.borrow().contains_key(&key);
        if missing { self.insert(key.clone(), default()); }
        ::std::cell::RefMut::map(self.store.borrow_mut(), move |store| store.get_mut(&key).expect("record entry just inserted"))
    }
}

impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> Default for SmeltRecord<K, V> {
    fn default() -> Self { Self::new() }
}

impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V, const N: usize> From<[(K, V); N]> for SmeltRecord<K, V> {
    fn from(values: [(K, V); N]) -> Self { values.into_iter().collect() }
}

impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> ::std::iter::FromIterator<(K, V)> for SmeltRecord<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { let record = Self::new(); record.extend(iter); record }
}

impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: Clone> IntoIterator for SmeltRecord<K, V> {
    type Item = (K, V);
    type IntoIter = ::std::vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V: PartialEq> PartialEq for SmeltRecord<K, V> {
    //~ Compared entry-by-entry and order-INSENSITIVELY, matching the `HashMap`
    //~ equality this replaces: JavaScript own-key order is observable through
    //~ enumeration, not through deep equality.
    fn eq(&self, other: &Self) -> bool { let left = self.store.borrow(); let right = other.store.borrow(); left.len() == right.len() && left.entries().iter().all(|entry| right.get(&entry.key).is_some_and(|found| *found == entry.value)) }
}

impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V: Eq> Eq for SmeltRecord<K, V> {}

impl<K, V> PartialEq<::std::collections::HashMap<K, V>> for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash, V: PartialEq {
    //~ Compared entry-by-entry rather than with `HashMap::eq`: the record's own
    //~ store is keyed by `SmeltFieldHasher` while the operand here is a stock
    //~ `HashMap` (`RandomState`), and `eq` requires both sides to share a hasher
    //~ type. Equality does not depend on the hasher, so this compares the
    //~ contents directly.
    fn eq(&self, other: &::std::collections::HashMap<K, V>) -> bool { let store = self.store.borrow(); store.len() == other.len() && store.entries().iter().all(|entry| other.get(&entry.key).is_some_and(|found| *found == entry.value)) }
}

// @smelt:item-end
