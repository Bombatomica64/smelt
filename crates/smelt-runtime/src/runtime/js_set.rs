// `SmeltJsSet`: JavaScript `Set` over erased members.

// JS `Set` container with SameValueZero membership and insertion order.
//
// Rust `HashSet` demands `Eq + Hash`, which is impossible for `f64`,
// generated unions, and generic type parameters, and is the wrong
// equality besides: JavaScript `Set` compares objects/functions by
// reference identity and treats `NaN` as equal to itself. Rather than
// require a per-element `Eq + Hash`, membership projects each element
// through its `IntoSmeltUnknown` erasure and compares the resulting
// runtime values with `SmeltJsKeyEq::same_js_key` (the same erased-key
// projection `SmeltJsMap` uses for keys). This makes one uniform,
// JS-correct container work for every element type that can be erased.
// Elements are stored in a `Vec` so iteration preserves insertion order
// like a real JS `Set`.
// Carries a stable object `id` — exactly like `SmeltJsMap` — so the
// identity a `Set` value has in JavaScript survives the erasure
// round-trip: when the set crosses a dynamic boundary
// (`IntoSmeltUnknown`) it becomes a marker-bearing `SmeltUnknown::Object`
// stamped with `__smelt_set` and its `id`, and un-erasing
// (`SmeltFromUnknown`) restores that same `id`. The marker is what lets
// `smelt_object_to_string_tag` report `[object Set]` and the
// `isSet`/`isEqualWith` runtime probes recognize the value once it is
// erased — a plain `SmeltUnknown::Array` cannot carry Set identity, which
// is why the marker boundary exists.
// Membership does NOT scan `entries`: every member is also recorded in a
// hash index (`hashed`) keyed by `smelt_js_member_hash_key` — a hash of
// the member's erased runtime value that is consistent with
// `same_js_key`, i.e. two members that compare SameValueZero-equal always
// land in the same bucket (see that function for the per-variant argument).
// Without it `insert` was `contains` + `push`, so building an n-member set
// — what `uniq`/`difference`/`intersection` do on every call — cost
// O(n^2) erasures. `entries` stays a plain insertion-ordered `Vec`, so
// iteration order, `IntoIterator`, and the erasure adapters are unchanged;
// the index only narrows which slots a comparison has to look at, and the
// surviving candidates are still confirmed with `same_member`, so a hash
// collision costs a comparison and never changes an answer.
// `unhashed` holds the slots of members that have no hashable identity —
// erased functions, whose `same_js_key` arm resolves through a mutable
// function-identity registry (`smelt_link_function_identity_key`) that can
// be rewritten after a member is inserted, which would leave a stored hash
// stale. Those members keep the linear scan.
//
// Members and index live in one store behind `Rc<RefCell<..>>`, and
// `.clone()` is another handle to the same set (same `id`): a JS `Set` is a
// reference value, so `const b = a; b.add(x)` adds to `a`, and a captured or
// module-level set that a function fills is filled for its caller. A genuine
// copy (`new Set(a)`, set algebra) is a NEW set with its own id and store
// (`fresh_copy`, `FromIterator`). This is `SmeltJsMap`'s treatment, and the
// same as `SmeltPrimSet`'s; see that type for the copy-on-write store this
// replaced and the writes it lost. Members are read by owned snapshot
// (`iter()`, the set-algebra helpers), because a borrow out of the `RefCell`
// cannot outlive its guard.
// @smelt:item SmeltJsSet when unknown
pub struct SmeltJsSet<T> {
    id: usize,
    store: ::std::rc::Rc<::std::cell::RefCell<SmeltJsSetStore<T>>>,
}

