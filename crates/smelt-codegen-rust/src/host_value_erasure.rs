//! The erasure adapter pair for a generated host runtime type.
//!
//! # Why a shared emitter
//!
//! A modeled class backed by a generated runtime type cannot use the generic
//! struct erasure. That path reads DECLARED FIELDS and stamps `__smelt_class`,
//! and a prelude type has no declared fields — so it produced a record with no
//! state and no host identity, which is why `x instanceof TextEncoder` on an
//! erased codec could not be answered at all.
//!
//! `Headers` and `Blob` each hand-wrote their own adapter, and the shape they
//! wrote is the same one every such type needs: the class's identity marker,
//! the spec's readable data properties, and the value's own object id so two
//! erasures of one value compare `===` equal. Nine types in, the shape is worth
//! having once.
//!
//! # Identity, not reconstruction
//!
//! `Headers` and `Blob` round-trip STRUCTURALLY: their record carries the
//! header pairs or the bytes, so a recovery can rebuild an equal value from it.
//! The text codecs, the `node:events` emitter and the `node:http` types cannot
//! — their state is closures, cells and a tokio shutdown sender, so there is
//! nothing to rebuild from.
//!
//! What JavaScript does there is not rebuild anything. Erasing a value and
//! narrowing it back yields the SAME object, so
//!
//! ```js
//! const x = emitter;        // widened to unknown by the type system
//! (x as EventEmitter).on("data", f);   // the same listener list
//! ```
//!
//! must reach the emitter's own listeners. So the erasure RETAINS the live
//! value in `SMELT_HOST_ORIGINS` under the record's object id, and the recovery
//! hands that value back. Reconstruction never enters into it; the only
//! question a type has to answer is what a record that did NOT come from an
//! erasure should mean, which is [`Recovery`].

use crate::rust::CodeWriter;

/// What a recovery answers for a record with no retained origin.
///
/// A record reaches the recovery without an origin when it did not come from an
/// erasure in this thread — a hand-built object carrying the marker, or one
/// rebuilt from JSON. Each type decides for itself, because the honest answer
/// differs and guessing one for all of them is how a wrong value gets in.
#[derive(Clone, Copy)]
pub enum Recovery {
    /// Recover losslessly: the record carries everything the value is.
    ///
    /// The expression builds the value from the record, so the origin registry
    /// is only an identity optimization here rather than the whole mechanism.
    /// Only the text codecs qualify — a codec's state besides its reference
    /// identity IS its encoding label.
    Structural(&'static str),
    /// Answer a fresh EMPTY value, and say so in the generated doc comment.
    ///
    /// Sound where "empty" is a real state of the type — an emitter with no
    /// listeners, a response nothing has been written to — and where the
    /// alternative would be to fabricate state that was never there.
    Empty(&'static str),
    /// Do not emit a recovery at all.
    ///
    /// For a type with no honest empty value. A `node:http` `Server` is a live
    /// listening socket with a handler closure and a shutdown sender; there is
    /// no such thing as an empty one, so narrowing an erased server stays a
    /// named blocker instead of producing a server that is not listening.
    /// `StdlibClass::narrows_from_erased` is the other half of this decision.
    None,
}

/// One readable data property on the erased record.
///
/// `name` is the JavaScript property name; `value_expr` is a Rust expression
/// over `self` producing a `SmeltUnknown`.
pub struct DataProperty {
    /// The JavaScript property name.
    pub name: &'static str,
    /// A Rust expression over `self` yielding the property's erased value.
    pub value_expr: &'static str,
}

/// Emit the `IntoSmeltUnknown` / `SmeltFromUnknown` pair for a host type.
///
/// The record is built with the value's OWN `id`, so erasing one value twice
/// yields two `===`-equal objects, and the marker is what makes the whole
/// record non-enumerable in `for...in` and what `instanceof` resolves through.
/// Both come from the shared host-object registry, so this emitter and
/// `instance_of_text` cannot disagree about a class's identity.
pub fn emit_adapters(
    writer: &mut CodeWriter,
    type_name: &str,
    marker: &str,
    data_properties: &[DataProperty],
    recovery: Recovery,
) {
    let entries = std::iter::once(format!(
        "(\"{marker}\".to_owned(), SmeltUnknown::Bool(true))"
    ))
    .chain(data_properties.iter().map(|property| {
        format!(
            "(\"{}\".to_owned(), {})",
            property.name, property.value_expr
        )
    }))
    .collect::<Vec<_>>()
    .join(", ");

    writer.line(format!("/// Erase a `{type_name}` for a dynamic boundary."));
    writer.line("///");
    writer.line("/// Retains the live value under the record's object id, so narrowing the");
    writer.line("/// erased value back hands out the SAME object rather than a copy.");
    writer.line(format!(
        "impl IntoSmeltUnknown for {type_name} {{ fn into_smelt_unknown(self) -> SmeltUnknown {{ let smelt_id = self.id; smelt_register_host_origin(smelt_id, self.clone()); SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([{entries}]))) }} }}"
    ));
    writer.blank_line();

    let fallback = match recovery {
        Recovery::None => return,
        Recovery::Structural(expr) => {
            writer.line(format!("/// Recover a `{type_name}` from an erased value."));
            writer.line("///");
            writer.line("/// Lossless: the record carries everything the value is, so a record");
            writer.line("/// that did not come from an erasure still recovers correctly.");
            expr
        }
        Recovery::Empty(expr) => {
            writer.line(format!("/// Recover a `{type_name}` from an erased value."));
            writer.line("///");
            writer.line("/// A record with no retained origin did not come from an erasure in");
            writer.line("/// this thread, and this type's state cannot be rebuilt from a record,");
            writer.line("/// so it answers a fresh EMPTY value — which is a real state of the");
            writer.line("/// type, unlike any state that could be fabricated for it.");
            expr
        }
    };
    writer.line(format!(
        "impl SmeltFromUnknown for {type_name} {{ fn smelt_from_unknown(value: SmeltUnknown) -> Self {{ smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| {fallback}) }} }}"
    ));
    writer.blank_line();
}
