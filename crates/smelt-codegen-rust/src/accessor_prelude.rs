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

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::Mir;

use crate::rust::CodeWriter;

/// The generated accessor storage type.
pub(crate) const ACCESSOR_TYPE: &str = "SmeltAccessor";

/// Whether any record field is stored as an accessor.
pub(crate) fn needed(mir: &Mir) -> bool {
    !mir.accessor_fields.is_empty()
}

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
pub(crate) fn constant_text(value_text: &str) -> String {
    format!("{ACCESSOR_TYPE}::constant({value_text})")
}

/// Wraps a getter closure (`Rc<dyn Fn() -> T>`) as an accessor.
pub(crate) fn getter_text(getter_text: &str) -> String {
    format!("{ACCESSOR_TYPE}::new({getter_text})")
}

/// Reads an accessor field: runs the getter.
pub(crate) fn read_text(field_text: &str) -> String {
    format!("{field_text}.get()")
}

/// Emits the accessor storage type.
///
/// With the erased runtime present (`with_unknown`), an accessor also
/// crosses the `SmeltUnknown` boundary: erasing it reads the getter now, and
/// recovering one from an erased value answers that value on every read (the
/// erased object no longer carries a live getter for a record field).
pub(crate) fn emit_prelude(writer: &mut CodeWriter, with_unknown: bool) {
    if with_unknown {
        writer.line(format!(
            "impl<T: IntoSmeltUnknown + 'static> IntoSmeltUnknown for {ACCESSOR_TYPE}<T> {{ fn into_smelt_unknown(self) -> SmeltUnknown {{ self.get().into_smelt_unknown() }} }}"
        ));
        writer.line(format!(
            "impl<T: SmeltFromUnknown + Clone + 'static> SmeltFromUnknown for {ACCESSOR_TYPE}<T> {{ fn smelt_from_unknown(value: SmeltUnknown) -> Self {{ Self::constant(T::smelt_from_unknown(value)) }} }}"
        ));
    }
    writer.line("/// A record field stored as a getter: every read runs it.");
    writer.line(format!(
        "struct {ACCESSOR_TYPE}<T>(::std::rc::Rc<dyn Fn() -> T>);"
    ));
    writer.line(format!(
        "impl<T: 'static> {ACCESSOR_TYPE}<T> {{ fn new(getter: ::std::rc::Rc<dyn Fn() -> T>) -> Self {{ Self(getter) }} fn get(&self) -> T {{ (self.0)() }} }}"
    ));
    writer.line(format!(
        "impl<T: Clone + 'static> {ACCESSOR_TYPE}<T> {{ fn constant(value: T) -> Self {{ Self(::std::rc::Rc::new(move || value.clone())) }} }}"
    ));
    writer.line(format!(
        "impl<T> Clone for {ACCESSOR_TYPE}<T> {{ fn clone(&self) -> Self {{ Self(self.0.clone()) }} }}"
    ));
    writer.line(format!(
        "impl<T: Default + Clone + 'static> Default for {ACCESSOR_TYPE}<T> {{ fn default() -> Self {{ Self::constant(T::default()) }} }}"
    ));
    writer.line(format!(
        "impl<T: ::std::fmt::Debug + 'static> ::std::fmt::Debug for {ACCESSOR_TYPE}<T> {{ fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{ self.get().fmt(formatter) }} }}"
    ));
    writer.line(format!(
        "impl<T: PartialEq + 'static> PartialEq for {ACCESSOR_TYPE}<T> {{ fn eq(&self, other: &Self) -> bool {{ self.get() == other.get() }} }}"
    ));
}
