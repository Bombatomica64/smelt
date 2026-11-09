// `SmeltJsMap`: JavaScript `Map` with SameValueZero keys and insertion order.

// JS `Map` container. Carries a stable object `id` so that the identity a
// `Map` value has in JavaScript survives the erasure round-trip: when the
// map crosses a dynamic boundary (`IntoSmeltUnknown`) it becomes a
// marker-bearing `SmeltUnknown::Object` stamped with `__smelt_map` and its
// `id`, and un-erasing (`SmeltFromUnknown`) restores that same `id`. The
// marker is what lets `smelt_object_to_string_tag` report `[object Map]`
// and the `isMap`/`isEqualWith` runtime probes recognize the value once it
// is erased — a plain `SmeltUnknown::Object` (a JS object literal) cannot
// carry that identity, which is why the marker boundary exists.
// A JavaScript `Map` is a REFERENCE value: every binding that names one
// Map object shares a single backing store, so `otherName.set(k, v)` is
// observable through every alias, and passing a Map into a function hands
// over the same object (not a copy). The backing `entries` therefore live
// behind a shared `Rc<RefCell<..>>`: `#[derive(Clone)]` produces an ALIAS
// (bumps the refcount, copies the stable `id`) rather than a deep copy, so
// a `.clone()` inserted by codegen when a Map flows through an expression
// or a recursive call still mutates the one shared store. Value-copy
// semantics here silently drops writes — e.g. `isEqualWith`'s cycle guard
// (`stack.set(a, b)` before recursing) never persists, so circular inputs
// recurse forever and abort. A genuinely independent Map is only produced
// by an explicit JS construction (`new Map(other)`), which lowers through
// `From`/`from_iter`/`new` and allocates a fresh store. Keys compare by
// SameValueZero via `SmeltJsKeyEq::same_js_key` (objects/arrays/functions
// by reference identity, primitives by value, `NaN` matches `NaN`).
// `Clone` is the alias-producing derive described above (it bumps the `Rc`
// refcount and copies the stable `id`); `Debug` is hand-written below so the
// store and its index stay invisible and the rendering keeps reading
// `SmeltJsMap { id: 1, entries: [..] }`.
// @smelt:item SmeltJsMap when unknown
#[derive(Clone)]
pub struct SmeltJsMap<K, V> {
    id: usize,
    store: ::std::rc::Rc<::std::cell::RefCell<SmeltJsMapStore<K, V>>>,
}

//~ The entries of a `SmeltJsMap` plus the hash index over their keys.
//~
//~ A lookup does NOT scan `entries`: every key is also recorded in a
//~ `SmeltJsSlotIndex` under `SmeltJsKeyEq::js_key_hash`, whose contract is that
//~ two `same_js_key`-equal keys always hash the same, so the index only narrows
//~ which slots a comparison has to look at and the surviving candidates are
//~ still confirmed with `same_js_key`. A hash collision therefore costs a
//~ comparison and never changes an answer. Without it `contains_key`, `get`,
//~ `insert` and `remove` were all linear, so building an n-entry Map — what
//~ `groupBy`/`countBy`/`uniqBy`/memoization caches do on every call — cost
//~ O(n^2) key comparisons.
//~
//~ The index lives INSIDE the `RefCell` alongside the entries because a JS Map
//~ is a reference value: a write through any alias must be seen by every other
//~ alias, and an index kept outside the shared store would go stale the moment
//~ another handle inserted. `entries` stays a plain insertion-ordered `Vec`, so
//~ iteration order, `IntoIterator`, and the erasure adapters are unchanged.
#[derive(Clone, Debug)]
struct SmeltJsMapStore<K, V> {
    entries: Vec<(K, V)>,
    index: SmeltJsSlotIndex,
}

impl<K, V> SmeltJsMapStore<K, V> {
    fn new() -> Self { Self { entries: Vec::new(), index: SmeltJsSlotIndex::new() } }
}

