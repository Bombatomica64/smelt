//! Spelling of Python `int` values ([`smelt_hir::Type::PyInt`]) in generated Rust.
//!
//! A Python `int` is unbounded, so it is carried as the runtime's
//! [`PY_INT_TYPE`] (`SmeltPyInt`, see `smelt_runtime`'s `py_int` family): a
//! machine word on the fast path and a `num_bigint::BigInt` once a value leaves
//! `i64`. This module holds the type's name and the spelling of its constants;
//! the emitter's operator, conversion and printing rules for it live with the
//! rest of each family (`emitter::py_int`).
//!
//! Behavioural requirement: every value is exact at every magnitude, as in
//! `CPython`. Type-preservation requirement: a Python `int` is never spelled
//! `i64`, `f64` or `SmeltUnknown`; only an explicit conversion (`float(n)`, a
//! sequence index, a crossing into TypeScript) leaves `SmeltPyInt`.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the emitter shards"
)]

use crate::rust::RustExpr;

/// The Rust type a Python `int` is carried as.
pub(crate) const PY_INT_TYPE: &str = "SmeltPyInt";

/// Renders the Python `int` constant whose canonical decimal digits are
/// `digits`.
///
/// A value in `i64` range is the small arm itself, which allocates nothing; a
/// wider one is built from its digits, which the frontend already validated
/// (an optional `-` and ASCII digits).
pub(crate) fn literal_expr(digits: &str) -> RustExpr {
    if let Ok(small) = digits.parse::<i64>() {
        RustExpr::chain(format!("{PY_INT_TYPE}::Small({small})"))
    } else {
        RustExpr::chain(format!("{PY_INT_TYPE}::from_literal_digits({digits:?})"))
    }
}

/// The type a raising Python numeric operation answers, given the interned
/// id of [`smelt_hir::Type::PyInt`] (a `range` answers a list of them).
pub(crate) const fn py_numeric_op_result(
    op: smelt_hir::PyNumericOp,
    py_int: smelt_hir::TypeId,
) -> smelt_hir::Type {
    use smelt_hir::{PyNumericOp, Type};
    match op {
        PyNumericOp::Parse
        | PyNumericOp::ParseBase
        | PyNumericOp::FromFloat
        | PyNumericOp::FloorDiv
        | PyNumericOp::Mod
        | PyNumericOp::Pow
        | PyNumericOp::PowMod
        | PyNumericOp::Shl
        | PyNumericOp::Shr
        | PyNumericOp::RoundToInt
        | PyNumericOp::RoundInt => Type::PyInt,
        PyNumericOp::ToFloat
        | PyNumericOp::TrueDiv
        | PyNumericOp::PowFloat
        | PyNumericOp::FloatPow
        | PyNumericOp::ParseFloat
        | PyNumericOp::RoundFloat => Type::Float,
        PyNumericOp::ToIndex | PyNumericOp::ToSize | PyNumericOp::Ord => Type::Int,
        PyNumericOp::Chr => Type::PyStr,
        PyNumericOp::Range => Type::List(py_int),
    }
}