//~ A hash index over the slots of an insertion-ordered container.
//~
//~ Shared by `SmeltJsSet` (its members) and `SmeltJsMap` (its keys): both keep
//~ their entries in a plain `Vec` so iteration stays insertion-ordered, and both
//~ use this to narrow a lookup to the slots whose entry hashes the same instead
//~ of scanning every one. The bookkeeping is identical for either container
//~ because it only ever handles slot numbers, never the entries themselves.
//~ `unhashed` holds the slots of entries with no stable hashable identity —
//~ erased functions, whose identity resolves through a registry a later
//~ `smelt_link_function_identity_key` call can rewrite, so a hash taken at
//~ insert time is not guaranteed to stay valid. Those keep the linear scan,
//~ which cannot go stale.
//~ The index is keyed by a value that has ALREADY been hashed:
//~ `SmeltJsKeyEq::js_key_hash` runs the key through `SmeltFieldHasher`,
//~ whose `finish` applies a splitmix64 finalizer, so the `u64` handed to
//~ the index has full avalanche in every bit. Hashing it a second time
//~ inside the index map would buy no distribution and cost a whole extra
//~ hash round on the hottest path a `groupBy`/`countBy`/`Set` has --
//~ `SmeltJsMapStore::position` and `smelt_js_member_hash_key` together were
//~ 31% of es-toolkit's `group_by`. `SmeltPreHashedHasher` therefore passes
//~ the key straight through.
#[derive(Default, Clone, Copy)]
pub struct SmeltPreHashedHasher(u64);

impl ::std::hash::Hasher for SmeltPreHashedHasher {
    fn finish(&self) -> u64 { self.0 }
    fn write_u64(&mut self, value: u64) { self.0 = value; }
    //~ Only `write_u64` is ever called: the map below is keyed by `u64` and
    //~ `<u64 as Hash>::hash` calls exactly that. The byte path is unreachable
    //~ for this map, but a `Hasher` impl must supply it, so fold the bytes the
    //~ same way `SmeltFieldHasher` does rather than leaving a silent no-op that
    //~ would collide everything if the key type ever changed.
    fn write(&mut self, bytes: &[u8]) {
        let mut hasher = SmeltFieldHasher(self.0);
        ::std::hash::Hasher::write(&mut hasher, bytes);
        self.0 = ::std::hash::Hasher::finish(&hasher);
    }
}

/// A map keyed by an already-hashed `u64`, which is passed through unmixed.
pub type SmeltPreHashedMap<V> = ::std::collections::HashMap<u64, V, ::std::hash::BuildHasherDefault<SmeltPreHashedHasher>>;

//~ The overwhelming majority of hash keys index exactly ONE slot: a
//~ `groupBy` over n distinct keys has n buckets of one entry each until a
//~ genuine hash collision, and duplicate JavaScript keys cannot coexist in
//~ a Map at all. A `Vec<usize>` per bucket therefore charged one heap
//~ allocation per distinct key for a single `usize`, on the very path this
//~ container exists to make fast. `SmeltSlotList` stores that lone slot
//~ inline and only allocates once a bucket actually holds two.
#[derive(Clone, Debug)]
enum SmeltSlotList {
    One(usize),
    Many(Vec<usize>),
}

