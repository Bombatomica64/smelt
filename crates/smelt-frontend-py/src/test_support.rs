//! Pytest test-discovery and public API recognition helpers.
//!
//! Smelt treats pytest as a source-language test API surface. These helpers
//! keep filename and symbol rules out of the main Python lowering logic so the
//! eventual native Rust test emitter can reuse the same decisions.
#![expect(
    clippy::redundant_pub_crate,
    reason = "helpers are intentionally crate-visible within a private lowering module"
)]

use std::path::Path;

/// Return whether `path` follows pytest's common test-file naming convention.
pub(crate) fn is_pytest_file(path: &str) -> bool {
    let Some(file_name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let is_python = Path::new(file_name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("py"));
    is_python && (file_name.starts_with("test_") || file_name.ends_with("_test.py"))
}

/// Return whether `name` follows pytest's top-level test function convention.
pub(crate) fn is_pytest_test_function(name: &str) -> bool {
    name.starts_with("test_")
}

/// Return whether a pytest mark name is recognized by the v1 test API surface.
pub(crate) fn is_recognized_pytest_mark(name: &str) -> bool {
    matches!(name, "parametrize" | "skip" | "skipif" | "xfail")
}

/// Return whether a pytest attribute is part of the recognized public API.
#[expect(
    dead_code,
    reason = "pytest API recognition is shared with the next raises/fixture lowering slice"
)]
pub(crate) fn is_recognized_pytest_api(name: &str) -> bool {
    name == "raises" || name == "fixture" || is_recognized_pytest_mark(name)
}
