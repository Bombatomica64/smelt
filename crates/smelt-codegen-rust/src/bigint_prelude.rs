//! Runtime prelude for JavaScript `bigint` values.
//!
//! A TypeScript `bigint` lowers to [`smelt_hir::Type::BigInt`] and is carried in
//! Rust as [`BIGINT_TYPE`] (`num_bigint::BigInt`): unbounded, like JavaScript's
//! own bigint, so `2n ** 100n` and `9_999_999_999_999_999_999_999n` are exact
//! and nothing ever round-trips through `f64`. `num-bigint` is the crate a Rust
//! team would reach for, and its operators already have the semantics the spec
//! asks for in most places:
//!
//! * `/` truncates toward zero and `%` takes the dividend's sign, exactly as
//!   ECMA-262 `BigInt::divide` / `BigInt::remainder` do;
//! * `&`, `|`, `^` and `!` treat a negative value as an infinite
//!   two's-complement bit string (ECMA-262 `BigIntBitwiseOp`);
//! * `>>` rounds toward negative infinity (`-5n >> 1n` is `-3n`).
//!
//! What this module adds is the part the crate does not decide: the JavaScript
//! THROWS (a zero divisor, a negative exponent, `BigInt(1.5)`, a malformed
//! `BigInt("1.5")`, an out-of-range `asIntN` width), the conversions between a
//! bigint and a `number` or a string, mixed `bigint`/`number` comparisons, and a
//! signed shift count (`x << -2n` is `x >> 2n`).
//!
//! The fallible operations answer `Result<_, Box<dyn Error>>` with the same
//! branded error record a source `throw new RangeError(..)` builds, because each
//! is reached through a fallible builtin call (`Rvalue::BigIntCall`) whose
//! unwind edge can land in a source `catch`. The infallible helpers that can
//! still throw on an absurd input (a shift or power whose result would exceed
//! V8's maximum bigint size) throw through the panic route, the same channel
//! every other rvalue-level throw uses.
//!
//! Emitted only when the crate's type table contains a bigint (see
//! `Need::BigInt`), and then inside the `SmeltUnknown` block,
//! because every throw payload is an erased error record.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::rust::RustExpr;

/// The Rust type a JavaScript `bigint` is carried as.
pub(crate) const BIGINT_TYPE: &str = "::num_bigint::BigInt";

/// Renders the bigint constant whose canonical decimal digits are `digits`.
///
/// A value that fits `i64` is built with `From<i64>`, the spelling a
/// hand-written port uses; anything wider is parsed from its digits, which are
/// already validated by the frontend (only an optional `-` and ASCII digits).
pub(crate) fn literal_expr(digits: &str) -> RustExpr {
    if let Ok(small) = digits.parse::<i64>() {
        RustExpr::atom(format!("{BIGINT_TYPE}::from({small}_i64)"))
    } else {
        RustExpr::atom(format!(
            "{digits:?}.parse::<{BIGINT_TYPE}>().expect(\"bigint literal digits\")"
        ))
    }
}
