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
//! value in the erased object's own `origin` slot (`smelt_with_host_origin`),
//! and the recovery hands that value back. The slot lives in the object's
//! store, so the retained value lives exactly as long as some erased view of
//! it does. Reconstruction never enters into it; the only
//! question a type has to answer is what a record that did NOT come from an
//! erasure should mean, which is [`Recovery`].

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible recovery types and helpers are used by the emitter, outside this private module"
)]

