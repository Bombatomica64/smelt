//! The identity-bearing statically-typed list — what `Type::List` lowers to.
//!
//! `SmeltList<T>` is a JavaScript array: a reference identity (`id`) plus a
//! *shared* backing buffer (`Rc<RefCell<Vec<T>>>`). `Clone` bumps the refcount
//! and keeps the id, so a cloned handle names the same array as the original —
//! identity and storage agree. That is what makes `const b = a; b.push(x)`
//! observable through `a`, an array passed to a function mutable by the callee,
//! and an array read back out of an object, a `Map`, or an outer array the same
//! array rather than a copy.
//!
//! This is the same treatment `SmeltArray` (the erased array) and `SmeltJsMap`
//! already have, and for the same reason: JS arrays and Maps are reference
//! values. It is deliberately stronger than `SmeltJsSet`'s copy-on-write store —
//! copy-on-write is not enough here, because in the motivating case two handles
//! are alive at the moment of the write (the container's stored handle and the
//! local alias), so `Rc::make_mut` would copy and the write would be lost.
//!
//! A genuine JS array copy — `[...a]`, `slice()` — goes through
//! [`SmeltList::fresh_copy`], which mints a new id *and* a new buffer. Keeping
//! those operations copying is what stops shared storage from becoming
//! over-sharing.
//!
//! # No `Deref`
//!
//! The elements live behind a `RefCell`, so there is no `&Vec<T>` to hand out
//! that could outlive the borrow — the same argument the erased `SmeltArray`
//! makes. Instead the type exposes [`SmeltList::borrow`] and
//! [`SmeltList::borrow_mut`], and the emitter renders a list receiver as
//! `list.borrow()` / `list.borrow_mut()`. Guards live to the end of the enclosing
//! statement, which is safe because emitted code is three-address: see the
//! invariant documented on `FunctionEmitter::local_value_text`. A read receiver
//! must never be rendered with `borrow_mut`, because an index expression such as
//! `arr.get({ .. arr.len() .. })` takes two simultaneous borrows of one cell —
//! two shared borrows are fine, a second mutable one panics "already borrowed".
//!
//! `Debug` is hand-written rather than derived so that it forwards to the
//! backing `Vec`: `console.log([1, 2, 3])` must print `[1.0, 2.0, 3.0]`, not the
//! `SmeltList { .. }` wrapper, and the shared cell must not surface as
//! `RefCell { value: [..] }`. `PartialEq`/`Hash` are structural for the same
//! reason — JS `===` on arrays is emitted as an id comparison by the emitter, so
//! these impls serve Rust-side containers and assertions.
//!
//! The `#[allow(dead_code)]` attributes inside the marked region are part of the
//! emitted text: a generated crate uses only the constructors its program needs.
//!
//! The type is emitted whenever a list is used, independently of `SmeltUnknown`;
//! the `SmeltUnknown`-dependent impls (erasure, `From<SmeltArray>`, serde) are
//! still emitted by `smelt-codegen-rust`'s `needs_unknown` block and have not
//! moved here yet.

use super::smelt_next_object_id;

