// `SmeltObject`: the erased JavaScript object.

// @smelt:item SmeltObject when unknown
#[derive(Debug)]
pub struct SmeltObject {
    id: usize,
// @smelt:if reference_origins
    store: ::std::rc::Rc<SmeltStoreCell<JsString, SmeltUnknown>>,
// @smelt:else
//=     store: ::std::rc::Rc<::std::cell::RefCell<SmeltFieldStore<JsString, SmeltUnknown>>>,
// @smelt:end
}

impl Clone for SmeltObject { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }
// @smelt:item-end

//~ Typed JavaScript records read source property names through these, with
//~ or without the erased object in the program.
// @smelt:item impl_SmeltRecord_JsString_V when record & js_string
impl<V> SmeltRecord<JsString, V> {
    //~ Whether the record has the key spelled by the runtime text `key`.
    #[allow(dead_code)]
    fn has_text_key(&self, key: &str) -> bool { key.with_js_key(|key| self.store.borrow().contains_key(key)) }
    //~ The value at the key spelled by the runtime text `key`.
    #[allow(dead_code)]
    fn get_text(&self, key: &str) -> Option<V> where V: Clone { key.with_js_key(|key| self.store.borrow().get(key).cloned()) }
}
// @smelt:item-end

// @smelt:item SmeltObjectKey when js_string
//~ A key an erased object is read by: a JavaScript string as it is, and the
//~ runtime's own `&str` keys (markers, spec property names) encoded on the
//~ stack when they are short, so a marker probe allocates nothing.
pub trait SmeltObjectKey {
    //~ Calls `read` with the key as a JavaScript string.
    fn with_js_key<R>(&self, read: impl FnOnce(&JsStr) -> R) -> R;
}

impl SmeltObjectKey for str {
    fn with_js_key<R>(&self, read: impl FnOnce(&JsStr) -> R) -> R {
        let mut buffer = [0u16; 64];
        if self.len() <= buffer.len() && self.is_ascii() {
            for (slot, byte) in buffer.iter_mut().zip(self.bytes()) { *slot = u16::from(byte); }
            return read(JsStr::from_units(&buffer[..self.len()]));
        }
        read(&JsString::from(self))
    }
}
impl SmeltObjectKey for String { fn with_js_key<R>(&self, read: impl FnOnce(&JsStr) -> R) -> R { self.as_str().with_js_key(read) } }
impl SmeltObjectKey for JsStr { fn with_js_key<R>(&self, read: impl FnOnce(&Self) -> R) -> R { read(self) } }
impl SmeltObjectKey for JsString { fn with_js_key<R>(&self, read: impl FnOnce(&JsStr) -> R) -> R { read(self) } }
impl<T: SmeltObjectKey + ?Sized> SmeltObjectKey for &T { fn with_js_key<R>(&self, read: impl FnOnce(&JsStr) -> R) -> R { (**self).with_js_key(read) } }
// @smelt:item-end

