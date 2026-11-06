//! Runtime prelude for `node:events`' `EventEmitter`.
//!
//! An emitter is an insertion-ordered list of listeners, each tagged with its
//! event name, a `once` flag, and its function identity (so `off` can find it).
//! The list lives behind an `Rc<RefCell<..>>` with a `smelt_next_object_id`
//! identity, in the same shape as the other reference objects: two variables
//! holding the same emitter observe each other's registrations.
//!
//! # Why the listener store is erased
//!
//! **Dynamic boundary.** A listener's signature is not knowable from the event
//! name — `on('data', cb)` takes a chunk and `on('end', cb)` takes nothing —
//! and `emit(name, ...args)` passes an arbitrary positional list decided by the
//! emitting site, not by the emitter's type. One emitter holds listeners for
//! many events at once, so the store is heterogeneous and keyed by a runtime
//! string. No concrete type, generated union, or scoped generic can express
//! that: the callback set is only known at run time, on the branch that
//! registered it. The store is therefore the existing erased callable ABI.
//!
//! # `emit` iterates a snapshot
//!
//! Measured against Node 22 rather than read from the docs, because two of
//! these are easy to get wrong and invisible to any compile step:
//!
//! * a listener **added** during an emit does NOT run in that emit;
//! * a listener **removed** during an emit **still runs** in it.
//!
//! Together those say `emit` takes a snapshot of the matching listeners and
//! calls that, rather than walking the live list. A `once` listener is removed
//! from the live list *before* its call, so a re-entrant `emit` cannot run it
//! twice.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

