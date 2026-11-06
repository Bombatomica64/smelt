//! Runtime support for JavaScript function *objects* and `[[Construct]]`.
//!
//! A JavaScript function is an object. Three consequences the generated runtime
//! has to answer, all emitted here:
//!
//! * **A function has own properties.** `f.prototype` is one of them, and source
//!   code writes others (`partialed.prototype = …`, `curried.placeholder = …`).
//!   An `Rc<dyn Fn(..)>` has nowhere to put them, so they live in a bag on the
//!   function's *canonical identity* token — the identity
//!   `smelt_same_function_identity` compares and `Function.prototype.length`
//!   is stored on (see `callable_registry`). Carrying the bag on the identity
//!   rather than per allocation is what makes a typed callable and every
//!   erasure adapter derived from it share ONE property bag, so a write through
//!   one spelling is visible through all of them.
//!
//! * **`new f(args)` is not `f(args)`.** `[[Construct]]` allocates an object
//!   whose prototype link is `f.prototype`, runs `f` with that object installed
//!   as the receiver, and answers the allocated object unless `f` returned an
//!   object of its own. All three are observable, and none of them happens for a
//!   plain call.
//!
//! * **`x instanceof f` is a prototype-chain walk** (`OrdinaryHasInstance`),
//!   not a nominal test: it follows `x`'s chain link by link looking for the
//!   *object* `f.prototype` by reference. That is why `Object.create(p)` has to
//!   record a real link to `p` and not merely copy `p`'s members. The link lives
//!   in `SMELT_OBJECT_PROTOTYPES`, keyed by the created object's identity (see
//!   `smelt_register_object_prototype` in `lib.rs`); the chain walk here,
//!   `v.__proto__` (`smelt_proto_accessor`) and `Object.getPrototypeOf`
//!   (`smelt_prototype_sentinel`) all read that ONE table, which is what keeps
//!   the three answers consistent.
//!
//! The `prototype` object is materialised on FIRST READ rather than at function
//! creation. Nothing can observe the object before something reads it, so the
//! two are indistinguishable, and closures that are never constructed through
//! (the overwhelming majority) pay no allocation.
//!
//! Known infidelity: an arrow function has no `prototype` in JavaScript, and
//! this runtime cannot tell an arrow from a function expression — MIR carries no
//! arrow flag — so it answers an object for both. Reading `.prototype` off an
//! arrow is the only way to see the difference.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