impl SmeltSlotList {
    //~ The slots in this bucket, in insertion order.
    fn slots(&self) -> &[usize] { match self { Self::One(slot) => ::std::slice::from_ref(slot), Self::Many(slots) => slots.as_slice() } }
    //~ Append `slot`, promoting a one-slot bucket to an allocated one.
    fn push(&mut self, slot: usize) { match self { Self::One(first) => *self = Self::Many(::std::vec![*first, slot]), Self::Many(slots) => slots.push(slot) } }
    //~ Drop `removed` and shift every later slot down by one, mirroring what
    //~ `entries.remove(removed)` did to the positions this bucket stores.
    //~ Returns whether the bucket is now empty and should be dropped.
    fn forget(&mut self, removed: usize) -> bool {
        match self {
            Self::One(slot) => { if *slot == removed { return true; } if *slot > removed { *slot -= 1; } false }
            Self::Many(slots) => {
                slots.retain(|existing| *existing != removed);
                for existing in slots.iter_mut() { if *existing > removed { *existing -= 1; } }
                //~ Demote back to the inline form so a bucket that collided once does not
                //~ keep its allocation for the rest of the container's life.
                match slots.len() { 0 => true, 1 => { *self = Self::One(slots[0]); false }, _ => false }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct SmeltJsSlotIndex {
    hashed: SmeltPreHashedMap<SmeltSlotList>,
    unhashed: Vec<usize>,
}

impl SmeltJsSlotIndex {
    fn new() -> Self { Self { hashed: SmeltPreHashedMap::default(), unhashed: Vec::new() } }
    //~ Index the freshly pushed slot `slot` under its entry's hash key.
    fn remember(&mut self, slot: usize, key: Option<u64>) { match key { Some(key) => match self.hashed.entry(key) { ::std::collections::hash_map::Entry::Occupied(mut bucket) => bucket.get_mut().push(slot), ::std::collections::hash_map::Entry::Vacant(bucket) => { bucket.insert(SmeltSlotList::One(slot)); } }, None => self.unhashed.push(slot) } }
    //~ Drop `slot` from the index and shift every later slot down by one, which
    //~ is what `entries.remove(slot)` did to the positions the index stores.
    fn forget(&mut self, slot: usize) { self.hashed.retain(|_, slots| !slots.forget(slot)); self.unhashed.retain(|existing| *existing != slot); for existing in self.unhashed.iter_mut() { if *existing > slot { *existing -= 1; } } }
    //~ The slots that may hold an entry with hash key `key`.
    fn slots(&self, key: Option<u64>) -> &[usize] { match key { Some(key) => self.hashed.get(&key).map_or(&[], SmeltSlotList::slots), None => self.unhashed.as_slice() } }
}

/// The members of a `SmeltJsSet` plus the hash index over them.
#[derive(Clone, Debug)]
struct SmeltJsSetStore<T> {
    entries: Vec<T>,
    index: SmeltJsSlotIndex,
}

impl<T> SmeltJsSetStore<T> {
    fn new() -> Self { Self { entries: Vec::new(), index: SmeltJsSlotIndex::new() } }
}

//~ Hand-written `Clone`: the derive would demand `T: Clone` even though
//~ sharing the store needs nothing of `T`.
impl<T> Clone for SmeltJsSet<T> { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }
//~ Hand-written `Debug` so the store stays invisible and the rendering keeps
//~ reading `SmeltJsSet { id: 1, entries: [..] }`, as it did when the members
//~ were an inline field.
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltJsSet<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltJsSet").field("id", &self.id).field("entries", &self.store.borrow().entries).finish() } }

impl<T> SmeltJsSet<T> {
    //~ The member at insertion position `index`, read live (see
    //~ `SmeltJsMap::smelt_key_at`).
    #[allow(dead_code)]
    fn smelt_live_len(&self) -> usize { self.store.borrow().entries.len() }
    #[allow(dead_code)]
    fn smelt_member_at(&self, index: usize) -> Option<T> where T: Clone { self.store.borrow().entries.get(index).cloned() }
    #[allow(dead_code)]
    fn smelt_entry_at(&self, index: usize) -> Option<(T, T)> where T: Clone { self.smelt_member_at(index).map(|member| (member.clone(), member)) }
    fn new() -> Self { Self::with_id(smelt_next_object_id()) }
    //~ Build an empty set that keeps a caller-supplied JavaScript object identity.
    fn with_id(id: usize) -> Self { Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsSetStore::new())) } }
    //~ Drop every member, in the shared store: every handle sees the set empty.
    //~ Needs no `T: Clone` and never copies the members it is about to drop.
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn clear(&mut self) { *self.store.borrow_mut() = SmeltJsSetStore::new(); }
    fn len(&self) -> usize { self.store.borrow().entries.len() }
    fn is_empty(&self) -> bool { self.store.borrow().entries.is_empty() }
}

impl<T: Clone + IntoSmeltUnknown> SmeltJsSet<T> {
    //~ SameValueZero equality via each element's erased runtime value.
    fn same_member(left: &T, right: &T) -> bool { left.clone().into_smelt_unknown().same_js_key(&right.clone().into_smelt_unknown()) }
    //~ The hash-index key of `value`, or `None` when it has no hashable identity.
    fn member_key(value: &T) -> Option<u64> { smelt_js_member_hash_key(&value.clone().into_smelt_unknown()) }
    //~ A NEW set with the same members in the same order: its own id and its
    //~ own store, so later writes to either set leave the other unchanged.
    fn fresh_copy(&self) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(self.store.borrow().clone())) } }
    //~ Slot of `value` in the members, comparing only the ones that share `key`
    //~ — every SameValueZero-equal member is guaranteed to be one of them.
    fn position_with_key(&self, value: &T, key: Option<u64>) -> Option<usize> { let store = self.store.borrow(); store.index.slots(key).iter().copied().find(|slot| Self::same_member(&store.entries[*slot], value)) }
    fn position(&self, value: &T) -> Option<usize> { self.position_with_key(value, Self::member_key(value)) }
    fn contains(&self, value: &T) -> bool { self.position(value).is_some() }
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn insert(&mut self, value: T) -> bool { let key = Self::member_key(&value); if self.position_with_key(&value, key).is_some() { return false; } let mut store = self.store.borrow_mut(); let slot = store.entries.len(); store.entries.push(value); store.index.remember(slot, key); true }
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn remove(&mut self, value: &T) -> bool { match self.position(value) { Some(slot) => { let mut store = self.store.borrow_mut(); store.entries.remove(slot); store.index.forget(slot); true }, None => false } }
    //~ The members, in insertion order, as an owned snapshot.
    fn iter(&self) -> ::std::vec::IntoIter<T> { self.store.borrow().entries.clone().into_iter() }
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) { for value in iter { self.insert(value); } }
    fn is_disjoint(&self, other: &Self) -> bool { self.iter().all(|value| !other.contains(&value)) }
    fn is_subset(&self, other: &Self) -> bool { self.iter().all(|value| other.contains(&value)) }
    fn is_superset(&self, other: &Self) -> bool { other.is_subset(self) }
    fn union(&self, other: &Self) -> ::std::vec::IntoIter<T> { let mut out = self.iter().collect::<Vec<_>>(); out.extend(other.iter().filter(|value| !self.contains(value))); out.into_iter() }
    fn intersection(&self, other: &Self) -> ::std::vec::IntoIter<T> { self.iter().filter(|value| other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn difference(&self, other: &Self) -> ::std::vec::IntoIter<T> { self.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn symmetric_difference(&self, other: &Self) -> ::std::vec::IntoIter<T> { let mut out = self.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>(); out.extend(other.iter().filter(|value| !self.contains(value))); out.into_iter() }
}

impl<T> Default for SmeltJsSet<T> { fn default() -> Self { Self::new() } }
impl<T: Clone + IntoSmeltUnknown, const N: usize> From<[T; N]> for SmeltJsSet<T> { fn from(values: [T; N]) -> Self { values.into_iter().collect() } }
impl<T: Clone + IntoSmeltUnknown> ::std::iter::FromIterator<T> for SmeltJsSet<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { let mut set = Self::new(); set.extend(iter); set } }
//~ Taking the members by value reclaims the store when this handle is the
//~ last one and copies them only when it is not, so the `T: Clone` bound is
//~ the price of the shared store (every generated element type is `Clone`).
impl<T: Clone> IntoIterator for SmeltJsSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { smelt_js_set_into_members(self.store).into_iter() } }
impl<T: Clone> IntoIterator for &SmeltJsSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.store.borrow().entries.clone().into_iter() } }
impl<T: Clone + IntoSmeltUnknown> PartialEq for SmeltJsSet<T> { fn eq(&self, other: &Self) -> bool { self.len() == other.len() && self.iter().all(|value| other.contains(&value)) } }

