//! Generated runtime for the JavaScript `Date` class.
//!
//! # `SmeltDate`
//!
//! A `Date` is a mutable reference object: `d.setDate(1)` changes the date
//! every alias of `d` sees, `a === b` asks whether two dates are the same
//! object, and `new Date(d)` makes a fresh one. `SmeltDate` is therefore a
//! handle on one shared cell holding the time value — milliseconds since the
//! epoch as an `f64`, `NaN` for an Invalid Date, exactly the value the
//! specification stores — and cloning the handle aliases, never copies.
//!
//! The date arithmetic itself (getters, setters, constructors, formatting)
//! stays in the emitter's timestamp templates (`emitter/strings_io.rs`): they
//! read the handle's time value, and a setter writes its result back into the
//! same cell (`SmeltDate::set_time`).
//!
//! # Dynamic boundary
//!
//! An erased `Date` is the marker record `{ "__smelt_date": <time value> }`,
//! the shape the erased date paths (and a time-zone context's
//! `__smelt_timezone` metadata) already read. Erasing keeps the live handle in
//! the record's origin slot, so narrowing the record back hands out the SAME
//! date. A record that did not come from an erasure — a hand-built marker, a
//! number, a string — is rebuilt as a fresh date from the value it carries.
//! These adapters are the only place a `SmeltDate` meets `SmeltUnknown`.

#![expect(
    clippy::redundant_pub_crate,
    reason = "the prelude emitter is called from the crate root"
)]

