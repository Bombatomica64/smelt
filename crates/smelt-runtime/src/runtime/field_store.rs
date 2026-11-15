// The property store behind every erased object: the field hash and the ordered entry store.

// @smelt:item use_hash when record
use ::std::hash::Hash;

//~ JavaScript object property lookup goes through these maps on EVERY field
//~ read, and `std`'s default `RandomState` is SipHash-1-3: DoS-resistant, and
//~ priced accordingly. Profiling es-toolkit's `partition` under callgrind put
//~ 16.2% of the whole benchmark in `BuildHasher::hash_one` plus
//~ `sip::Hasher::write` — more than six times the 2.7% spent in the transpiled
//~ function itself. Property keys are short strings from the program's own
//~ source, not attacker-controlled input reaching a server, so the collision
//~ resistance buys nothing here.
//~
//~ This is the FxHash construction rustc uses on its own symbol tables: one
//~ multiply-rotate per 8 bytes, no keying. It is also DETERMINISTIC, which
//~ matters beyond speed — `RandomState` seeds per process, so anything that
//~ observes map iteration order (and some erased-value paths do) varies run to
//~ run. That is the suspected mechanism behind the intermittent remeda
//~ `pipe` failure recorded in smeltlist-shared-buffer.md (in git history); a fixed
//~ hasher makes such a failure reproduce every run instead of one in six.
#[derive(Default, Clone, Copy)]
pub struct SmeltFieldHasher(u64);

impl ::std::hash::Hasher for SmeltFieldHasher {
    //~ FxHash's accumulator has weak avalanche in its high bits, and hashbrown
    //~ takes its control byte from the TOP 7 bits — so a raw `self.0` collides
    //~ badly on structured keys. That is not hypothetical: routing the
    //~ Set/Map member index (`SmeltFieldMap<u64, ..>`) through the unfinalized
    //~ form cost es-toolkit `unique` 25.4M -> 30.9M instructions, because every
    //~ extra bucket collision pays a `same_member` comparison and each of those
    //~ erases a value. The splitmix64 finalizer is six cheap ALU ops and
    //~ restores full avalanche, which brought that case back and past its
    //~ starting point.
    fn finish(&self) -> u64 {
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }
    fn write(&mut self, bytes: &[u8]) {
        const SMELT_FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        let mut hash = self.0;
        let mut rest = bytes;
        while rest.len() >= 8 {
            let (head, tail) = rest.split_at(8);
            let word = u64::from_le_bytes(head.try_into().unwrap_or([0; 8]));
            hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);
            rest = tail;
        }
        if !rest.is_empty() {
            let mut buf = [0_u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            let word = u64::from_le_bytes(buf);
            hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);
        }
        self.0 = hash;
    }
}

/// The map behind a JavaScript object/record, keyed by property name.
pub type SmeltFieldMap<K, V> = ::std::collections::HashMap<K, V, ::std::hash::BuildHasherDefault<SmeltFieldHasher>>;

