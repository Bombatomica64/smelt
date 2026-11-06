//! Test-harness lowering: describe/it suites and expect/assert matchers.

mod async_expect;
mod fake_timers;
mod matchers;
mod mocks;
mod recognition;
mod suites;
mod to_throw;
mod type_tests;

pub(in crate::lowering) use matchers::LoweredActual;
pub(in crate::lowering) use mocks::is_test_mock_namespace;
