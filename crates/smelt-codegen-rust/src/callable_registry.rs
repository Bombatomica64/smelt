//! Bounded identity side tables for erased callables and reference records.
//!
//! # What the tables answer
//!
//! An `Rc<dyn Fn(..)>` is only a code pointer and its captures, but a
//! JavaScript function is an object: it has an identity that survives being
//! wrapped (`f === f` across an erasure adapter), a `length`, own properties,
//! and — when it crossed an erased ABI — a typed callable it came from or an
//! erased callable object it was narrowed out of. The generated runtime keeps
//! that metadata beside the allocation, keyed by the `Rc`'s ADDRESS
//! ([`SmeltAddressTable`]), because both representations of one function (the
//! typed `Rc<dyn Fn(A) -> R>` and the erased `SmeltUnknown::Function`) have to
//! reach it and neither can carry a field.
//!
//! # Why the tables are bounded (the leak this module closes)
//!
//! The registries this replaces (`SMELT_CALLABLE_KEY_GUARDS`,
//! `SMELT_FUNCTION_ORIGINS`, `SMELT_FUNCTION_IDENTITIES`,
//! `SMELT_FUNCTION_LENGTHS`, `SMELT_CALLABLE_OBJECTS`,
//! `SMELT_ERASED_FUNCTION_VALUES`, `SMELT_FUNCTION_PROPERTIES`,
//! `SMELT_FUNCTION_CLASSES`, `SMELT_RECEIVER_METHODS`,
//! `SMELT_REFERENCE_OBJECT_IDENTITIES`) were nine separate maps that never
//! removed an entry: there is no drop hook on an `Rc` to remove it from. A
//! request in the Hono showcase erases ~20 callables, so a server leaked
//! ~16 KB per request and reached 10 GiB. Sweeping dead keys was tried and
//! reclaimed little, because the maps kept their own keys alive: the typed
//! origin of an erased adapter, the callable object a typed callback was
//! narrowed from, and the host values in the (separate) host-origin map all
//! held strong references back into the graph the keys belong to.
//!
//! The design here makes the life of every entry equal to the life of the
//! allocation it describes:
//!
//! * **One entry per allocation, owning its guard.** Each entry stores a
//!   `Weak` to the allocation it is keyed by. A `Weak` keeps the `RcBox`
//!   allocated after the value is dropped, so the address cannot be handed to
//!   a new allocation while the entry exists — the address-reuse guarantee
//!   that fixed remeda's flaky lazy `pipe` (a fresh `map(cb)` evaluator landed
//!   on a dead callback's address and inherited its callable-object entry).
//!   Because the guard and the metadata are ONE entry, they are removed
//!   together: once removed, a recycled address finds nothing, never a stale
//!   entry.
//! * **Dead entries are swept, amortized.** An insert that finds the table at
//!   its sweep threshold first removes every entry whose `Weak` is dead, then
//!   resets the threshold to a fixed multiple of the live count. Each entry is examined
//!   O(1) times on average, and the table stays within a constant factor of
//!   the number of LIVE allocations it describes.
//! * **Identity is a shared token, not an address.** Every allocation that
//!   denotes one JavaScript function holds the same [`SmeltFunctionIdentity`]
//!   (an `Rc`). The token's address is the identity number `===` compares,
//!   and it can only be recycled once no entry holds the token — that is,
//!   once nothing that could be compared against it is alive. The token also
//!   carries the function's `length` and own-property bag, so those die with
//!   the last representation of the function instead of being keyed by a
//!   number in a map of their own.
//! * **No entry may own a path back to its key.** A value stored in an entry
//!   that strongly reaches the entry's own allocation would keep the
//!   allocation, and so the entry, alive forever. The erasure adapters are
//!   arranged so that never happens (see `smelt_register_callable_object`'s
//!   callers in `emitter/coercion.rs`). A JavaScript-level cycle — a function
//!   whose own `prototype.constructor` is itself — is still a cycle, exactly
//!   as it is for any `Rc` graph in the generated runtime.
//!
//! # Why not carry the metadata inline on the erased value
//!
//! The alternative (status report §6.1) was an erased callable as a struct
//! carrying origin/identity/length. That removes the table for ERASED
//! callables only: typed callbacks (`Rc<dyn Fn(A) -> R>` in struct fields and
//! parameters, the shape a hand-written port uses) still need identity and
//! callable-object metadata, so a side table would remain, and every one of
//! the ~200 `SmeltUnknown::Function` construction and match sites would change
//! ABI. Tying each entry's life to its allocation bounds the memory for BOTH
//! representations through one mechanism and keeps both ABIs as they are.
//! Host values do go inline: they already had a slot to go to (the erased
//! object store's `origin`, see `reference_origin`).

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the prelude emitter"
)]