/// Take the members out of a set store, reusing them when this is the last handle.
fn smelt_js_set_into_members<T: Clone>(store: ::std::rc::Rc<::std::cell::RefCell<SmeltJsSetStore<T>>>) -> Vec<T> { match ::std::rc::Rc::try_unwrap(store) { Ok(store) => store.into_inner().entries, Err(shared) => shared.borrow().entries.clone() } }
//~ Erase a `Set` to a marker-bearing object: `{ __smelt_set: [members...] }`
//~ stamped with the set's stable `id`. This is the dynamic boundary adapter
//~ — the only place a typed `SmeltJsSet` becomes a shapeless `SmeltUnknown`
//~ — and it preserves both the members and the object identity so
//~ `isSet`/`isEqualWith`/`Object.prototype.toString` work on the erased
//~ value and `SmeltFromUnknown` can restore it losslessly. Mirrors the
//~ `SmeltJsMap` `__smelt_map` adapter. Members are sorted by their stable
//~ erased-hash key — as the pre-marker bare-array erasure did — so that
//~ spreading / iterating an erased Set yields a deterministic order and
//~ structural equality over two sets with the same members (but different
//~ insertion order) still compares equal.
impl<T: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltJsSet<T> { fn into_smelt_unknown(self) -> SmeltUnknown { let id = self.id; let members = smelt_js_set_into_members(self.store).into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>(); let object = Vec::from([("__smelt_set".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), members)))]); SmeltUnknown::Object(SmeltObject::with_id(id, object)) } }

