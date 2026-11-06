//! The concrete typed-array family: byte storage and element views.
//!
//! Increment 1 of `blocker-logs/standards-typed-array-views-plan.md`. Two
//! runtime types, both with a JavaScript reference identity, plus the
//! `SmeltDataView` record, the `SmeltTypedArrayKind` enum and the `SmeltUint8Array`
//! alias:
//!
//! * [`emit_array_buffer`] — `SmeltArrayBuffer`, byte storage that owns its
//!   bytes and interprets none of them (`ArrayBuffer`);
//! * [`emit_typed_array`] — `SmeltTypedArray`, an ELEMENT VIEW over storage: a
//!   kind (element type), a byte offset into the buffer, and an element count.
//!
//! Making the offset and the buffer real is the whole point of the shape. A
//! typed array is not a `Vec<u8>` with a name: `view.subarray(1)` is a second
//! view over the SAME storage, so a write through one is visible through the
//! other, and `view.byteOffset`/`view.buffer` are what the spec's own members
//! answer. A design that copied bytes per view would pass every test that does
//! not share a buffer and silently diverge on every test that does.
//!
//! The kind table is derived from the host-object registry
//! (`smelt_stdlib::TYPED_ARRAY_CLASS_NAMES` plus `typed_array_element`), so the
//! eleven views' names, element widths and `[object X]` tags cannot drift from
//! the erased face's answers for the same eleven classes.
//!
//! ## What is reachable from source today
//!
//! Only the `Uint8` face, under its existing name: `SmeltUint8Array` is emitted
//! as an alias of `SmeltTypedArray`, so `TextEncoder.encode`,
//! `TextDecoder.decode` and `crypto.getRandomValues` keep the value they always
//! had, with the same `length`/`byteLength`/`to_bytes`/`Debug`/`PartialEq`
//! answers. The remaining members exist for increment 3, which is where a
//! source `Uint8Array` annotation stops being erased; until then a
//! `new Uint8Array(..)` is still the erased host record
//! (`byte_buffer_prelude`), and both faces answer through the same registry.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


/// The `SmeltTypedArrayKind` variant expression for a source constructor name.
///
/// `None` for a name that is not one of the eleven views — including
/// `ArrayBuffer`, which is the storage half and has no element kind. Derived
/// from the shared host-object registry, so the emitter kind selection and the
/// emitted enum cannot name different variants for the same class.
#[must_use]
pub(crate) fn kind_expression(class_name: &str) -> Option<String> {
    Some(element_kind_expression(smelt_stdlib::typed_array_element(
        class_name,
    )?))
}

/// The `SmeltTypedArrayKind` variant expression for a registry element type.
///
/// The variant is named by the element's own registry spelling, which is also
/// what a `DataView` accessor's name carries — so a `getInt16` call and a
/// `new Int16Array(..)` reach the same variant without either site spelling it.
#[must_use]
pub(crate) fn element_kind_expression(element: smelt_stdlib::TypedArrayElement) -> String {
    format!("SmeltTypedArrayKind::{}", element.element_name())
}

#[cfg(test)]
mod tests {
    use crate::runtime_prelude::rendered_text;

    /// The runtime with the typed array family on, under the given extra flags.
    fn family_text(extra: &[&str]) -> String {
        rendered_text(&|flag| flag == "byte_array" || extra.contains(&flag))
    }

    /// The kind table answers for the registry's views, not a second list.
    ///
    /// The concrete family and the erased face answer for the same classes, so a
    /// name, a marker or an element width that differed between them would be a
    /// value that changes shape when it crosses the boundary. The runtime spells
    /// the table out as real Rust, which is why this test holds it to the registry.
    #[test]
    fn the_kind_table_is_the_registry() {
        let text = family_text(&["unknown"]);
        for class_name in smelt_stdlib::TYPED_ARRAY_CLASS_NAMES {
            let element = smelt_stdlib::typed_array_element(class_name)
                .expect("a registered view has an element type");
            let marker = smelt_stdlib::host_object_marker(class_name)
                .expect("a registered view has a marker");
            let variant = element.element_name();
            for arm in [
                format!("Self::{variant} => \"{class_name}\""),
                format!("Self::{variant} => \"{marker}\""),
                format!("Self::{variant} => {}", element.byte_width()),
            ] {
                assert!(text.contains(&arm), "{class_name}: the kind table has no `{arm}`");
            }
        }
    }