//~ The property store shared by `SmeltRecord` and `SmeltObject`.
//~
//~ The representation it replaces was a `HashMap<K, V>` plus a separate
//~ `Vec<K>` holding JavaScript own-key order, each behind its own
//~ `Rc<RefCell<..>>`: four heap allocations per object, two stored copies of
//~ every key, and a full hash-table probe (hash, control-byte scan, bucket
//~ compare) on every property read. Callgrind put `SmeltObject::get` at
//~ 17.4% of es-toolkit's `partition` on that shape -- the largest single
//~ cost centre in the case -- and object CONSTRUCTION at another large
//~ slice on top.
//~
//~ JavaScript objects are overwhelmingly SMALL: a handful of named fields.
//~ For those a hash table is the wrong structure, and so is hashing at all.
//~ This keeps the key/value pairs in ONE `Vec` in own-key order and, below
//~ `SMELT_FIELD_SCAN_LIMIT` entries, resolves a property by scanning it and
//~ comparing keys directly. `str` equality compares lengths first, so the
//~ handful of differently sized names in a record are rejected without
//~ touching their bytes -- cheaper than the ~40 instructions it takes to
//~ hash even a four-byte key. Objects used as dictionaries do need O(1)
//~ lookup, so a `hash -> first position` index is built and maintained once
//~ a store grows past that limit; only then is a key ever hashed.
//~
//~ Nothing here keys off a field name, a library, or a source spelling: it
//~ is the representation every erased object and every record uses.
/// Hash a property key's bytes into a property store's key space.
//~
//~ Only the dictionary-sized path uses this; a record-sized store scans.
//~ Deliberately not routed through `Hash`/`SmeltFieldHasher`: `<str as Hash>::hash`
//~ makes TWO `Hasher::write` calls -- the bytes, then a `0xff` terminator that
//~ disambiguates concatenations -- and each rebuilds an 8-byte staging buffer.
//~ Property names are short, so this folds the length in directly (which serves
//~ the same disambiguating purpose) and takes the <=8-byte case, nearly every
//~ property name, as a single load and mix. No avalanche finalizer: the value
//~ only ever enters the `index` map, which re-hashes it through
//~ `SmeltFieldHasher`, and that finalizes.
#[inline]
fn smelt_field_hash_bytes(bytes: &[u8]) -> u64 {
    const SMELT_FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    let mut hash = bytes.len() as u64;
    let mut rest = bytes;
    while rest.len() >= 8 {
        let (head, tail) = rest.split_at(8);
        let word = u64::from_le_bytes(head.try_into().unwrap_or([0; 8]));
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);
        rest = tail;
    }
    if !rest.is_empty() {
        let mut buf = [0_u8; 8];
        buf[..rest.len()].copy_from_slice(rest);
        let word = u64::from_le_bytes(buf);
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);
    }
    hash
}

//~ The same-length tie-break the scan was missing.
//~
//~ A record-sized store is resolved by scanning `entries` and comparing
//~ keys. `str` equality rejects a DIFFERENT-length key for free, but two
//~ keys of the SAME length go all the way into `memcmp` -- and a real
//~ record is full of those. On the es-toolkit benchmark records
//~ (`id`/`group`/`value`/`flag`) a read of `"value"` paid a full failing
//~ `memcmp` against `"group"` before reaching its own entry, which is why
//~ callgrind put `__memcmp_avx2_movbe` at 15.3% of `sum_by` next to
//~ `position`'s 21.8%.
//~
//~ This is NOT the `smelt_key_hash` the docstring below rules out: it is
//~ not a hash function, it is three loads and two shifts, and it is stored
//~ ALONGSIDE the key so a mismatching entry is rejected from the entries
//~ vector itself, without dereferencing the key's heap buffer at all.
//~ Length goes in the high half so it still subsumes the free
//~ length-rejection the scan already had.
/// A cheap same-length tie-break for a property key: length, first byte, last byte.
//~
//~ Equal keys MUST produce equal fingerprints, so every `SmeltPropertyKey`
//~ impl delegates here rather than computing its own -- a `String` entry and
//~ the `&str` a lookup borrows against it must agree or the lookup would
//~ report a present key absent. Unequal keys may collide; a fingerprint hit
//~ is always confirmed with full key equality.
#[inline]
fn smelt_field_fingerprint(bytes: &[u8]) -> u32 {
    let first = bytes.first().copied().unwrap_or(0) as u32;
    let last = bytes.last().copied().unwrap_or(0) as u32;
    ((bytes.len() as u32) << 16) | (first << 8) | last
}

/// One property of a store: its key, its value, and the key's fingerprint.
//~
//~ `fingerprint` is derived from `key` at insert time and is never updated
//~ independently -- the key of an existing entry is never rewritten, only its
//~ value is -- so the two cannot drift apart.
#[derive(Debug)]
pub struct SmeltFieldEntry<K, V> {
    fingerprint: u32,
    key: K,
    value: V,
}

/// Entries a property store scans linearly before it builds a hash index.
//~
//~ Set where an object stops looking like a record and starts looking like a
//~ dictionary: below it a length-first key scan beats hashing plus a probe,
//~ above it dictionary-shaped objects need their O(1) lookup back.
const SMELT_FIELD_SCAN_LIMIT: usize = 12;

