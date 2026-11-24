//! Test-harness lowering: describe/it suites and expect/assert matchers.

mod assertion_check;
mod assertion_library;
mod async_expect;
mod chai_words;
mod containment;
mod fake_timers;
/// The `expect` matchers and the shared assertion site.
pub(in crate::lowering) mod matchers;
mod mocks;
mod recognition;
mod suites;
mod to_throw;
mod type_tests;

pub(in crate::lowering) use matchers::LoweredActual;
pub(in crate::lowering) use mocks::is_test_mock_namespace;