    /// The emitted family keeps the byte view's name and its shared storage.
    ///
    /// `SmeltUint8Array` is the door every reachable use comes through today
    /// (`TextEncoder.encode`, `TextDecoder.decode`, `crypto.getRandomValues`),
    /// and `over_buffer`/`subarray` are what make a view a VIEW rather than a
    /// copy — the members increment 3 needs and the ones a refactor would most
    /// easily drop.
    #[test]
    fn the_emitted_family_keeps_the_byte_view_and_its_storage() {
        let text = family_text(&["unknown", "data_view"]);
        for expected in [
            "pub type SmeltUint8Array = SmeltTypedArray;",
            "pub fn over_buffer(",
            "pub fn subarray(",
            "pub fn byte_offset(&self) -> f64",
            "pub fn buffer(&self) -> SmeltArrayBuffer",
            "impl IntoSmeltUnknown for SmeltTypedArray",
            "impl SmeltFromUnknown for SmeltTypedArray",
            "impl IntoSmeltUnknown for SmeltArrayBuffer",
        ] {
            assert!(text.contains(expected), "missing `{expected}`");
        }
        // The adapters are gated: a program that never crosses the dynamic
        // boundary does not emit the carrier, so the impls must not be emitted
        // either.
        let typed_only = family_text(&[]);
        assert!(
            !typed_only.contains("impl IntoSmeltUnknown for SmeltTypedArray"),
            "the erasure adapters must stay behind the unknown gate"
        );
    }

    /// `DataView` is pay-for-use on its own, and its accessors are the kind
    /// table's decode/encode reached through a byte-order parameter.
    ///
    /// Most programs that hold a typed array never mention a `DataView`, so
    /// emitting it unconditionally would put ~80 lines into every such crate;
    /// and the accessors must NOT re-derive the widths, which is what the
    /// window reversal buys.
    #[test]
    fn the_data_view_half_is_gated_and_shares_the_kind_table() {
        let with_view = family_text(&["unknown", "data_view"]);
        for expected in [
            "pub struct SmeltDataView",
            "pub fn over_buffer(buffer: &SmeltArrayBuffer, byte_offset: usize, byte_length: Option<usize>) -> Self",
            "pub fn get(&self, kind: SmeltTypedArrayKind, offset: usize, little_endian: bool) -> f64",
            "pub fn set(&self, kind: SmeltTypedArrayKind, offset: usize, value: f64, little_endian: bool)",
            "impl IntoSmeltUnknown for SmeltDataView",
            "impl SmeltFromUnknown for SmeltDataView",
            // The whole point: one definition of each width, reached by
            // reversing the window rather than by a second decode table.
            "if !little_endian { window.reverse(); }",
            "kind.decode(&window, 0)",
        ] {
            assert!(with_view.contains(expected), "missing `{expected}`");
        }
        let without = family_text(&["unknown"]);
        assert!(
            !without.contains("pub struct SmeltDataView"),
            "the `DataView` half must stay behind its own gate"
        );
        // The family's other half is unaffected by that gate.
        assert!(without.contains("pub struct SmeltTypedArray"));
    }

    /// A `SharedArrayBuffer` is the storage type with its species flag set, and
    /// the flag reaches the tag, the marker and `instanceof`.
    ///
    /// A concrete type that answered "ArrayBuffer" for a `SharedArrayBuffer`
    /// would be observably wrong in three places at once, which is why the two
    /// constructors can share a Rust type at all only if the flag is carried.
    #[test]
    fn shared_storage_carries_its_species() {
        let text = family_text(&["unknown"]);
        for expected in [
            "pub fn new_shared(byte_length: usize) -> Self",
            "pub fn is_shared(&self) -> bool { self.shared }",
            "pub fn class_name(&self) -> &'static str { if self.shared { \"SharedArrayBuffer\" } else { \"ArrayBuffer\" } }",
            // The erased face's identity follows the flag too.
            "let marker = if self.shared {",
            // And a view over shared storage reports it back.
            "buffer_shared: buffer.is_shared()",
        ] {
            assert!(text.contains(expected), "missing `{expected}`");
        }
    }
}