/// The ordered property store behind a JavaScript object or record.
//~
//~ `entries` is held in JavaScript own-key order (array-index keys first, in
//~ ascending numeric order, then the remaining string keys in insertion
//~ order), so `keys()`/`iter()` read the order as a fact rather than
//~ re-deriving it. `index` maps a key hash to the FIRST entry position
//~ carrying it and exists ONLY while the store is larger than
//~ `SMELT_FIELD_SCAN_LIMIT` — a small store never hashes a key at all.
#[derive(Debug)]
pub struct SmeltFieldStore<K, V> {
    entries: Vec<SmeltFieldEntry<K, V>>,
    index: Option<SmeltFieldMap<u64, usize>>,
    //~ How many keys live in the runtime's internal `__smelt` namespace.
    internal: u32,
// @smelt:if reference_origins
    //~ The live typed value this store is an erased view of, if any (see `smelt_with_origin`).
    origin: Option<Box<dyn ::std::any::Any>>,
// @smelt:end
}

impl<K, V> SmeltFieldStore<K, V> {
    //~ Build an empty store sized for `capacity` entries.
    fn with_capacity(capacity: usize) -> Self { Self { entries: Vec::with_capacity(capacity), index: None, internal: 0/*@if reference_origins*/, origin: None/*@end*/ } }
    #[inline]
    fn len(&self) -> usize { self.entries.len() }
    //~ Borrow the key/value pairs in JavaScript own-key order.
    #[inline]
    fn entries(&self) -> &[SmeltFieldEntry<K, V>] { &self.entries }
    //~ Whether any key is in the internal `__smelt` namespace (markers, slots).
    //~
    //~ Every marker probe (`smelt_object_has_host_marker`, the byte-buffer
    //~ marker, `for...in`'s regexp/error/arguments checks) asks about keys in
    //~ that namespace, and almost every object has none: a store that says so
    //~ answers all of them without scanning its entries once per marker.
    #[inline]
    fn has_internal_keys(&self) -> bool { self.internal != 0 }
}

impl<K, V> Default for SmeltFieldStore<K, V> {
    fn default() -> Self { Self { entries: Vec::new(), index: None, internal: 0/*@if reference_origins*/, origin: None/*@end*/ } }
}

impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V> SmeltFieldStore<K, V> {
    //~ Rebuild the hash index, or drop it when the store is small enough to scan.
    fn reindex(&mut self) {
        if self.entries.len() <= SMELT_FIELD_SCAN_LIMIT { self.index = None; return; }
        let mut index: SmeltFieldMap<u64, usize> = SmeltFieldMap::with_capacity_and_hasher(self.entries.len(), ::std::default::Default::default());
        for (position, entry) in self.entries.iter().enumerate() { index.entry(entry.key.smelt_key_hash()).or_insert(position); }
        self.index = Some(index);
    }
    //~ Return the entry position holding `key`, or `None`.
    //~
    //~ A small store is SCANNED, comparing keys directly: `str` equality checks
    //~ the length first, so a handful of differently sized property names are
    //~ rejected without touching their bytes, and the key is never hashed. That
    //~ is the whole point — hashing a short key costs more than the scan it
    //~ would save. Two keys of the SAME length are the case that costs, so
    //~ each entry carries a `smelt_field_fingerprint` — length, first byte,
    //~ last byte — that is compared first and rejects such a key out of the
    //~ entries vector, without dereferencing its bytes. A fingerprint hit is
    //~ still confirmed with full key equality, so the answer is unchanged.
    //~ Past `SMELT_FIELD_SCAN_LIMIT` entries the store is a
    //~ dictionary rather than a record, and the hash index resolves the
    //~ position directly; a hash collision between two DISTINCT keys is
    //~ vanishingly unlikely but not impossible, so a failed key confirmation
    //~ falls back to the scan rather than reporting the key absent.
    #[inline]
    fn position<Q>(&self, key: &Q) -> Option<usize> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {
        let fingerprint = key.smelt_key_fingerprint();
        if let Some(index) = self.index.as_ref() {
            let start = *index.get(&key.smelt_key_hash())?;
            let entry = &self.entries[start];
            if entry.fingerprint == fingerprint && entry.key.borrow() == key { return Some(start); }
        }
        self.entries.iter().position(|entry| entry.fingerprint == fingerprint && entry.key.borrow() == key)
    }
    #[inline]
    fn get<Q>(&self, key: &Q) -> Option<&V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {
        self.position(key).map(|position| &self.entries[position].value)
    }
    fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {
        let position = self.position(key)?;
        Some(&mut self.entries[position].value)
    }
    #[inline]
    fn contains_key<Q>(&self, key: &Q) -> bool where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {
        self.position(key).is_some()
    }
    //~ Remove `key`, returning its value; later positions shift, so reindex.
    fn remove<Q>(&mut self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {
        let position = self.position(key)?;
        let entry = self.entries.remove(position);
        if entry.key.smelt_is_internal_key() { self.internal -= 1; }
        let value = entry.value;
        self.reindex();
        Some(value)
    }
    //~ Insert or overwrite `key`, keeping JavaScript own-key order.
    //~
    //~ A new key almost always appends, which keeps the index valid with a
    //~ single added mapping; only an array-index key landing before an existing
    //~ entry shifts later positions and forces a rebuild.
    fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(position) = self.position(&key) { return Some(::std::mem::replace(&mut self.entries[position].value, value)); }
        if key.smelt_is_internal_key() { self.internal += 1; }
        let position = smelt_js_key_order_position(&self.entries, &key);
        if position == self.entries.len() {
            let hash = if self.index.is_some() { key.smelt_key_hash() } else { 0 };
            let fingerprint = key.smelt_key_fingerprint();
            self.entries.push(SmeltFieldEntry { fingerprint, key, value });
            match self.index.as_mut() {
                Some(index) => { index.entry(hash).or_insert(position); }
                None => self.reindex(),
            }
        } else {
            let fingerprint = key.smelt_key_fingerprint();
            self.entries.insert(position, SmeltFieldEntry { fingerprint, key, value });
            self.reindex();
        }
        None
    }
}

