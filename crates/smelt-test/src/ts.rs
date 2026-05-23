//! Vitest/Jest-style helpers for generated Rust tests.
//!
//! Rust cannot expose methods named exactly like `toBe` or `toEqual` in an
//! idiomatic way, so codegen maps those public source APIs to snake-case helper
//! methods with matching assertion semantics.

use std::fmt;

use crate::{SameValue, fail};

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
pub const fn expect_fn<F>(function: F) -> ThrowExpectation<F> {
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

    /// Asserts `toContain` for generated string and collection values.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_contain<U>(&self, expected: U)
    where
        T: Contains<U> + fmt::Debug,
        U: fmt::Debug,
    {
        let matched = self.actual.contains_value(&expected);
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to contain {expected:?}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to contain {expected:?}",
                actual = self.actual
            ),
        );
    }

    /// Asserts `toHaveLength` for generated string and collection values.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_have_length(&self, expected: usize)
    where
        T: HasLength + fmt::Debug,
    {
        let actual_len = self.actual.value_len();
        let matched = actual_len == expected;
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to have length {expected}, got {actual_len}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to have length {expected}",
                actual = self.actual
            ),
        );
    }

    /// Asserts `toHaveProperty` for generated object/map values.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_have_property<K>(&self, key: K)
    where
        T: HasProperty<K> + fmt::Debug,
        K: fmt::Debug,
    {
        let matched = self.actual.has_property(&key);
        self.assert_match(
            matched,
            format_args!(
                "expected {actual:?} to have property {key:?}",
                actual = self.actual
            ),
            format_args!(
                "expected {actual:?} not to have property {key:?}",
                actual = self.actual
            ),
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

/// Containment abstraction for generated TypeScript `toContain` checks.
pub trait Contains<T> {
    /// Returns true when `expected` is contained in `self`.
    #[must_use]
    fn contains_value(&self, expected: &T) -> bool;
}

impl Contains<Self> for String {
    fn contains_value(&self, expected: &Self) -> bool {
        self.contains(expected)
    }
}

impl Contains<&str> for String {
    fn contains_value(&self, expected: &&str) -> bool {
        self.contains(*expected)
    }
}

impl<T> Contains<T> for Vec<T>
where
    T: PartialEq,
{
    fn contains_value(&self, expected: &T) -> bool {
        self.contains(expected)
    }
}

/// Length abstraction for generated TypeScript `toHaveLength` checks.
pub trait HasLength {
    /// Returns the JavaScript-visible length for a value.
    #[must_use]
    fn value_len(&self) -> usize;
}

impl HasLength for String {
    fn value_len(&self) -> usize {
        self.chars().count()
    }
}

impl<T> HasLength for Vec<T> {
    fn value_len(&self) -> usize {
        self.len()
    }
}

/// Property-key abstraction for generated TypeScript `toHaveProperty` checks.
pub trait HasProperty<K> {
    /// Returns true when `key` is present on the value.
    #[must_use]
    fn has_property(&self, key: &K) -> bool;
}

impl<K, V, S> HasProperty<K> for std::collections::HashMap<K, V, S>
where
    K: Eq + std::hash::Hash,
    S: std::hash::BuildHasher,
{
    fn has_property(&self, key: &K) -> bool {
        self.contains_key(key)
    }
}

/// Effect/Vitest-compatible structural equality assertion.
///
/// # Panics
///
/// Panics when the values are not equal.
pub fn deep_strict_equal<T, U>(actual: &T, expected: &U)
where
    T: PartialEq<U> + fmt::Debug,
    U: fmt::Debug,
{
    if actual != expected {
        fail(format_args!(
            "assertion failed: deepStrictEqual\n  actual: {actual:?}\nexpected: {expected:?}"
        ));
    }
}

/// Assertion builder for `expect(fn)` calls.
#[derive(Debug)]
pub struct ThrowExpectation<F> {
    /// The callable expected to throw.
    function: F,
    /// Whether the assertion is negated through `.not`.
    inverted: bool,
}

impl<F> ThrowExpectation<F> {
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

    /// Asserts `toThrow` by checking whether the callable returns an error.
    ///
    /// # Panics
    ///
    /// Panics when the expectation is not satisfied.
    pub fn to_throw<T, E>(self)
    where
        F: FnOnce() -> Result<T, E>,
        E: fmt::Display,
    {
        let matched = (self.function)().is_err();
        if self.inverted {
            if matched {
                fail("expected function not to throw");
            }
        } else if !matched {
            fail("expected function to throw");
        }
    }

    /// Asserts `toThrow(message)` by checking an exception message substring.
    ///
    /// # Panics
    ///
    /// Panics when the callable does not return an error, or when the error
    /// message does not contain `expected`.
    pub fn to_throw_with_message<T, E>(self, expected: &str)
    where
        F: FnOnce() -> Result<T, E>,
        E: fmt::Display,
    {
        let error_text = (self.function)().err().map(|error| error.to_string());
        let matched = error_text
            .as_deref()
            .is_some_and(|text| text.contains(expected));
        if self.inverted {
            if matched {
                fail(format_args!(
                    "expected function not to throw message containing {expected:?}"
                ));
            }
        } else if let Some(text) = error_text {
            if !matched {
                fail(format_args!(
                    "expected thrown message to contain {expected:?}, got {text:?}"
                ));
            }
        } else {
            fail("expected function to throw");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{deep_strict_equal, expect, expect_fn};
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
    fn to_contain_matches_strings_and_vectors() {
        expect("alpha".to_owned()).to_contain("ph");
        expect(vec![1_i32, 2_i32, 3_i32]).to_contain(2_i32);
        assert!(
            catches_panic(|| expect(vec![1_i32]).to_contain(2_i32)),
            "missing item should fail toContain"
        );
    }

    #[test]
    fn to_have_length_matches_strings_and_vectors() {
        expect("éa".to_owned()).to_have_length(2);
        expect(vec![1_i32, 2_i32]).to_have_length(2);
        assert!(
            catches_panic(|| expect(vec![1_i32]).to_have_length(2)),
            "different length should fail toHaveLength"
        );
    }

    #[test]
    fn to_have_property_matches_hash_maps() {
        let map = std::collections::HashMap::from([("name".to_owned(), 1_i32)]);
        expect(map).to_have_property("name".to_owned());
    }

    #[test]
    fn deep_strict_equal_uses_structural_equality() {
        deep_strict_equal(&vec![1_i32, 2_i32], &vec![1_i32, 2_i32]);
        assert!(
            catches_panic(|| deep_strict_equal(&vec![1_i32], &vec![2_i32])),
            "different values should fail deepStrictEqual"
        );
    }

    #[test]
    fn to_throw_matches_errors() {
        expect_fn(|| Err::<(), _>("expected test error")).to_throw();
    }

    #[test]
    fn to_throw_fails_without_error() {
        assert!(
            catches_panic(|| expect_fn(|| Ok::<_, &str>(())).to_throw()),
            "non-error function should fail toThrow"
        );
    }

    #[test]
    fn to_throw_with_message_checks_error_text() {
        expect_fn(|| Err::<(), _>("bad value")).to_throw_with_message("value");
        assert!(
            catches_panic(
                || expect_fn(|| Err::<(), _>("bad value")).to_throw_with_message("missing")
            ),
            "different error message should fail toThrow message matching"
        );
    }

    #[test]
    fn negated_to_throw_matches_ok() {
        expect_fn(|| Ok::<_, &str>(())).not().to_throw();
    }
}
