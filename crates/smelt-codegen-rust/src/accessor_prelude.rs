//! Record fields stored as accessors (object-literal `get`ters).
//!
//! JavaScript evaluates a getter on every read:
//!
//! ```ts
//! function makeCounter(): Counter {
//!   let value: number | undefined;
//!   return { bump: () => { value = (value ?? 0) + 1; }, get cachedValue() { return value; } };
//! }
//! ```
//!
//! `counter.cachedValue` answers `undefined` before `bump()` and `1` after it.
//! A struct field holding the getter's value at construction froze it at the
//! first value. A hand port stores the getter and calls it on each read, so a
//! record field that some construction site supplies through a getter
//! (`smelt_hir::Crate::accessor_fields`, decided by the frontend) is stored as
//! a `SmeltAccessor<T>` ([`ACCESSOR_TYPE`]) instead of a `T`:
//!
//! * construction from the literal hands the getter closure to
//!   `SmeltAccessor::new`; any other construction (an adapter rebuilding the
//!   record, a `Default`) supplies a plain value through
//!   `SmeltAccessor::constant`;
//! * every read of the field is `.get()`, which runs the getter now —
//!   a direct read, a destructuring, a spread (which copies the current value,
//!   as JavaScript's spread does) and the erasure into `SmeltUnknown` (the
//!   getter is evaluated at erase time).
//!
//! Records no literal supplies through a getter keep plain fields.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are called from the emitter shards, outside this private module"
)]

use crate::rust::RustExpr;
use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::Mir;


/// The generated accessor storage type.
pub(crate) const ACCESSOR_TYPE: &str = "SmeltAccessor";

/// Whether field `field` of the record named `owner` is stored as an accessor.
pub(crate) fn is_accessor_field(mir: &Mir, owner: Symbol, field: Symbol) -> bool {
    mir.accessor_fields.contains(&(owner, field))
}

/// Whether field `field` of a value of record type `ty` is an accessor.
pub(crate) fn is_accessor_field_of_type(mir: &Mir, ty: TypeId, field: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::Class { name, .. }) => is_accessor_field(mir, *name, field),
        Some(Type::Optional(inner)) => is_accessor_field_of_type(mir, *inner, field),
        _ => false,
    }
}

/// The storage type text of an accessor of `value_ty_text`.
pub(crate) fn storage_type_text(value_ty_text: &str) -> String {
    format!("{ACCESSOR_TYPE}<{value_ty_text}>")
}

/// Wraps a plain value as an accessor that answers it.
pub(crate) fn constant_expr(value_text: &RustExpr) -> RustExpr {
    RustExpr::atom(format!("{ACCESSOR_TYPE}::constant({value_text})"))
}

/// Wraps a getter closure (`Rc<dyn Fn() -> T>`) as an accessor.
pub(crate) fn getter_expr(getter_expr: &RustExpr) -> RustExpr {
    RustExpr::atom(format!("{ACCESSOR_TYPE}::new({getter_expr})"))
}

/// Reads an accessor field: runs the getter.
pub(crate) fn read_expr(field_text: &RustExpr) -> RustExpr {
    RustExpr::postfix_on(&field_text, format!("{field_text}.get()"))
}