//~ The hash side of `SmeltJsKeyEq::same_js_key`, used to bucket `SmeltJsSet`
//~ members. The contract a hash index needs is one-directional: whenever two
//~ erased values are `same_js_key`-equal they MUST hash the same. Per variant:
//~
//~ * `Null`/`Undefined`/`Bool`/`String`/`Symbol` compare by value and hash
//~   that same value, tagged by variant (`same_js_key` never equates values
//~   of two different variants: its fallthrough is structural equality, whose
//~   cross-variant arm is `false`).
//~ * `Number` is SameValueZero, which differs from `f64` bit equality in
//~   exactly two places, and both are normalized here: every `NaN` hashes as
//~   one canonical `NaN` (`same_js_key` equates them), and `-0.0` hashes as
//~   `+0.0` (`f64 ==` equates them, but their bit patterns differ). Any other
//~   pair that is `==` and not `NaN` already has identical bits.
//~ * `Array`/`Object`/`Promise` compare by REFERENCE identity — `same_js_key`
//~   looks only at the stable `id` — so the hash is that `id` and nothing
//~   else. Deliberately NOT the structural hash
//~   (`smelt_unknown_stable_hash_key`): that one hashes contents, so mutating
//~   a member already in a set would change its hash and lose it, and two
//~   structurally equal but distinct objects (two distinct JS Set members)
//~   would collide needlessly.
//~ * `Function` returns `None`: `same_js_key` resolves callables through
//~   `smelt_function_identity_of`, a registry a later
//~   `smelt_link_function_identity_key` call can still rewrite, so no hash
//~   taken at insert time is guaranteed to stay valid. `None` members keep
//~   the linear scan, which cannot go stale.
//~ Hash one already-normalized value into the same key space as
//~ `smelt_js_member_hash_key`. Used by the primitive `SmeltJsKeyEq::js_key_hash`
//~ impls, which compare a key only against another key of its own type and so
//~ need self-consistency, not the cross-variant tagging the erased hash does.
fn smelt_js_hash_one<H: ::std::hash::Hash>(value: &H) -> u64 { let mut hasher = SmeltFieldHasher::default(); value.hash(&mut hasher); ::std::hash::Hasher::finish(&hasher) }

// @smelt:item-end

// @smelt:item smelt_js_member_hash_key
// @smelt:if unknown
fn smelt_js_member_hash_key(value: &SmeltUnknown) -> Option<u64> {
    let mut hasher = SmeltFieldHasher::default();
    match value {
        SmeltUnknown::Null => 0_u8.hash(&mut hasher),
        SmeltUnknown::Undefined => 1_u8.hash(&mut hasher),
        SmeltUnknown::Bool(value) => { 2_u8.hash(&mut hasher); value.hash(&mut hasher); },
        SmeltUnknown::Number(value) => { 3_u8.hash(&mut hasher); let bits = if value.is_nan() { f64::NAN.to_bits() } else if *value == 0.0 { 0_f64.to_bits() } else { value.to_bits() }; bits.hash(&mut hasher); },
// @smelt:end
// @smelt:if bigint
        SmeltUnknown::BigInt(value) => { 9_u8.hash(&mut hasher); value.hash(&mut hasher); },
// @smelt:end
// @smelt:if unknown
        SmeltUnknown::String(value) => { 4_u8.hash(&mut hasher); value.hash(&mut hasher); },
        SmeltUnknown::Symbol(value) => { 5_u8.hash(&mut hasher); value.hash(&mut hasher); },
        SmeltUnknown::Array(value) => { 6_u8.hash(&mut hasher); value.id.hash(&mut hasher); },
        SmeltUnknown::Object(value) => { 7_u8.hash(&mut hasher); value.id.hash(&mut hasher); },
        SmeltUnknown::Promise(value) => { 8_u8.hash(&mut hasher); value.id.hash(&mut hasher); },
        SmeltUnknown::Function(_) => return None,
    }
    Some(::std::hash::Hasher::finish(&hasher))
}

// @smelt:end
// @smelt:item-end
