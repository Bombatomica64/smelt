//! Runtime model of `Array.prototype` / `String.prototype` methods on erased values.
//!
//! **Dynamic boundary.** Everything here operates on `SmeltUnknown` because it
//! runs exactly where the static shape is gone: a method call whose receiver is
//! typed `any` (or is a property read off one, a type parameter, an erased
//! class) is lowered as a property read through `smelt_get_unknown_field`
//! followed by the erased call ABI. Which prototype answers the read is decided
//! by the receiver's RUNTIME tag, as JavaScript's own property lookup decides
//! it, so no concrete type, generated union or scoped generic can stand in for
//! the receiver at the emit site: the same `value.slice(1)` is a list slice for
//! one caller and a string slice for the next. A statically typed receiver
//! never reaches this module — typed lists and strings lower to their own
//! typed operations before codegen ever sees an erased read (see
//! `emitter/place.rs`, whose erased-read arm is gated on the receiver's type).
//!
//! What the module emits:
//!
//! * `smelt_array_prototype_member` / `smelt_string_prototype_member` — the
//!   member READ: a method name from the shared tables in
//!   [`smelt_stdlib::erased_prototype_methods`] becomes a callable bound to the
//!   receiver, anything else is `None` (so the caller falls back to
//!   `undefined`). The bound callable captures the receiver handle, and an
//!   erased array's handle shares its storage, so `push`/`sort`/`splice`
//!   mutate the caller's array exactly as JavaScript does.
//! * `smelt_array_prototype_apply` / `smelt_string_prototype_apply` — the
//!   member CALL, one arm per method, with JavaScript's argument rules:
//!   callbacks receive `(element, index, array)` (`reduce` receives the
//!   accumulator first), relative positions count from the end, a missing
//!   callback throws `TypeError: undefined is not a function`, and an empty
//!   `reduce` without an initial value throws its `TypeError`.
//! * the small coercion helpers those arms share (`ToBoolean`,
//!   `ToIntegerOrInfinity`, `ToString`, position clamping) and the `TypeError`
//!   thrower the erased call ABI also uses for a non-callable callee
//!   (`smelt_throw_not_callable`).
//!
//! Two deliberate simplifications, both documented at their arms: a bound
//! method read does not register a canonical function identity or `length`
//! (each read would otherwise grow the process-wide identity registries by
//! one entry per CALL, since every `arr.map(f)` performs a read), and string
//! positions count Unicode scalar values, the same unit every other erased
//! string operation in the prelude (`length`, indexing) already uses.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


