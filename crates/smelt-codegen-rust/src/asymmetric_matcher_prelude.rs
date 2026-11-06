//! Runtime prelude for vitest *asymmetric matchers*.
//!
//! An asymmetric matcher — `expect.any(Number)`, `expect.arrayContaining([..])`,
//! `expect.objectContaining({..})`, `expect.stringContaining("x")`,
//! `expect.stringMatching(/x/)`, `expect.closeTo(n, p)`, `expect.anything()`,
//! and every one of those under `expect.not` — is a **value**, not an
//! assertion. Vitest types these factories `any`; what makes the value a
//! matcher is that the deep equality behind `toEqual` / `toStrictEqual` /
//! `toHaveBeenCalledWith` *asks* it whether the actual value matches, instead
//! of comparing it structurally. So a matcher can be stored in a variable,
//! nested inside an expected object or array literal, or handed over as one
//! argument of `toHaveBeenCalledWith`.
//!
//! The frontend lowers one to a branded record — `{ __smelt_asymmetric:
//! <kind>, sample: [<factory arguments>], inverted: <bool> }` — using the same
//! convention as `__smelt_vitest_mock`, `__smelt_map` and `__smelt_set`: the
//! value's kind travels with it, so a runtime helper recognizes it wherever it
//! ended up. This module emits the two halves that give the record meaning:
//!
//! * [`MATCH_FN`] — the per-kind predicate.
//! * [`EQUALS_FN`] — the deep-equality walk that consults the brand on **either**
//!   side, at **every** level, which is what makes a nested matcher work.
//!
//! # Why this is not `PartialEq for SmeltUnknown`
//!
//! `smelt_unknown_structural_eq` backs `PartialEq for SmeltUnknown`, and
//! transpiled library code observes that operator — es-toolkit's own `isEqual`
//! among other things. Teaching it about `__smelt_asymmetric` would let a
//! test-harness marker change what a transpiled predicate answers about an
//! ordinary object. The walk here is therefore a separate function, and when
//! neither operand holds a matcher anywhere it delegates to the structural one
//! unchanged, so an ordinary `toEqual` compares exactly as it did before.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

