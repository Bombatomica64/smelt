//! Runtime support for reading a JavaScript builtin's members as values.
//!
//! `Array.prototype.slice` is a property read, not a call: JavaScript answers it
//! with a function object, and code stores that object, probes it with `typeof`,
//! reads its `length`, and eventually invokes it through `.call`/`.apply`. The
//! generated runtime therefore needs three things, all emitted here:
//!
//! * a value for `<Builtin>.prototype` — an interned marker record, so the
//!   member read has somewhere to land and `Array.prototype === Array.prototype`
//!   holds;
//! * one interned callable per modeled `(builtin, kind, member)` triple, carrying
//!   the member's JavaScript `length` and a stable identity;
//! * a dispatcher that applies the member to its receiver.
//!
//! The set of modeled members is [`smelt_stdlib::BUILTIN_MEMBER_FUNCTIONS`] — the
//! same registry the arity comes from — so a member Smelt cannot run is simply
//! absent and reads as `undefined` rather than becoming a callable that lies.
//!
//! A prototype method consumes its receiver as the leading argument, which is
//! what `Function.prototype.call`/`.apply` must feed it. Those two are generic
//! over any callable, so the receiver-consuming callables register themselves in
//! a side table (`smelt_register_receiver_method`) and `smelt_function_method`
//! consults it instead of unconditionally dropping the receiver.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