// @smelt:item SmeltList
/// A JavaScript array: a reference identity plus a shared backing buffer.
pub struct SmeltList<T> {
    id: usize,
    values: ::std::rc::Rc<::std::cell::RefCell<Vec<T>>>,
}
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltList<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.values.borrow().fmt(formatter) } }
impl<T> Clone for SmeltList<T> { fn clone(&self) -> Self { Self { id: self.id, values: ::std::rc::Rc::clone(&self.values) } } }
#[allow(dead_code)]
impl<T> SmeltList<T> {
    /// Create an identity-bearing typed list with a fresh JS reference identity.
    fn new(values: Vec<T>) -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)) } }
    /// Reuse a caller-supplied identity so an erase/extract round-trip stays `===` equal.
    fn with_id(id: usize, values: Vec<T>) -> Self { Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)) } }
    /// Reuse an existing shared buffer, so a re-wrap keeps aliasing the same array.
    fn with_storage(id: usize, values: ::std::rc::Rc<::std::cell::RefCell<Vec<T>>>) -> Self { Self { id, values } }
    /// Another handle on this array's shared buffer.
    fn storage(&self) -> ::std::rc::Rc<::std::cell::RefCell<Vec<T>>> { ::std::rc::Rc::clone(&self.values) }
    /// JS reference identity of this list.
    fn id(&self) -> usize { self.id }
    /// Borrow the backing storage for reading. The guard lives to the end of the
    /// enclosing statement; never pair it with `borrow_mut` in one expression.
    fn borrow(&self) -> ::std::cell::Ref<'_, Vec<T>> { self.values.borrow() }
    /// Borrow the backing storage for writing, through the shared cell — so this
    /// takes `&self`, and every other handle on the array observes the write.
    fn borrow_mut(&self) -> ::std::cell::RefMut<'_, Vec<T>> { self.values.borrow_mut() }
    /// Element count, taking its own short-lived borrow.
    fn len(&self) -> usize { self.values.borrow().len() }
    /// Whether the array holds no elements.
    fn is_empty(&self) -> bool { self.values.borrow().is_empty() }
    /// Replace every element in place, so aliases observe the new contents.
    fn replace_all(&self, values: Vec<T>) { *self.values.borrow_mut() = values; }
}
#[allow(dead_code)]
impl<T: Clone> SmeltList<T> {
    /// A JS array copy (`[...a]`, `slice`): same contents, a NEW identity AND a
    /// new buffer. This is the operation that keeps sharing from over-sharing.
    fn fresh_copy(&self) -> Self { Self::new(self.values.borrow().clone()) }
    /// Snapshot the elements. This COPIES unless this is the last handle, so
    /// mutating the result does not write back.
    fn into_vec(self) -> Vec<T> { match ::std::rc::Rc::try_unwrap(self.values) { Ok(cell) => cell.into_inner(), Err(shared) => shared.borrow().clone() } }
    /// Snapshot the elements without consuming the handle.
    fn to_vec(&self) -> Vec<T> { self.values.borrow().clone() }
    /// Read one element by index, cloned out of the shared buffer (JS `arr[i]`).
    fn get_index(&self, index: usize) -> Option<T> { self.values.borrow().get(index).cloned() }
    /// Set the element at a numeric index, extending with `fill` holes to match JS `arr[i] = v`.
    fn set_index(&self, index: usize, value: T, fill: T) { let mut values = self.values.borrow_mut(); if index >= values.len() { values.resize(index.saturating_add(1), fill); } values[index] = value; }
    /// Iterate a snapshot of the elements, so the buffer is not borrowed across the loop body.
    fn iter(&self) -> ::std::vec::IntoIter<T> { self.values.borrow().clone().into_iter() }
}
impl<T> From<Vec<T>> for SmeltList<T> { fn from(values: Vec<T>) -> Self { Self::new(values) } }
impl<T> ::std::iter::FromIterator<T> for SmeltList<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { Self::new(iter.into_iter().collect()) } }
impl<T: Clone> IntoIterator for SmeltList<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.into_vec().into_iter() } }
impl<T: Clone> IntoIterator for &SmeltList<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.values.borrow().clone().into_iter() } }
impl<T: PartialEq> PartialEq for SmeltList<T> { fn eq(&self, other: &Self) -> bool { *self.values.borrow() == *other.values.borrow() } }
impl<T: PartialEq> Eq for SmeltList<T> {}
impl<T: ::std::hash::Hash> ::std::hash::Hash for SmeltList<T> { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { self.values.borrow().hash(state); } }
impl<T> Default for SmeltList<T> { fn default() -> Self { Self::new(Vec::new()) } }
impl<T: Clone> From<SmeltList<T>> for Vec<T> { fn from(list: SmeltList<T>) -> Self { list.into_vec() } }
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::SmeltList;

    /// A fresh list mints a fresh identity; `Clone` shares it *and* the buffer.
    ///
    /// Codegen inserts `.clone()` freely when a list flows through expressions
    /// and recursive calls. If those clones minted new ids, `a === a` would read
    /// `false` after any such copy, which is why `Clone` is *not* a JS array
    /// copy; and because a JS array is a reference value, the clone must also
    /// share the storage, or a write through one handle would be invisible to
    /// the other while both still claimed to be the same array.
    #[test]
    fn clone_shares_identity_and_storage_while_new_mints_one() {
        let list = SmeltList::new(vec![1, 2, 3]);
        let aliased = list.clone();
        assert_eq!(
            list.id(),
            aliased.id(),
            "Clone models another handle on the same JS array, so the id is shared"
        );
        aliased.borrow_mut().push(4);
        assert_eq!(
            list.len(),
            4,
            "a push through one handle is visible through the other"
        );
        let other = SmeltList::new(vec![1, 2, 3]);
        assert_ne!(
            list.id(),
            other.id(),
            "two separately constructed arrays are never `===` in JS"
        );
    }

    /// `fresh_copy` is the JS array copy: equal contents, a NEW identity, and an
    /// independent buffer.
    ///
    /// This is the assertion that catches over-sharing. `[...a]` and `a.slice()`
    /// are fresh arrays in JavaScript: writing to either side must not cross.
    #[test]
    fn fresh_copy_mints_a_new_identity_and_buffer() {
        let list = SmeltList::new(vec![1, 2, 3]);
        let copy = list.fresh_copy();
        assert_eq!(list.to_vec(), copy.to_vec(), "`[...a]` copies the contents");
        assert_ne!(
            list.id(),
            copy.id(),
            "`[...a] === a` is `false` in JavaScript"
        );
        copy.borrow_mut().push(4);
        assert_eq!(list.len(), 3, "a write to the copy does not reach the source");
    }

    /// `with_storage` re-wraps an existing buffer, keeping the alias.
    #[test]
    fn with_storage_keeps_aliasing_the_same_array() {
        let list = SmeltList::new(vec![1]);
        let rewrapped = SmeltList::with_storage(list.id(), list.storage());
        rewrapped.borrow_mut().push(2);
        assert_eq!(list.len(), 2, "a re-wrap names the same array");
        assert_eq!(list.id(), rewrapped.id(), "and keeps its identity");
    }

    /// `with_id` restores a caller-supplied identity for erase round-trips.
    #[test]
    fn with_id_preserves_a_round_tripped_identity() {
        let list = SmeltList::new(vec!["a"]);
        let restored = SmeltList::with_id(list.id(), vec!["a"]);
        assert_eq!(
            list.id(),
            restored.id(),
            "erasing then extracting a list must stay `===` equal"
        );
    }

    /// Equality and hashing ignore identity and compare contents.
    ///
    /// Rust-side `==` on `SmeltList` is *structural*: JS `===` on arrays is
    /// emitted as an id comparison by the emitter, so this impl is what
    /// `HashMap`/`assert_eq!` in generated code use.
    #[test]
    fn equality_and_hashing_are_structural() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash as _, Hasher as _};

        let left = SmeltList::new(vec![1, 2]);
        let right = SmeltList::new(vec![1, 2]);
        assert_eq!(left, right, "distinct ids, equal contents compare equal");

        let mut left_hasher = DefaultHasher::new();
        let mut right_hasher = DefaultHasher::new();
        left.hash(&mut left_hasher);
        right.hash(&mut right_hasher);
        assert_eq!(
            left_hasher.finish(),
            right_hasher.finish(),
            "hashing must agree with structural equality"
        );
    }

    /// `borrow`/`borrow_mut` expose the backing `Vec`, and iteration is ordered.
    #[test]
    fn borrow_exposes_the_backing_vec() {
        let list = SmeltList::from(vec![1, 2]);
        list.borrow_mut().push(3);
        assert_eq!(list.len(), 3, "borrow_mut reaches Vec::push");
        assert_eq!(
            list.borrow().iter().copied().collect::<Vec<_>>(),
            vec![1, 2, 3],
            "iteration preserves array order"
        );
        assert_eq!(
            (&list).into_iter().collect::<Vec<_>>(),
            vec![1, 2, 3],
            "iterating a handle yields a snapshot in order"
        );
        assert_eq!(
            list.clone().into_vec(),
            vec![1, 2, 3],
            "into_vec yields the elements"
        );
        assert_eq!(
            Vec::from(list),
            vec![1, 2, 3],
            "From<SmeltList> yields the elements"
        );
    }

    /// `set_index` extends with holes, matching JS `arr[i] = v` past the end.
    #[test]
    fn set_index_extends_with_holes() {
        let list = SmeltList::new(vec![1]);
        list.set_index(3, 9, 0);
        assert_eq!(list.to_vec(), vec![1, 0, 0, 9], "the gap is filled");
        assert_eq!(list.get_index(3), Some(9), "and the element landed");
        assert_eq!(list.get_index(9), None, "out of range reads as missing");
    }

    /// `Default` and `FromIterator` both build identity-bearing lists.
    #[test]
    fn default_and_collect_mint_identities() {
        let empty: SmeltList<u8> = SmeltList::default();
        let collected: SmeltList<u8> = (1..=3).collect();
        assert!(empty.is_empty(), "Default is the empty array");
        assert_eq!(collected.len(), 3, "FromIterator collects every element");
        assert_ne!(
            empty.id(),
            collected.id(),
            "each construction is a distinct JS array"
        );
    }

    /// `Debug` forwards to the backing `Vec`, not the wrapper or the cell.
    ///
    /// `console.log([1, 2, 3])` prints `[1, 2, 3]`; a derived `Debug` would print
    /// `SmeltList { id: .., values: RefCell { value: [..] } }`.
    #[test]
    fn debug_prints_the_backing_vec() {
        let list = SmeltList::new(vec![1, 2, 3]);
        assert_eq!(
            format!("{list:?}"),
            "[1, 2, 3]",
            "Debug must not leak the wrapper"
        );
    }
}
