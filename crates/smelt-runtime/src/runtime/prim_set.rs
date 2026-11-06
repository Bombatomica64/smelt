// `SmeltPrimSet`: the insertion-ordered `Set` of primitives that needs no erased carrier.

// A source `Set` whose elements can key a Rust hash map: `SmeltPrimSet`.
//
// JavaScript specifies `Set` iteration as INSERTION ORDER, and these sets
// used to be a bare `::std::collections::HashSet`, which has none:
// `[...new Set('hello')].join('')` answered `leoh` from one construction and
// `hoel` from another in the same program. Sizes and membership were right,
// so nothing threw — code that de-duplicates while preserving order, one of
// the commonest JS idioms there is, silently scrambled its output.
//
// The shape is the record store's: a `Vec` of entries for order plus a hash
// index for lookup, so `has`/`add`/`delete` stay hashed while iteration,
// `forEach`, spread and `Array.from` answer in source order. Its method set
// and signatures deliberately match `SmeltJsSet`'s (below, under
// `needs_unknown`), so one emitted call text serves either backing.
//
// It exists SEPARATELY from `SmeltJsSet` because that container's membership
// runs every element through `IntoSmeltUnknown` for SameValueZero, which
// pulls the whole erased-value carrier into any program holding a set: the
// snapshot for a four-line `Set<string>` program went from 18 lines to over
// 450 when both backings were merged. For `bool`/`i64`/`String` (and
// optionals/unions of those) SameValueZero IS Rust equality, so nothing is
// lost by hashing the values directly, and pay-for-use is kept.
//
// Members live behind an `Rc` and clone as a refcount bump, matching
// `SmeltJsSet`: codegen clones a set at every use of a captured one, and the
// store is copy-on-write (`Rc::make_mut`) so a cloned set stays an
// independent value.
// @smelt:item SmeltPrimSetStore when prim_set
#[derive(Debug, Clone)]
pub struct SmeltPrimSetStore<T> {
    entries: Vec<T>,
    index: ::std::collections::HashSet<T>,
}

pub struct SmeltPrimSet<T> {
    id: usize,
    store: ::std::rc::Rc<SmeltPrimSetStore<T>>,
}

impl<T> SmeltPrimSet<T> {
    fn new() -> Self { Self::with_id(smelt_next_object_id()) }
    fn with_id(id: usize) -> Self { Self { id, store: ::std::rc::Rc::new(SmeltPrimSetStore { entries: Vec::new(), index: ::std::collections::HashSet::new() }) } }
    fn len(&self) -> usize { self.store.entries.len() }
    fn is_empty(&self) -> bool { self.store.entries.is_empty() }
    fn iter(&self) -> ::std::slice::Iter<'_, T> { self.store.entries.iter() }
}

impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> SmeltPrimSet<T> {
    fn store_mut(&mut self) -> &mut SmeltPrimSetStore<T> { ::std::rc::Rc::make_mut(&mut self.store) }
    fn contains(&self, value: &T) -> bool { self.store.index.contains(value) }
    //~ `add` on an element already present is a no-op in JS, and in
    //~ particular does NOT move it to the end of the iteration order.
    fn insert(&mut self, value: T) -> bool { if self.store.index.contains(&value) { return false; } let store = self.store_mut(); store.index.insert(value.clone()); store.entries.push(value); true }
    fn remove(&mut self, value: &T) -> bool { if !self.store.index.contains(value) { return false; } let store = self.store_mut(); store.index.remove(value); store.entries.retain(|entry| entry != value); true }
    fn clear(&mut self) { let store = self.store_mut(); store.entries.clear(); store.index.clear(); }
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) { for value in iter { self.insert(value); } }
    fn is_disjoint(&self, other: &Self) -> bool { self.store.entries.iter().all(|value| !other.contains(value)) }
    fn is_subset(&self, other: &Self) -> bool { self.store.entries.iter().all(|value| other.contains(value)) }
    fn is_superset(&self, other: &Self) -> bool { other.is_subset(self) }
    //~ The set-algebra helpers answer iterators of borrowed members in
    //~ insertion order, matching both `HashSet`'s signatures (so the emitted
    //~ call text is unchanged) and `SmeltJsSet`'s ordering.
    fn union<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.store.entries.iter().collect(); out.extend(other.store.entries.iter().filter(|value| !self.contains(value))); out.into_iter() }
    fn intersection<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.store.entries.iter().filter(|value| other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.store.entries.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn symmetric_difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.store.entries.iter().filter(|value| !other.contains(value)).collect(); for value in other.store.entries.iter() { if !self.contains(value) { out.push(value); } } out.into_iter() }
}

impl<T> Clone for SmeltPrimSet<T> { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPrimSet<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltPrimSet").field("id", &self.id).field("entries", &self.store.entries).finish() } }
impl<T> Default for SmeltPrimSet<T> { fn default() -> Self { Self::new() } }
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash, const N: usize> From<[T; N]> for SmeltPrimSet<T> { fn from(values: [T; N]) -> Self { values.into_iter().collect() } }
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> ::std::iter::FromIterator<T> for SmeltPrimSet<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { let mut set = Self::new(); set.extend(iter); set } }
impl<T: Clone> IntoIterator for SmeltPrimSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { match ::std::rc::Rc::try_unwrap(self.store) { Ok(store) => store.entries.into_iter(), Err(store) => store.entries.clone().into_iter() } } }
impl<'smelt_set, T> IntoIterator for &'smelt_set SmeltPrimSet<T> { type Item = &'smelt_set T; type IntoIter = ::std::slice::Iter<'smelt_set, T>; fn into_iter(self) -> Self::IntoIter { self.store.entries.iter() } }
//~ Two sets are equal when they hold the same members, as JS structural
//~ comparison helpers (`isEqual`) expect; `===` identity is answered by
//~ the `id` field through `reference_identity_text`, not by this impl.
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> PartialEq for SmeltPrimSet<T> { fn eq(&self, other: &Self) -> bool { self.store.entries.len() == other.store.entries.len() && self.store.entries.iter().all(|value| other.contains(value)) } }

// @smelt:item-end