impl<K: SmeltJsKeyEq, V> SmeltJsMapStore<K, V> {
    //~ Build the store of `new Map(entries)`: the entries are `set` in order,
    //~ so a key seen again keeps its FIRST position and takes its LAST value,
    //~ as JavaScript's Map constructor does. Each entry costs one hash. It used
    //~ to keep duplicate keys as separate slots: `new Map([["p", 1], ["p", 3]])`
    //~ answered `size` 2 and `get("p")` 1.
    fn from_entries(entries: Vec<(K, V)>) -> Self {
        let mut store = Self { entries: Vec::with_capacity(entries.len()), index: SmeltJsSlotIndex::new() };
        for (key, value) in entries {
            let hash = key.js_key_hash();
            if let Some(slot) = store.position(&key, hash) {
                store.entries[slot].1 = value;
            } else {
                store.index.remember(store.entries.len(), hash);
                store.entries.push((key, value));
            }
        }
        store
    }
    //~ Slot of `key`, comparing only the slots that share `hash` — every
    //~ `same_js_key`-equal key is guaranteed to be one of them.
    fn position(&self, key: &K, hash: Option<u64>) -> Option<usize> { self.index.slots(hash).iter().copied().find(|slot| self.entries[*slot].0.same_js_key(key)) }
}

impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltJsMap<K, V> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltJsMap").field("id", &self.id).field("entries", &self.store.borrow().entries).finish() } }

impl<K, V> SmeltJsMap<K, V> {
    fn new() -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::new())) } }
    //~ `clear` removes every entry without comparing keys, so it needs no
    //~ `SmeltJsKeyEq`/`Clone` bounds and lives on the unbounded impl block. It
    //~ replaces the store in place (through the shared `RefCell`, so every alias
    //~ sees the clear) rather than reallocating the handle, which would detach
    //~ the other aliases.
    fn clear(&mut self) { *self.store.borrow_mut() = SmeltJsMapStore::new(); }
}

impl<K: SmeltJsKeyEq + Clone, V: Clone> SmeltJsMap<K, V> {
    fn len(&self) -> usize { self.store.borrow().entries.len() }
    fn contains_key(&self, key: &K) -> bool { let hash = key.js_key_hash(); self.store.borrow().position(key, hash).is_some() }
    fn get(&self, key: &K) -> Option<V> { let hash = key.js_key_hash(); let store = self.store.borrow(); store.position(key, hash).map(|slot| store.entries[slot].1.clone()) }
    fn insert(&mut self, key: K, value: V) -> Option<V> { let hash = key.js_key_hash(); let mut store = self.store.borrow_mut(); let existing = store.position(&key, hash); if let Some(slot) = existing { Some(::std::mem::replace(&mut store.entries[slot].1, value)) } else { let slot = store.entries.len(); store.entries.push((key, value)); store.index.remember(slot, hash); None } }
    fn remove(&mut self, key: &K) -> Option<V> { let hash = key.js_key_hash(); let mut store = self.store.borrow_mut(); let existing = store.position(key, hash); match existing { Some(slot) => { let removed = store.entries.remove(slot).1; store.index.forget(slot); Some(removed) }, None => None } }
    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> { self.store.borrow().entries.clone().into_iter() }
    fn keys(&self) -> ::std::vec::IntoIter<K> { self.store.borrow().entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>().into_iter() }
    fn values(&self) -> ::std::vec::IntoIter<V> { self.store.borrow().entries.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>().into_iter() }
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) { for (key, value) in iter { self.insert(key, value); } }
    //~ Borrow the value stored under `key` for mutation, inserting `default`
    //~ first when the key is absent. This is the accessor a source
    //~ `dict[key].push(item)` lowers to (see
    //~ `smelt_mir::opt::DictEntryInPlaceMutation`): the entry is mutated
    //~ THROUGH the map, so neither the old value nor the updated one is ever
    //~ copied. Without it, growing n grouped entries cost O(n^2) element
    //~ clones because every mutation copied the whole entry out and back.
    //~
    //~ The returned `RefMut` is a live borrow of the shared store, so the
    //~ caller must not touch the same map while holding it; codegen therefore
    //~ evaluates the key, the default, and the pushed item BEFORE the call.
    //~ The slot is resolved (or appended) under one `borrow_mut`, which is
    //~ released before the guard is taken, so the guard projects a settled
    //~ slot index. Insertion order and `SameValueZero` key identity come from
    //~ the same `position`/`remember` pair `insert` uses, so a fused mutation
    //~ is indistinguishable from the copy-out/copy-back form it replaces —
    //~ and, the store being shared, it is visible through every alias.
    fn entry_or_insert(&mut self, key: K, default: impl FnOnce() -> V) -> ::std::cell::RefMut<'_, V> {
        let hash = key.js_key_hash();
        let slot = { let mut store = self.store.borrow_mut(); match store.position(&key, hash) { Some(slot) => slot, None => { let slot = store.entries.len(); store.entries.push((key, default())); store.index.remember(slot, hash); slot } } };
        ::std::cell::RefMut::map(self.store.borrow_mut(), move |store| &mut store.entries[slot].1)
    }
}

