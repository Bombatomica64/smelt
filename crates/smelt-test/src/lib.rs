//! Test helpers for Rust tests generated from TypeScript and Python sources.
//!
//! `smelt-test` is the compatibility layer between source-language test APIs
//! and Rust's native test harness. Codegen should emit ordinary `#[test]`
//! functions and call these helpers to preserve the public behavior of common
//! Vitest/Jest and pytest assertions.

use std::{
    any::Any,
    fmt,
    panic::{self, UnwindSafe},
};

pub mod py;
pub mod ts;

/// Compares values with JavaScript `Object.is` semantics.
///
/// Vitest and Jest use this behavior for `toBe`. It differs from plain Rust
/// equality for floating-point values: `NaN` compares equal to itself and
/// positive zero differs from negative zero.
pub trait SameValue<Rhs: ?Sized = Self> {
    /// Returns true when `self` and `other` are the same JavaScript value.
    #[must_use]
    fn same_value(&self, other: &Rhs) -> bool;
}

/// Implements `SameValue` by delegating to Rust equality for scalar types.
macro_rules! impl_same_value_by_eq {
    ($($ty:ty),* $(,)?) => {
        $(
            impl SameValue for $ty {
                fn same_value(&self, other: &Self) -> bool {
                    self == other
                }
            }
        )*
    };
}

impl_same_value_by_eq!(
    bool, char, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, String,
);

impl SameValue for str {
    fn same_value(&self, other: &Self) -> bool {
        self == other
    }
}

impl SameValue for &str {
    fn same_value(&self, other: &Self) -> bool {
        self == other
    }
}

impl<T> SameValue for Option<T>
where
    T: SameValue,
{
    fn same_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(left), Some(right)) => left.same_value(right),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

impl<T> SameValue for Vec<T>
where
    T: SameValue,
{
    fn same_value(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other.iter())
                .all(|(left, right)| left.same_value(right))
    }
}

impl SameValue for f32 {
    fn same_value(&self, other: &Self) -> bool {
        same_f32_bits(*self, *other)
    }
}

impl SameValue for f64 {
    fn same_value(&self, other: &Self) -> bool {
        same_f64_bits(*self, *other)
    }
}

/// Implements the float-specific part of JavaScript `Object.is`.
const fn same_f32_bits(left: f32, right: f32) -> bool {
    if left.is_nan() && right.is_nan() {
        true
    } else {
        left.to_bits() == right.to_bits()
    }
}

/// Implements the double-specific part of JavaScript `Object.is`.
const fn same_f64_bits(left: f64, right: f64) -> bool {
    if left.is_nan() && right.is_nan() {
        true
    } else {
        left.to_bits() == right.to_bits()
    }
}

/// Formats an assertion failure from generated test code.
///
/// # Panics
///
/// Always panics with `message`.
#[expect(
    clippy::panic,
    reason = "generated assertion helpers fail tests by panicking under Rust's native test harness"
)]
pub fn fail(message: impl fmt::Display) -> ! {
    panic!("{message}");
}

/// Runs a closure and returns true when it panics.
pub fn catches_panic(function: impl FnOnce() + UnwindSafe) -> bool {
    panic::catch_unwind(function).is_err()
}

/// Runs a closure and returns the panic message when it panics.
pub fn panic_message(function: impl FnOnce() + UnwindSafe) -> Option<String> {
    panic::catch_unwind(function)
        .err()
        .map(|payload| payload_message(payload.as_ref()))
}

/// Extracts a string representation from a panic payload.
fn payload_message(payload: &(dyn Any + Send)) -> String {
    payload.downcast_ref::<String>().map_or_else(
        || {
            payload.downcast_ref::<&'static str>().map_or_else(
                || "non-string panic payload".to_owned(),
                |message| (*message).to_owned(),
            )
        },
        Clone::clone,
    )
}

#[cfg(test)]
mod tests {
    use super::{SameValue, catches_panic};

    #[test]
    fn same_value_treats_nan_as_equal() {
        assert!(
            f64::NAN.same_value(&f64::NAN),
            "NaN should compare equal under Object.is semantics"
        );
    }

    #[test]
    fn same_value_distinguishes_signed_zero() {
        assert!(
            !0.0_f64.same_value(&-0.0_f64),
            "Object.is distinguishes positive and negative zero"
        );
    }

    #[test]
    fn same_value_compares_vectors_recursively() {
        assert!(
            vec![Some(1_i32), None].same_value(&vec![Some(1_i32), None]),
            "nested values should compare recursively"
        );
    }

    #[test]
    fn catches_panic_reports_panics_without_propagating() {
        assert!(
            catches_panic(|| panic!("expected test panic")),
            "panic should be caught"
        );
        assert!(
            !catches_panic(|| {
                let value = 1_i32;
                assert_eq!(value, 1_i32, "non-panicking closure should run");
            }),
            "non-panicking closure should not be reported as panic"
        );
    }
}