// @smelt:item-end

// @smelt:item SmeltPendingFill when reference_origins & record
/// A snapshot an erased view builds the first time its store is read.
struct SmeltPendingFill<K, V> {
    build: Box<dyn FnOnce() -> Vec<(K, V)>>,
    insert: fn(&mut SmeltFieldStore<K, V>, Vec<(K, V)>),
}

// @smelt:if live_views
/// The typed record a store is a live view of, with this view's conversions.
//~
//~ `object` speaks JavaScript property names and erased values, which is the
//~ one vocabulary every view of a record shares; `key`, `wrap` and `unwrap`
//~ turn them into this store's key and value types and back (an erased view's
//~ are the identity, a typed dictionary view's convert each value). `same`
//~ says whether two values of the view are the same reference, and `apply`
//~ installs a fresh entry list (`SmeltFieldStore::resync`): both are stored
//~ as functions so the cell's own methods need no bound on `K` or `V`.
struct SmeltLiveLink<K, V> {
    object: ::std::rc::Rc<dyn SmeltLiveObject>,
    key: fn(JsString) -> K,
    wrap: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> V>,
    unwrap: ::std::rc::Rc<dyn Fn(&V) -> SmeltUnknown>,
    same: fn(&V, &V) -> bool,
    apply: fn(&mut SmeltFieldStore<K, V>, Vec<(K, V)>, fn(&V, &V) -> bool),
}

/// Who answers a read of one key of a store.
enum SmeltLiveRead<V> {
    //~ The store itself: it is not a live view, or the key is the runtime's own.
    Store,
    //~ The record the store is a live view of: its value, or no such property.
    Record(Option<V>),
}

impl<V> SmeltLiveRead<V> {
    //~ The record's answer, or the store's own when the read is the store's.
    #[inline]
    fn or_store(self, store: impl FnOnce() -> Option<V>) -> Option<V> { match self { Self::Record(found) => found, Self::Store => store() } }
}

// @smelt:end
/// The `RefCell` around a field store, building a pending snapshot on first use.
//~
//~ See `lazy_view` in the code generator: `borrow`/`borrow_mut` are the
//~ `RefCell` ones after materializing; `origin`/`origin_mut` skip it and reach
//~ nothing but the `origin` slot.
pub struct SmeltStoreCell<K, V> {
    cell: ::std::cell::RefCell<SmeltFieldStore<K, V>>,
    pending: ::std::cell::Cell<Option<SmeltPendingFill<K, V>>>,
// @smelt:if live_views
    //~ The typed record this store is a live view of, if any (see `live_view.rs`).
    live: ::std::cell::RefCell<Option<SmeltLiveLink<K, V>>>,
// @smelt:end
}