impl<K, V> Default for SmeltJsMap<K, V> {
    fn default() -> Self { Self::new() }
}

//~ The bulk constructors carry `K: SmeltJsKeyEq` because they build the key
//~ index up front. Every key type a Map can be built with already satisfies it
//~ (the map's own accessors demand it), so this widens no call site.
impl<K: SmeltJsKeyEq, V, const N: usize> From<[(K, V); N]> for SmeltJsMap<K, V> {
    fn from(entries: [(K, V); N]) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::from_entries(Vec::from(entries)))) } }
}

impl<K: SmeltJsKeyEq, V> ::std::iter::FromIterator<(K, V)> for SmeltJsMap<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::from_entries(iter.into_iter().collect()))) } }
}

impl<K: Clone, V: Clone> IntoIterator for SmeltJsMap<K, V> {
    type Item = (K, V);
    type IntoIter = ::std::vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter { ::std::rc::Rc::try_unwrap(self.store).map(|cell| cell.into_inner()).unwrap_or_else(|shared| shared.borrow().clone()).entries.into_iter() }
}

impl<K: SmeltJsKeyEq + Clone, V: PartialEq + Clone> PartialEq for SmeltJsMap<K, V> {
    fn eq(&self, other: &Self) -> bool { let store = self.store.borrow(); store.entries.len() == other.store.borrow().entries.len() && store.entries.iter().all(|(key, value)| other.get(key).is_some_and(|other_value| other_value == *value)) }
}
impl<K: SmeltJsKeyEq + Clone, V: Eq + Clone> Eq for SmeltJsMap<K, V> {}
//~ Erase a `Map` to a marker-bearing object: `{ __smelt_map: [[k, v], ...] }`
//~ stamped with the map's stable `id`. This is the dynamic boundary adapter
//~ — the only place a typed `SmeltJsMap` becomes a shapeless `SmeltUnknown`
//~ — and it preserves both the entries (as an array of `[key, value]` pairs)
//~ and the object identity so `isMap`/`isEqualWith`/`Object.prototype.toString`
//~ work on the erased value and `SmeltFromUnknown` can restore it losslessly.
impl<K: IntoSmeltUnknown + Clone, V: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltJsMap<K, V> { fn into_smelt_unknown(self) -> SmeltUnknown { let id = self.id; let pairs = self.store.borrow().entries.clone().into_iter().map(|(key, value)| SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), vec![key.into_smelt_unknown(), value.into_smelt_unknown()]))).collect::<Vec<_>>(); let object = Vec::from([("__smelt_map".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), pairs)))]); SmeltUnknown::Object(SmeltObject::with_id(id, object)) } }

// @smelt:item-end
