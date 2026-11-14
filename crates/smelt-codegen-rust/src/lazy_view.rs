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
//! slot is read and written without building the snapshot (`origin` /
//! `origin_mut`), because the origin is exactly the part of the view that does
//! not depend on it. That convention is enforced by the cell's surface: the
//! unbuilt store is reachable ONLY as its `origin` slot, so no caller can read
//! the entries of a view whose snapshot is still pending.
//!
//! `pending` is taken out of its `Cell` before the builder runs, so a builder
//! that reads this same store again (a snapshot of a value whose fields reach
//! the view) sees nothing pending instead of building twice.
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

use crate::rust::RustExpr;

/// The erased object for a reference record whose snapshot waits for first use.
///
/// `id_text` is the record's stable identity, `build_body` the statements that
/// fill `smelt_object_entries` from `smelt_object_value`/`smelt_struct_value`
/// (the same text the eager view runs), and `handle` the live handle, which is
/// both captured by the builder and stamped as the origin.
pub(crate) fn lazy_reference_object_expr(
    id_text: &RustExpr,
    build_body: &str,
    handle: &str,
) -> RustExpr {
    RustExpr::atom(format!(
        "SmeltObject::lazy_with_id({id_text}, {{ let smelt_lazy_handle = {handle}.clone(); move || {{ let smelt_object_value = smelt_lazy_handle; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::new(); {build_body}smelt_object_entries }} }}).smelt_with_origin({handle}.clone())",
        id_text = id_text.delimited()
    ))
}
