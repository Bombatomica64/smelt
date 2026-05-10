//! TypeScript test-framework detection helpers.
//!
//! These helpers deliberately model public test APIs as frontend-known names.
//! The first lowering phase only needs to stop treating framework symbols as
//! user runtime imports; native Rust test emission is layered on top later.
#![expect(
    clippy::redundant_pub_crate,
    reason = "helpers are intentionally crate-visible within a private lowering module"
)]

/// Vitest-compatible module specifiers recognized by Smelt's test lowering.
const VITEST_COMPATIBLE_MODULES: &[&str] = &["vitest", "@effect/vitest"];

/// Names imported from Vitest-compatible modules that are test-framework APIs.
const VITEST_BUILTIN_NAMES: &[&str] = &[
    "describe",
    "it",
    "test",
    "expect",
    "beforeEach",
    "afterEach",
];

/// Return whether `module` is a Vitest-compatible framework import source.
pub(crate) fn is_vitest_compatible_module(module: &str) -> bool {
    VITEST_COMPATIBLE_MODULES.contains(&module)
}

/// Return whether `name` is a supported Vitest public API builtin.
pub(crate) fn is_vitest_builtin_name(name: &str) -> bool {
    VITEST_BUILTIN_NAMES.contains(&name)
}