#[allow(dead_code)]
impl<K, V> SmeltStoreCell<K, V> {
    //~ A cell over an already-populated store.
    #[inline]
    fn new(store: SmeltFieldStore<K, V>) -> Self { Self { cell: ::std::cell::RefCell::new(store), pending: ::std::cell::Cell::new(None)/*@if live_views*/, live: ::std::cell::RefCell::new(None)/*@end*/ } }
    //~ A cell whose entries `build` produces on the first read or write.
    fn lazy(store: SmeltFieldStore<K, V>, build: Box<dyn FnOnce() -> Vec<(K, V)>>, insert: fn(&mut SmeltFieldStore<K, V>, Vec<(K, V)>)) -> Self {
        Self { cell: ::std::cell::RefCell::new(store), pending: ::std::cell::Cell::new(Some(SmeltPendingFill { build, insert }))/*@if live_views*/, live: ::std::cell::RefCell::new(None)/*@end*/ }
    }
    //~ Build the pending snapshot, if any, into the store.
    #[inline]
    fn materialize(&self) {
        if let Some(fill) = self.pending.take() {
            let entries = (fill.build)();
            (fill.insert)(&mut self.cell.borrow_mut(), entries);
        }
    }
    //~ Borrow the store, building a pending snapshot first.
    //~
    //~ The store of a LIVE view (`live_view.rs`) is a cache of the record it
    //~ stands for, and borrowing does not refresh it: a read of one data key
    //~ asks the record (`live_get`), a read of the runtime's own `__smelt`
    //~ keys needs no refresh, and every whole-store read of data (keys,
    //~ iteration, length, equality, JSON, inspection) calls `sync` first.
    #[inline]
    fn borrow(&self) -> ::std::cell::Ref<'_, SmeltFieldStore<K, V>> { self.materialize(); self.cell.borrow() }
    //~ Mutably borrow the store, building a pending snapshot first. A write to
    //~ a data key of a live view does NOT go through here alone: `SmeltRecord`
    //~ and `SmeltObject` hand it to the record first (`live_set`/`live_remove`).
    #[inline]
    fn borrow_mut(&self) -> ::std::cell::RefMut<'_, SmeltFieldStore<K, V>> { self.materialize(); self.cell.borrow_mut() }
// @smelt:if live_views
    //~ Whether this store is a live view of a typed record.
    #[inline]
    fn is_live(&self) -> bool { self.live.borrow().is_some() }
    //~ Refresh a live view's data entries from its record, before a
    //~ whole-store read; nothing to do for any other store. A store already
    //~ borrowed was refreshed by that borrower and is left as it is.
    #[inline]
    fn sync(&self) {
        if self.live.borrow().is_some() { self.sync_live(); }
    }
    fn sync_live(&self) {
        let Some((object, key, wrap, same, apply)) = self.live.borrow().as_ref().map(|link| (::std::rc::Rc::clone(&link.object), link.key, ::std::rc::Rc::clone(&link.wrap), link.same, link.apply)) else { return };
        if self.cell.try_borrow_mut().is_err() { return; }
        let entries = smelt_live_entries(&object).into_iter().map(|(name, value)| (key(name), wrap(value))).collect::<Vec<_>>();
        if let Ok(mut store) = self.cell.try_borrow_mut() { apply(&mut store, entries, same); }
    }
// @smelt:end
    //~ The origin slot, pending snapshot left unbuilt.
    #[inline]
    fn origin(&self) -> ::std::cell::Ref<'_, Option<Box<dyn ::std::any::Any>>> { ::std::cell::Ref::map(self.cell.borrow(), |store| &store.origin) }
    //~ The origin slot, mutably, pending snapshot left unbuilt.
    #[inline]
    fn origin_mut(&self) -> ::std::cell::RefMut<'_, Option<Box<dyn ::std::any::Any>>> { ::std::cell::RefMut::map(self.cell.borrow_mut(), |store| &mut store.origin) }
}
// @smelt:if live_views

