//! Erased views whose field snapshot is built on first use.
//!
//! # The cost this removes
//!
//! Erasing a reference record (a class lifted to `Rc<RefCell<Inner>>`, see
//! `reference_origin`) builds an erased object holding a snapshot of every
//! own field, its prototype members and its markers, then stamps the live
//! handle into the store's `origin` slot. Very often the erased value is only
//! carried across an erased ABI and narrowed straight back, and narrowing reads
//! the origin, never the snapshot. Hono shows the shape on every request: a
//! route handler is stored as `unknown`, so `handler(c, next)` erases the
//! `Context` — about twenty fields, most of them class-field arrows that each
//! become an erased adapter registered in the callable table — and the
//! handler's own adapter immediately recovers the same `Context` from the
//! origin. Building that snapshot was ~13% of a request.
//!
//! # The representation
//!
//! The field store sits behind [`STORE_CELL`]'s `SmeltStoreCell` instead of a
//! bare `RefCell`. It answers `borrow()`/`borrow_mut()` exactly like the
//! `RefCell` it wraps — every existing call site is unchanged — except that a
//! store created with a pending snapshot builds it first. A view built eagerly
//! has nothing pending and behaves byte for byte as before. Only the origin
//! slot is read and written without building the snapshot
//! (`raw_borrow`/`raw_borrow_mut`), because the origin is exactly the part of
//! the view that does not depend on it.
//!
//! # Why this preserves behaviour
//!
//! The snapshot is built by the same field-wise text, from the same live
//! handle, on the first read of ANY entry: a member read, `for...in`, JSON,
//! equality, a marker probe, a write. A reference record is one JavaScript
//! object, so a view of it that reflects the fields as of its first read is
//! at least as faithful as one frozen at the erasure; a value record (copy
//! semantics) never takes this path, and neither does a generic record, whose
//! type parameters a `'static` boxed builder could not name.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the prelude emitter and the emitter shards"
)]

/// The store cell, emitted right after `SmeltFieldStore`.
///
/// `SmeltPendingFill::insert` is a plain function pointer chosen where the
/// store's key and value types are concrete, so materializing needs no trait
/// bounds and works from every generic `borrow()` site. The builder runs with
/// no borrow of this store held, so a snapshot that reads other erased values
/// cannot trip the `RefCell`.
pub(crate) const STORE_CELL: &str = r"/// A snapshot an erased view builds the first time its store is read.
struct SmeltPendingFill<K, V> {
    build: Box<dyn FnOnce() -> Vec<(K, V)>>,
    insert: fn(&mut SmeltFieldStore<K, V>, Vec<(K, V)>),
}

/// The `RefCell` around a field store, building a pending snapshot on first use.
///
/// See `lazy_view` in the code generator: `borrow`/`borrow_mut` are the
/// `RefCell` ones after materializing; `raw_borrow`/`raw_borrow_mut` skip it
/// and exist only for the `origin` slot.
pub struct SmeltStoreCell<K, V> {
    cell: ::std::cell::RefCell<SmeltFieldStore<K, V>>,
    pending: ::std::cell::Cell<Option<SmeltPendingFill<K, V>>>,
}

#[allow(dead_code)]
impl<K, V> SmeltStoreCell<K, V> {
    /// A cell over an already-populated store.
    #[inline]
    fn new(store: SmeltFieldStore<K, V>) -> Self { Self { cell: ::std::cell::RefCell::new(store), pending: ::std::cell::Cell::new(None) } }
    /// A cell whose entries `build` produces on the first read or write.
    fn lazy(store: SmeltFieldStore<K, V>, build: Box<dyn FnOnce() -> Vec<(K, V)>>, insert: fn(&mut SmeltFieldStore<K, V>, Vec<(K, V)>)) -> Self {
        Self { cell: ::std::cell::RefCell::new(store), pending: ::std::cell::Cell::new(Some(SmeltPendingFill { build, insert })) }
    }
    /// Build the pending snapshot, if any, into the store.
    #[inline]
    fn materialize(&self) {
        if let Some(fill) = self.pending.take() {
            let entries = (fill.build)();
            (fill.insert)(&mut self.cell.borrow_mut(), entries);
        }
    }
    /// Borrow the store, building a pending snapshot first.
    #[inline]
    fn borrow(&self) -> ::std::cell::Ref<'_, SmeltFieldStore<K, V>> { self.materialize(); self.cell.borrow() }
    /// Mutably borrow the store, building a pending snapshot first.
    #[inline]
    fn borrow_mut(&self) -> ::std::cell::RefMut<'_, SmeltFieldStore<K, V>> { self.materialize(); self.cell.borrow_mut() }
    /// Borrow the store as it is, pending snapshot left unbuilt (the origin slot only).
    #[inline]
    fn raw_borrow(&self) -> ::std::cell::Ref<'_, SmeltFieldStore<K, V>> { self.cell.borrow() }
    /// Mutably borrow the store as it is (the origin slot only).
    #[inline]
    fn raw_borrow_mut(&self) -> ::std::cell::RefMut<'_, SmeltFieldStore<K, V>> { self.cell.borrow_mut() }
}

impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltStoreCell<K, V> {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.borrow().fmt(formatter) }
}";

/// The `SmeltObject` constructor for a view whose entries are built lazily.
pub(crate) const OBJECT_LAZY_CTOR: &str = "    /// Build an erased object whose entries `build` produces on first use (see `SmeltStoreCell`).\n    #[allow(dead_code)]\n    fn lazy_with_id(id: usize, build: impl FnOnce() -> Vec<(String, SmeltUnknown)> + 'static) -> Self { Self { id, store: ::std::rc::Rc::new(SmeltStoreCell::lazy(SmeltFieldStore::default(), Box::new(build), |store, entries| { for (key, value) in entries { store.insert(key, value); } })) } }";

/// The erased object for a reference record whose snapshot waits for first use.
///
/// `id_text` is the record's stable identity, `build_body` the statements that
/// fill `smelt_object_entries` from `smelt_object_value`/`smelt_struct_value`
/// (the same text the eager view runs), and `handle` the live handle, which is
/// both captured by the builder and stamped as the origin.
pub(crate) fn lazy_reference_object_text(id_text: &str, build_body: &str, handle: &str) -> String {
    format!(
        "SmeltObject::lazy_with_id({id_text}, {{ let smelt_lazy_handle = {handle}.clone(); move || {{ let smelt_object_value = smelt_lazy_handle; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::new(); {build_body}smelt_object_entries }} }}).smelt_with_origin({handle}.clone())"
    )
}
