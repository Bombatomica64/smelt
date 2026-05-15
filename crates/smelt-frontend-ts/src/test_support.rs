//! TypeScript test-framework detection helpers.
//!
//! These helpers deliberately model public test APIs as frontend-known names.
//! The first lowering phase only needs to stop treating framework symbols as
//! user runtime imports; native Rust test emission is layered on top later.
/// Vitest-compatible module specifiers recognized by Smelt's test lowering.
const VITEST_COMPATIBLE_MODULES: &[&str] = &["vitest", "@effect/vitest", "@fast-check/vitest"];

/// Names imported from Vitest-compatible modules that are test-framework APIs.
const VITEST_BUILTIN_NAMES: &[&str] = &[
    "describe",
    "it",
    "test",
    "expect",
    "expectTypeOf",
    "beforeAll",
    "afterAll",
    "beforeEach",
    "afterEach",
];

/// Public type-test API roots that are erased after static lowering.
const TYPE_TEST_BUILTIN_NAMES: &[&str] = &["expectTypeOf", "expectType", "assertType"];

/// Return whether `module` is a Vitest-compatible framework import source.
#[must_use]
pub fn is_vitest_compatible_module(module: &str) -> bool {
    VITEST_COMPATIBLE_MODULES.contains(&module)
}

/// Return whether `name` is a supported Vitest public API builtin.
#[must_use]
pub fn is_vitest_builtin_name(name: &str) -> bool {
    VITEST_BUILTIN_NAMES.contains(&name)
}

/// Return whether `name` is a supported type-test-only assertion API.
#[must_use]
pub fn is_type_test_builtin_name(name: &str) -> bool {
    TYPE_TEST_BUILTIN_NAMES.contains(&name)
}
