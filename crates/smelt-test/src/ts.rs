//! Vitest/Jest-style helpers for generated Rust tests.
//!
//! Rust cannot expose methods named exactly like `toBe` or `toEqual` in an
//! idiomatic way, so codegen maps those public source APIs to snake-case helper
//! methods with matching assertion semantics.

use std::{fmt, panic::UnwindSafe};

use crate::{SameValue, catches_panic, fail};

/// Starts a Vitest/Jest-style value expectation.
#[must_use]
pub const fn expect<T>(actual: T) -> Expect<T> {
    Expect {
        actual,
        inverted: false,
    }
}

/// Starts an expectation for a callable used with `toThrow`.
#[must_use]
pub const fn expect_fn<F>(function: F) -> ThrowExpectation<F>
where
    F: FnOnce() + UnwindSafe,
{
    ThrowExpectation {
        function,
        inverted: false,
    }
}

/// Assertion builder for `expect(value)` calls.
#[derive(Debug)]
pub struct Expect<T> {
    /// The received value under test.
    actual: T,
    /// Whether the assertion is negated through `.not`.
    inverted: bool,
}

impl<T> Expect<T> {
    /// Returns a negated assertion builder, matching Vitest/Jest `.not`.
    #[must_use]
    #[expect(
        clippy::should_implement_trait,
        reason = "generated Vitest/Jest tests need a method shaped like the source `.not` API"
    )]
    pub fn not(self) -> Self {
        Self {
            actual: self.actual,
            inverted: !self.inverted,
        }
    }

    /// Asserts `toBe` using JavaScript `Object.is` semantics.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_be<U>(&self, expected: U)
    where
        T: SameValue<U> + fmt::Debug,
        U: fmt::Debug,
    {
        let matched = self.actual.same_value(&expected);
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to be {expected:?}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to be {expected:?}",
                actual = self.actual
            ),
        );
    }

    /// Asserts `toEqual` using structural Rust equality.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_equal<U>(&self, expected: U)
    where
        T: PartialEq<U> + fmt::Debug,
        U: fmt::Debug,
    {
        let matched = self.actual == expected;
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to equal {expected:?}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to equal {expected:?}",
                actual = self.actual
            ),
        );
    }

    /// Asserts `toStrictEqual` using structural Rust equality for v1.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_strict_equal<U>(&self, expected: U)
    where
        T: PartialEq<U> + fmt::Debug,
        U: fmt::Debug,
    {
        let matched = self.actual == expected;
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to strictly equal {expected:?}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to strictly equal {expected:?}",
                actual = self.actual
            ),
        );
    }

    /// Asserts `toBeTruthy` for generated boolean expressions.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_be_truthy(&self)
    where
        T: Copy + Into<bool> + fmt::Debug,
    {
        let matched = self.actual.into();
        self.assert_match(
            matched,
            format_args!("expected {actual:?} to be truthy", actual = self.actual),
            format_args!("expected {actual:?} not to be truthy", actual = self.actual),
        );
    }

    /// Asserts `toBeFalsy` for generated boolean expressions.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_be_falsy(&self)
    where
        T: Copy + Into<bool> + fmt::Debug,
    {
        let matched = !self.actual.into();
        self.assert_match(
            matched,
            format_args!("expected {actual:?} to be falsy", actual = self.actual),
            format_args!("expected {actual:?} not to be falsy", actual = self.actual),
        );
    }

    /// Asserts `toBeNull` for generated nullable values.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_be_null(&self)
    where
        T: IsNull + fmt::Debug,
    {
        let matched = self.actual.is_null();
        self.assert_match(
            matched,
            format_args!("expected {actual:?} to be null", actual = self.actual),
            format_args!("expected {actual:?} not to be null", actual = self.actual),
        );
    }

    /// Handles positive and negated assertion results.
    fn assert_match(
        &self,
        matched: bool,
        positive_message: fmt::Arguments<'_>,
        negative_message: fmt::Arguments<'_>,
    ) {
        if self.inverted {
            if matched {
                fail(negative_message);
            }
        } else if !matched {
            fail(positive_message);
        }
    }
}

/// Null-checking abstraction for generated TypeScript nullable values.
pub trait IsNull {
    /// Returns true when the value represents JavaScript `null`.
    #[must_use]
    fn is_null(&self) -> bool;
}

impl<T> IsNull for Option<T> {
    fn is_null(&self) -> bool {
        self.is_none()
    }
}

/// Assertion builder for `expect(fn)` calls.
#[derive(Debug)]
pub struct ThrowExpectation<F>
where
    F: FnOnce() + UnwindSafe,
{
    /// The callable expected to throw.
    function: F,
    /// Whether the assertion is negated through `.not`.
    inverted: bool,
}

impl<F> ThrowExpectation<F>
where
    F: FnOnce() + UnwindSafe,
{
    /// Returns a negated throw assertion builder.
    #[must_use]
    #[expect(
        clippy::should_implement_trait,
        reason = "generated Vitest/Jest tests need a method shaped like the source `.not` API"
    )]
    pub fn not(self) -> Self {
        Self {
            function: self.function,
            inverted: !self.inverted,
        }
    }

    /// Asserts `toThrow` by checking whether the callable panics.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_throw(self) {
        let matched = catches_panic(self.function);
        if self.inverted {
            if matched {
                fail("expected function not to throw");
            }
        } else if !matched {
            fail("expected function to throw");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{expect, expect_fn};
    use crate::catches_panic;

    #[test]
    fn to_be_passes_for_same_primitive() {
        expect(42_i32).to_be(42_i32);
    }

    #[test]
    fn to_be_fails_for_different_primitive() {
        assert!(
            catches_panic(|| expect(42_i32).to_be(41_i32)),
            "mismatched primitive should fail"
        );
    }

    #[test]
    fn to_be_uses_object_is_for_nan() {
        expect(f64::NAN).to_be(f64::NAN);
    }

    #[test]
    fn to_be_uses_object_is_for_signed_zero() {
        assert!(
            catches_panic(|| expect(0.0_f64).to_be(-0.0_f64)),
            "signed zeros should differ under toBe"
        );
    }

    #[test]
    fn not_inverts_to_be() {
        expect(42_i32).not().to_be(41_i32);
        assert!(
            catches_panic(|| expect(42_i32).not().to_be(42_i32)),
            "negated matching value should fail"
        );
    }

    #[test]
    fn to_equal_uses_structural_equality() {
        expect(vec![1_i32, 2_i32]).to_equal(vec![1_i32, 2_i32]);
    }

    #[test]
    fn to_strict_equal_uses_structural_equality_for_v1() {
        expect(Some(vec!["a".to_owned()])).to_strict_equal(Some(vec!["a".to_owned()]));
    }

    #[test]
    fn truthy_and_falsy_match_booleans() {
        expect(true).to_be_truthy();
        expect(false).to_be_falsy();
    }

    #[test]
    fn negated_truthy_and_falsy_match_booleans() {
        expect(false).not().to_be_truthy();
        expect(true).not().to_be_falsy();
    }

    #[test]
    fn to_be_null_matches_none() {
        expect(None::<i32>).to_be_null();
        expect(Some(1_i32)).not().to_be_null();
    }

    #[test]
    fn to_throw_matches_panics() {
        expect_fn(|| panic!("expected test panic")).to_throw();
    }

    #[test]
    fn to_throw_fails_without_panic() {
        assert!(
            catches_panic(|| expect_fn(|| {}).to_throw()),
            "non-panicking function should fail toThrow"
        );
    }

    #[test]
    fn negated_to_throw_matches_non_panics() {
        expect_fn(|| {}).not().to_throw();
    }
}
