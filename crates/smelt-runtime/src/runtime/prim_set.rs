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
// A JS `Set` is a reference value: `const b = a; b.add(x)` adds to `a`, a set
// mutated inside a function is mutated for its caller, and a module-level set
// filled by a function is filled for everyone. So the members live in ONE
// store behind `Rc<RefCell<..>>` and `.clone()` is another handle to it with
// the same `id`, exactly like `SmeltList` and `SmeltJsMap`. A genuine copy —
// `new Set(a)`, Python `set(a)`, the result of set algebra — is a NEW set
// (`fresh_copy`, `FromIterator`) with its own id and buffer.
//
// The store used to be copy-on-write (`Rc::make_mut`), which made every handle
// an independent VALUE: codegen reads a captured or global set through a clone,
// so `seen.add(n)` inside a function wrote to a copy and was lost, and an
// alias never saw a write through the other name.
//
// Members are read by snapshot: `iter()` and the set-algebra helpers answer
// owned members in insertion order (the `SmeltJsMap` shape), because a borrow
// out of the `RefCell` cannot outlive its guard. Iterating a snapshot also
// means a body that adds to the set it iterates cannot invalidate the walk.
// @smelt:item SmeltPrimSetStore when prim_set
#[derive(Debug, Clone)]
pub struct SmeltPrimSetStore<T> {
    entries: Vec<T>,
    index: ::std::collections::HashSet<T>,
}

pub struct SmeltPrimSet<T> {
    id: usize,
    store: ::std::rc::Rc<::std::cell::RefCell<SmeltPrimSetStore<T>>>,
}

impl<T> SmeltPrimSet<T> {
    fn new() -> Self { Self::with_id(smelt_next_object_id()) }
    fn with_id(id: usize) -> Self { Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltPrimSetStore { entries: Vec::new(), index: ::std::collections::HashSet::new() })) } }
    fn len(&self) -> usize { self.store.borrow().entries.len() }
    fn is_empty(&self) -> bool { self.store.borrow().entries.is_empty() }
}

impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> SmeltPrimSet<T> {
    /// A NEW set with the same members in the same order: its own id and its
    /// own store, so later writes to either set leave the other unchanged.
    fn fresh_copy(&self) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(self.store.borrow().clone())) } }
    /// The members, in insertion order, as an owned snapshot.
    fn iter(&self) -> ::std::vec::IntoIter<T> { self.store.borrow().entries.clone().into_iter() }
    fn contains(&self, value: &T) -> bool { self.store.borrow().index.contains(value) }
    //~ `add` on an element already present is a no-op in JS, and in
    //~ particular does NOT move it to the end of the iteration order.
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn insert(&mut self, value: T) -> bool { let mut store = self.store.borrow_mut(); if store.index.contains(&value) { return false; } store.index.insert(value.clone()); store.entries.push(value); true }
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn remove(&mut self, value: &T) -> bool { let mut store = self.store.borrow_mut(); if !store.index.remove(value) { return false; } store.entries.retain(|entry| entry != value); true }
    #[expect(clippy::needless_pass_by_ref_mut, reason = "the write goes through the shared cell; `&mut` keeps the signature saying it writes, as SmeltJsMap does")] fn clear(&mut self) { let mut store = self.store.borrow_mut(); store.entries.clear(); store.index.clear(); }
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) { for value in iter { self.insert(value); } }
    fn is_disjoint(&self, other: &Self) -> bool { self.iter().all(|value| !other.contains(&value)) }
    fn is_subset(&self, other: &Self) -> bool { self.iter().all(|value| other.contains(&value)) }
    fn is_superset(&self, other: &Self) -> bool { other.is_subset(self) }
    //~ The set-algebra helpers answer owned members in insertion order,
    //~ matching `SmeltJsSet`'s signatures and ordering.
    fn union(&self, other: &Self) -> ::std::vec::IntoIter<T> { let mut out = self.iter().collect::<Vec<_>>(); out.extend(other.iter().filter(|value| !self.contains(value))); out.into_iter() }
    fn intersection(&self, other: &Self) -> ::std::vec::IntoIter<T> { self.iter().filter(|value| other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn difference(&self, other: &Self) -> ::std::vec::IntoIter<T> { self.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>().into_iter() }
    fn symmetric_difference(&self, other: &Self) -> ::std::vec::IntoIter<T> { let mut out = self.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>(); out.extend(other.iter().filter(|value| !self.contains(value))); out.into_iter() }
}

impl<T> Clone for SmeltPrimSet<T> { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPrimSet<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltPrimSet").field("id", &self.id).field("entries", &self.store.borrow().entries).finish() } }
impl<T> Default for SmeltPrimSet<T> { fn default() -> Self { Self::new() } }
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash, const N: usize> From<[T; N]> for SmeltPrimSet<T> { fn from(values: [T; N]) -> Self { values.into_iter().collect() } }
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> ::std::iter::FromIterator<T> for SmeltPrimSet<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { let mut set = Self::new(); set.extend(iter); set } }
impl<T: Clone> IntoIterator for SmeltPrimSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { match ::std::rc::Rc::try_unwrap(self.store) { Ok(store) => store.into_inner().entries.into_iter(), Err(store) => store.borrow().entries.clone().into_iter() } } }
impl<T: Clone> IntoIterator for &SmeltPrimSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.store.borrow().entries.clone().into_iter() } }
//~ Two sets are equal when they hold the same members, as JS structural
//~ comparison helpers (`isEqual`) expect; `===` identity is answered by
//~ the `id` field through `reference_identity_text`, not by this impl.
impl<T: Clone + ::std::cmp::Eq + ::std::hash::Hash> PartialEq for SmeltPrimSet<T> { fn eq(&self, other: &Self) -> bool { self.len() == other.len() && self.iter().all(|value| other.contains(&value)) } }

// @smelt:item-end

// @smelt:item impl_SmeltFromUnknown_for_SmeltPrimSet_T when prim_set & unknown
/// Recover a primitive `Set` from its erased array, or a `SmeltJsSet` marker.
impl<T: SmeltFromUnknown + Clone + ::std::cmp::Eq + ::std::hash::Hash> SmeltFromUnknown for SmeltPrimSet<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { let members = match value { SmeltUnknown::Array(members) => members, SmeltUnknown::Object(object) => match object.get("__smelt_set") { Some(SmeltUnknown::Array(members)) => members, _ => return Self::default() }, _ => return Self::default() }; members.into_vec().into_iter().map(T::smelt_from_unknown).collect() } }

// @smelt:item-end
