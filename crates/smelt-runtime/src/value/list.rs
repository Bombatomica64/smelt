//! The identity-bearing statically-typed list — what `Type::List` lowers to.
//!
//! `SmeltList<T>` is a `Vec<T>` plus a JavaScript reference identity. It
//! `Deref`s to its backing `Vec<T>` so every `Vec` method is available without
//! restating it, and `Clone` *shares* the reference id (so the value-copies
//! codegen inserts when a list flows through an expression keep identity) while
//! deep-cloning the elements. A genuine JS array copy — `[...a]`, `slice()` —
//! goes through `fresh_copy`, which mints a new id.
//!
//! `Debug` is hand-written rather than derived so that it forwards to the
//! backing `Vec`: `console.log([1, 2, 3])` must print `[1.0, 2.0, 3.0]`, not the
//! `SmeltList { .. }` wrapper. `PartialEq`/`Hash` are structural for the same
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
pub struct SmeltList<T> {
    id: usize,
    values: Vec<T>,
}
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltList<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.values.fmt(formatter) } }
impl<T: Clone> Clone for SmeltList<T> { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } } }
#[allow(dead_code)]
impl<T> SmeltList<T> {
    /// Create an identity-bearing typed list with a fresh JS reference identity.
    fn new(values: Vec<T>) -> Self { Self { id: smelt_next_object_id(), values } }
    /// Reuse a caller-supplied identity so an erase/extract round-trip stays `===` equal.
    fn with_id(id: usize, values: Vec<T>) -> Self { Self { id, values } }
    /// A JS array copy (`[...a]`, `slice`): same contents, a NEW reference identity.
    fn fresh_copy(&self) -> Self where T: Clone { Self::new(self.values.clone()) }
    /// JS reference identity of this list.
    fn id(&self) -> usize { self.id }
    /// Consume the list, yielding the backing storage.
    fn into_vec(self) -> Vec<T> { self.values }
}
impl<T> From<Vec<T>> for SmeltList<T> { fn from(values: Vec<T>) -> Self { Self::new(values) } }
impl<T> ::std::iter::FromIterator<T> for SmeltList<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { Self::new(iter.into_iter().collect()) } }
impl<T> ::std::ops::Deref for SmeltList<T> { type Target = Vec<T>; fn deref(&self) -> &Vec<T> { &self.values } }
impl<T> ::std::ops::DerefMut for SmeltList<T> { fn deref_mut(&mut self) -> &mut Vec<T> { &mut self.values } }
impl<T> IntoIterator for SmeltList<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.values.into_iter() } }
impl<'smelt_list, T> IntoIterator for &'smelt_list SmeltList<T> { type Item = &'smelt_list T; type IntoIter = ::std::slice::Iter<'smelt_list, T>; fn into_iter(self) -> Self::IntoIter { self.values.iter() } }
impl<T: PartialEq> PartialEq for SmeltList<T> { fn eq(&self, other: &Self) -> bool { self.values == other.values } }
impl<T: PartialEq> Eq for SmeltList<T> {}
impl<T: ::std::hash::Hash> ::std::hash::Hash for SmeltList<T> { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { self.values.hash(state); } }
impl<T> Default for SmeltList<T> { fn default() -> Self { Self::new(Vec::new()) } }
impl<T> From<SmeltList<T>> for Vec<T> { fn from(list: SmeltList<T>) -> Self { list.values } }
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::SmeltList;

    /// A fresh list mints a fresh identity; `Clone` shares it.
    ///
    /// Codegen inserts `.clone()` freely when a list flows through expressions
    /// and recursive calls. If those clones minted new ids, `a === a` would read
    /// `false` after any such copy, which is why `Clone` is *not* a JS array
    /// copy.
    #[test]
    fn clone_shares_identity_while_new_mints_one() {
        let list = SmeltList::new(vec![1, 2, 3]);
        let aliased = list.clone();
        assert_eq!(
            list.id(),
            aliased.id(),
            "Clone models a value-copy of the same JS array, so the id is shared"
        );
        let other = SmeltList::new(vec![1, 2, 3]);
        assert_ne!(
            list.id(),
            other.id(),
            "two separately constructed arrays are never `===` in JS"
        );
    }

    /// `fresh_copy` is the JS array copy: equal contents, a NEW identity.
    #[test]
    fn fresh_copy_mints_a_new_identity() {
        let list = SmeltList::new(vec![1, 2, 3]);
        let copy = list.fresh_copy();
        assert_eq!(*list, *copy, "`[...a]` copies the contents");
        assert_ne!(
            list.id(),
            copy.id(),
            "`[...a] === a` is `false` in JavaScript"
        );
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

    /// `Deref`/`DerefMut` expose the backing `Vec`, and iteration is ordered.
    #[test]
    fn deref_exposes_the_backing_vec() {
        let mut list = SmeltList::from(vec![1, 2]);
        list.push(3);
        assert_eq!(list.len(), 3, "DerefMut reaches Vec::push");
        assert_eq!(
            (&list).into_iter().copied().collect::<Vec<_>>(),
            vec![1, 2, 3],
            "iteration preserves array order"
        );
        assert_eq!(
            list.clone().into_vec(),
            vec![1, 2, 3],
            "into_vec yields the backing storage"
        );
        assert_eq!(
            Vec::from(list),
            vec![1, 2, 3],
            "From<SmeltList> yields the backing storage"
        );
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

    /// `Debug` forwards to the backing `Vec`, not the wrapper.
    ///
    /// `console.log([1, 2, 3])` prints `[1, 2, 3]`; a derived `Debug` would print
    /// `SmeltList { id: .., values: [..] }`.
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