// @smelt:item impl_SmeltObject when unknown
impl SmeltObject {
    //~ A JavaScript object is a REFERENCE value, so recovering an erased one
    //~ at its record type must not copy it: `SmeltObject` and
    //~ `SmeltRecord<JsString, SmeltUnknown>` are the same representation (an id
    //~ plus a shared field store), so the recovery can hand back a second
    //~ handle on the SAME store. Rebuilding the entries instead detached the
    //~ recovered record from the value it came from, and every write through
    //~ it -- `Object.assign(erasedTarget, source)`, a property store on a
    //~ recovered object -- landed in a copy nothing else could see.
    //~ A second handle on this object's own field store.
    fn smelt_shared_record(&self) -> SmeltRecord<JsString, SmeltUnknown> { SmeltRecord { id: self.id, store: ::std::rc::Rc::clone(&self.store) } }
    //~ Build an erased object from entries in source order.
    //~
    //~ The entry sequence is ordered on purpose: JavaScript own-key order is a
    //~ property of how the object was written, and a `HashMap` argument would
    //~ have thrown that away before the constructor could see it. Duplicate
    //~ keys keep the first key's position and take the last value, as in JS.
    fn new<K: Into<JsString>>(entries: Vec<(K, SmeltUnknown)>) -> Self { Self::with_id(smelt_next_object_id(), entries) }
    //~ Build an erased object that keeps a source value's reference identity.
// @smelt:if reference_origins
    fn with_id<K: Into<JsString>>(id: usize, entries: Vec<(K, SmeltUnknown)>) -> Self { let object = Self { id, store: ::std::rc::Rc::new(SmeltStoreCell::new(SmeltFieldStore::with_capacity(entries.len()))) }; for (key, value) in entries { object.insert(key, value); } object }
// @smelt:else
//=     fn with_id<K: Into<JsString>>(id: usize, entries: Vec<(K, SmeltUnknown)>) -> Self { let object = Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::with_capacity(entries.len()))) }; for (key, value) in entries { object.insert(key, value); } object }
// @smelt:end
    fn from_unknown_record(record: SmeltRecord<JsString, SmeltUnknown>) -> Self { Self { id: record.id, store: record.store } }
    fn len(&self) -> usize { self.store.borrow().len() }
    #[inline]
    fn contains_key(&self, key: &(impl SmeltObjectKey + ?Sized)) -> bool { key.with_js_key(|key| self.store.borrow().contains_key(key)) }
    #[inline]
    fn get(&self, key: &(impl SmeltObjectKey + ?Sized)) -> Option<SmeltUnknown> { key.with_js_key(|key| self.store.borrow().get(key).cloned()) }
    //~ [`Self::get`] spelled like a record's text-keyed read.
    #[allow(dead_code)]
    fn get_text(&self, key: &str) -> Option<SmeltUnknown> { self.get(key) }
    fn insert(&self, key: impl Into<JsString>, value: SmeltUnknown) -> Option<SmeltUnknown> { self.store.borrow_mut().insert(key.into(), value) }
    fn remove(&self, key: &(impl SmeltObjectKey + ?Sized)) -> Option<SmeltUnknown> { key.with_js_key(|key| self.store.borrow_mut().remove(key)) }
    fn iter(&self) -> ::std::vec::IntoIter<(JsString, SmeltUnknown)> { self.store.borrow().entries().iter().map(|entry| (entry.key.clone(), entry.value.clone())).collect::<Vec<_>>().into_iter() }
    fn keys(&self) -> Vec<JsString> { self.store.borrow().entries().iter().map(|entry| entry.key.clone()).collect() }
    fn values(&self) -> Vec<SmeltUnknown> { self.store.borrow().entries().iter().map(|entry| entry.value.clone()).collect() }
// @smelt:if reference_origins
    //~ Record the live typed value this erased object is a view of.
    //~
    //~ Called by a reference record's erasure with a clone of its handle, so
    //~ narrowing the erased object back hands out the SAME instance rather than
    //~ a copy rebuilt from the field snapshot.
    #[allow(dead_code)]
    fn smelt_with_origin<T: 'static>(self, origin: T) -> Self { *self.store.origin_mut() = Some(Box::new(origin)); self }
    //~ The live typed value this erased object is a view of, when it is a `T`.
    #[allow(dead_code)]
    fn smelt_origin<T: Clone + 'static>(&self) -> Option<T> { self.store.origin().as_ref().and_then(|origin| origin.downcast_ref::<T>()).cloned() }
    //~ Build an erased object whose entries `build` produces on first use (see `SmeltStoreCell`).
    #[allow(dead_code)]
    fn lazy_with_id<K: Into<JsString>>(id: usize, build: impl FnOnce() -> Vec<(K, SmeltUnknown)> + 'static) -> Self { Self { id, store: ::std::rc::Rc::new(SmeltStoreCell::lazy(SmeltFieldStore::default(), Box::new(move || build().into_iter().map(|(key, value)| (key.into(), value)).collect()), |store, entries| { for (key, value) in entries { store.insert(key, value); } })) } }
// @smelt:end
}
// @smelt:if reference_origins

// @smelt:end
// @smelt:item-end

// @smelt:item impl_SmeltRecord_K_V when reference_origins & unknown
impl<K, V> SmeltRecord<K, V> {
    //~ The live typed value this record's store is an erased view of, when it is a `T`.
    #[allow(dead_code)]
    fn smelt_origin<T: Clone + 'static>(&self) -> Option<T> { self.store.origin().as_ref().and_then(|origin| origin.downcast_ref::<T>()).cloned() }
}
// @smelt:item-end

// @smelt:item smelt_object_has_host_marker when unknown
/// Return whether an erased object key is visible to JavaScript `for...in` iteration.
fn smelt_object_has_host_marker(object: &SmeltObject) -> bool { object.store.borrow().has_internal_keys() && ["__smelt_abortcontroller", "__smelt_abortsignal", "__smelt_arraybuffer", "__smelt_sharedarraybuffer", "__smelt_int8array", "__smelt_uint8array", "__smelt_uint8clampedarray", "__smelt_int16array", "__smelt_uint16array", "__smelt_int32array", "__smelt_uint32array", "__smelt_float32array", "__smelt_float64array", "__smelt_bigint64array", "__smelt_biguint64array", "__smelt_buffer", "__smelt_dataview", "__smelt_weakmap", "__smelt_weakset", "__smelt_file", "__smelt_blob", "__smelt_headers", "__smelt_urlsearchparams", "__smelt_url", "__smelt_crypto_key", "__smelt_request", "__smelt_response", "__smelt_textencoder", "__smelt_textdecoder", "__smelt_eventemitter", "__smelt_httpserver", "__smelt_incomingmessage", "__smelt_serverresponse", "__smelt_formdata", "__smelt_databasesync", "__smelt_statementsync", "__smelt_readablestream", "__smelt_domexception", "__smelt_intl_collator", "__smelt_intl_displaynames", "__smelt_intl_durationformat", "__smelt_intl_listformat", "__smelt_intl_locale", "__smelt_intl_numberformat", "__smelt_intl_pluralrules", "__smelt_intl_segmenter", "__smelt_number", "__smelt_boolean", "__smelt_string", "__smelt_symbol", "__smelt_builtin_namespace", "__smelt_global_object"].iter().any(|marker| object.contains_key(marker)) }
fn smelt_record_has_host_marker<V>(record: &SmeltRecord<JsString, V>) -> bool { record.store.borrow().has_internal_keys() && ["__smelt_abortcontroller", "__smelt_abortsignal", "__smelt_arraybuffer", "__smelt_sharedarraybuffer", "__smelt_int8array", "__smelt_uint8array", "__smelt_uint8clampedarray", "__smelt_int16array", "__smelt_uint16array", "__smelt_int32array", "__smelt_uint32array", "__smelt_float32array", "__smelt_float64array", "__smelt_bigint64array", "__smelt_biguint64array", "__smelt_buffer", "__smelt_dataview", "__smelt_weakmap", "__smelt_weakset", "__smelt_file", "__smelt_blob", "__smelt_headers", "__smelt_urlsearchparams", "__smelt_url", "__smelt_crypto_key", "__smelt_request", "__smelt_response", "__smelt_textencoder", "__smelt_textdecoder", "__smelt_eventemitter", "__smelt_httpserver", "__smelt_incomingmessage", "__smelt_serverresponse", "__smelt_formdata", "__smelt_databasesync", "__smelt_statementsync", "__smelt_readablestream", "__smelt_domexception", "__smelt_intl_collator", "__smelt_intl_displaynames", "__smelt_intl_durationformat", "__smelt_intl_listformat", "__smelt_intl_locale", "__smelt_intl_numberformat", "__smelt_intl_pluralrules", "__smelt_intl_segmenter", "__smelt_number", "__smelt_boolean", "__smelt_string", "__smelt_symbol", "__smelt_builtin_namespace", "__smelt_global_object"].iter().any(|marker| record.has_text_key(marker)) }
// @smelt:item-end

// @smelt:item impl_PartialEq_for_SmeltObject when unknown
impl PartialEq for SmeltObject { fn eq(&self, other: &Self) -> bool { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_eq(self, other, &mut smelt_seen) } }
impl Eq for SmeltObject {}
impl ::std::hash::Hash for SmeltObject { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_hash(self, state, &mut smelt_seen); } }
impl IntoIterator for SmeltObject { type Item = (JsString, SmeltUnknown); type IntoIter = ::std::vec::IntoIter<(JsString, SmeltUnknown)>; fn into_iter(self) -> Self::IntoIter { self.iter() } }

// @smelt:item-end