//~ Reads and writes of ONE data key of a live view go to the record itself,
//~ so neither needs the whole view refreshed first.
#[allow(dead_code)]
impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V> SmeltStoreCell<K, V> {
    //~ The record behind this view and its key's JavaScript spelling, when
    //~ this store is a live view and `key` is not one of the runtime's own.
    #[inline]
    fn live_target<'a, Q: SmeltPropertyKey + ?Sized>(&self, key: &'a Q) -> Option<(::std::rc::Rc<dyn SmeltLiveObject>, &'a JsStr)> {
        let live = self.live.borrow();
        let link = live.as_ref()?;
        let name = key.smelt_live_key()?;
        //~ A key in the runtime's namespace is the store's own (a marker, a
        //~ prototype slot) unless the record declares it: a symbol-keyed
        //~ property is stored under such a key and is the record's field.
        if name.smelt_is_internal_key() && !link.object.smelt_field_names().iter().any(|declared| *name == **declared) { return None; }
        Some((::std::rc::Rc::clone(&link.object), name))
    }
    //~ Read a data key of a live view from its record: `Store` when this
    //~ store is not a live view or the key is the runtime's own, else the
    //~ record's answer (`None` when it has no such property). A value that is
    //~ the same reference as the cached one answers with the cached handle, so
    //~ an erased array or object read twice through one view is one value, as
    //~ it was while the view was a snapshot.
    fn live_get<Q>(&self, key: &Q) -> SmeltLiveRead<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized, V: Clone {
        let Some((object, name)) = self.live_target(key) else { return SmeltLiveRead::Store };
        let Some(fresh) = smelt_live_get(&object, name) else { return SmeltLiveRead::Record(None) };
        let Some((wrap, same)) = self.live.borrow().as_ref().map(|link| (::std::rc::Rc::clone(&link.wrap), link.same)) else { return SmeltLiveRead::Store };
        let fresh = wrap(fresh);
        if let Ok(store) = self.cell.try_borrow() && store.len() != 0 && let Some(cached) = store.get(key) && same(cached, &fresh) { return SmeltLiveRead::Record(Some(cached.clone())); }
        SmeltLiveRead::Record(Some(fresh))
    }
    //~ Whether the record behind a live view has the data key `key`; `None`
    //~ as for `live_get`.
    fn live_has<Q>(&self, key: &Q) -> Option<bool> where Q: SmeltPropertyKey + ?Sized {
        let (object, name) = self.live_target(key)?;
        Some(smelt_live_get(&object, name).is_some())
    }
    //~ Write a data key of a live view into its record. `false` when this
    //~ store is not a live view or the key is the runtime's own: the write
    //~ is then the store's alone.
    fn live_set(&self, key: &K, value: &V) -> bool {
        let Some((object, name)) = self.live_target(key) else { return false };
        let Some(unwrap) = self.live.borrow().as_ref().map(|link| ::std::rc::Rc::clone(&link.unwrap)) else { return false };
        smelt_live_set(&object, name, unwrap(value));
        true
    }
    //~ Delete a data key of a live view from its record; `false` as for `live_set`.
    fn live_remove<Q>(&self, key: &Q) -> bool where Q: SmeltPropertyKey + ?Sized {
        let Some((object, name)) = self.live_target(key) else { return false };
        smelt_live_remove(&object, name);
        true
    }
}

impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V> SmeltFieldStore<K, V> {
    //~ Replace this store's data entries with `fresh`, in `fresh`'s order,
    //~ keeping the runtime's own `__smelt` entries. A cached value that is
    //~ the same reference as its fresh one (`same`) is kept, for the reason
    //~ `SmeltStoreCell::live_get` gives.
    fn resync(&mut self, fresh: Vec<(K, V)>, same: fn(&V, &V) -> bool) {
        //~ The usual refresh changes values and no key: update in place.
        if self.entries.len() == fresh.len() && self.entries.iter().zip(&fresh).all(|(entry, (key, _))| entry.key == *key) {
            for (entry, (_, value)) in self.entries.iter_mut().zip(fresh) { if !same(&entry.value, &value) { entry.value = value; } }
            return;
        }
        let mut cached = ::std::mem::take(&mut self.entries).into_iter().map(Some).collect::<Vec<_>>();
        self.index = None;
        self.internal = 0;
        for (key, value) in fresh {
            let kept = cached.iter_mut().find(|slot| slot.as_ref().is_some_and(|entry| entry.key == key)).and_then(Option::take);
            let value = match kept { Some(entry) if same(&entry.value, &value) => entry.value, _ => value };
            self.insert(key, value);
        }
        for entry in cached.into_iter().flatten() { if entry.key.smelt_is_internal_key() { self.insert(entry.key, entry.value); } }
    }
}
// @smelt:end

impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltStoreCell<K, V> {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.borrow().fmt(formatter) }
}

// @smelt:item-end
