//! Prerequisite probing for sandbox-backed tests.
//!
//! Sandbox-backed tests need a real hard-sandbox backend (`bwrap` plus
//! `prlimit`) and a real guest runtime (`node`, `tsc`, `python3`) present on the
//! host. Developer machines routinely have none of them, so these tests are
//! written to skip when a prerequisite is missing.
//!
//! The problem is that a skip spelled `return Ok(())` is indistinguishable from
//! a pass. That is how the sandbox — the fail-closed security boundary — came to
//! report green in CI while never executing a single guest process (see gap 3 of
//! `blocker-logs/test-coverage-analysis.md`).
//!
//! This module makes the skip *observable* and lets CI *forbid* it:
//!
//! - Every skip prints a `SKIP` notice naming the test and the missing
//!   prerequisite, so a local run says what it did not check.
//! - Setting [`REQUIRE_SANDBOX_ENV`] turns a missing prerequisite into a test
//!   failure. CI sets it, so the tier can never silently rot again.
//!
//! It also centralizes executable discovery. The previous per-test helpers only
//! probed hardcoded absolute paths such as `/usr/bin/node`, which never match a
//! runner that installs toolchains elsewhere (GitHub's `setup-node` puts Node
//! under the hosted tool cache). [`locate_executable`] searches `PATH` first.
//!
//! ## Testability
//!
//! The environment is read in exactly one place ([`sandbox_required`]) and every
//! decision below it is a pure function of that boolean. Tests therefore never
//! mutate process environment — which `unsafe_code = "deny"` forbids anyway, and
//! which would be racy under the threaded test runner.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// Environment variable that forbids skipping sandbox-backed tests.
///
/// Set it to any value other than `0`, `false`, `no`, or the empty string to
/// make a missing prerequisite fail instead of skip.
pub const REQUIRE_SANDBOX_ENV: &str = "SMELT_REQUIRE_SANDBOX";

/// Reports whether sandbox-backed tests are forbidden from skipping.
///
/// This is the only reader of the process environment in this module.
#[must_use]
pub fn sandbox_required() -> bool {
    required_from(std::env::var_os(REQUIRE_SANDBOX_ENV).as_deref())
}

/// Decides strict mode from a raw environment value.
///
/// Split out from [`sandbox_required`] so the falsy-spelling table can be tested
/// without mutating the environment.
fn required_from(value: Option<&OsStr>) -> bool {
    value.is_some_and(|raw| {
        let text = raw.to_string_lossy().to_ascii_lowercase();
        !matches!(text.trim(), "" | "0" | "false" | "no" | "off")
    })
}

/// Records a missing test prerequisite, skipping or failing per environment.
///
/// Returns `Ok(())` so a caller can `return` it directly as a skip, unless
/// [`REQUIRE_SANDBOX_ENV`] is set, in which case it returns a descriptive error.
/// The error type is generic so one call site works both in tests returning
/// `Result<(), String>` and tests returning `Result<(), Box<dyn Error>>`.
///
/// # Errors
///
/// Returns an error naming the test and the missing prerequisite whenever
/// [`sandbox_required`] holds.
pub fn missing_prerequisite<E>(test: &str, requirement: &str) -> Result<(), E>
where
    E: From<String>,
{
    match prerequisite_outcome(sandbox_required(), test, requirement) {
        Ok(notice) => {
            report_skip(&notice);
            Ok(())
        }
        Err(failure) => Err(E::from(failure)),
    }
}

/// Builds the skip notice, or the failure message when skipping is forbidden.
///
/// Pure so both branches are directly testable.
fn prerequisite_outcome(required: bool, test: &str, requirement: &str) -> Result<String, String> {
    if required {
        return Err(format!(
            "{test}: required prerequisite is unavailable: {requirement}. \
             {REQUIRE_SANDBOX_ENV} is set, so skipping is not permitted — install \
             the prerequisite or unset {REQUIRE_SANDBOX_ENV}"
        ));
    }
    Ok(format!(
        "SKIP {test}: {requirement} (set {REQUIRE_SANDBOX_ENV}=1 to make this a failure)"
    ))
}

/// Writes a skip notice to the test log.
#[expect(
    clippy::print_stderr,
    reason = "a skip notice must reach the test log; an invisible skip is the defect this module exists to remove"
)]
fn report_skip(notice: &str) {
    eprintln!("{notice}");
}

/// Locates an executable on `PATH`, then at explicit fallback paths.
///
/// `PATH` is searched first so a runner-installed toolchain wins over a distro
/// package. `fallbacks` keeps absolute paths working on hosts with a trimmed
/// `PATH`.
#[must_use]
pub fn locate_executable(name: &str, fallbacks: &[&str]) -> Option<PathBuf> {
    search_path(name).or_else(|| {
        fallbacks
            .iter()
            .map(Path::new)
            .find(|candidate| candidate.is_file())
            .map(Path::to_path_buf)
    })
}

/// Searches `PATH` for a named executable.
fn search_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// Falsy spellings must not accidentally arm strict mode.
    #[test]
    fn falsy_values_do_not_require_the_sandbox() {
        for falsy in ["0", "false", "FALSE", "no", "off", "", "   "] {
            let value = OsString::from(falsy);
            assert!(
                !required_from(Some(value.as_os_str())),
                "{falsy:?} must not enable strict sandbox mode"
            );
        }
    }

    /// An unset variable leaves the default skip behavior in place.
    #[test]
    fn unset_variable_does_not_require_the_sandbox() {
        assert!(
            !required_from(None),
            "an unset variable must leave skipping permitted"
        );
    }

    /// Truthy spellings arm strict mode.
    #[test]
    fn truthy_values_require_the_sandbox() {
        for truthy in ["1", "true", "yes", "on", "required"] {
            let value = OsString::from(truthy);
            assert!(
                required_from(Some(value.as_os_str())),
                "{truthy:?} must enable strict sandbox mode"
            );
        }
    }

    /// Default mode produces a notice that names what went unchecked.
    #[test]
    fn permissive_mode_yields_a_named_skip_notice() -> Result<(), String> {
        let notice = prerequisite_outcome(false, "guest_source_is_valid_javascript", "node")?;
        assert!(
            notice.contains("SKIP")
                && notice.contains("guest_source_is_valid_javascript")
                && notice.contains("node"),
            "skip notice must name the test and the prerequisite, got: {notice}"
        );
        Ok(())
    }

    /// Strict mode is the whole point: the skip becomes a failure.
    #[test]
    fn strict_mode_turns_a_missing_prerequisite_into_a_failure() -> Result<(), String> {
        let Err(failure) = prerequisite_outcome(true, "sandboxed_guest_runs", "bwrap") else {
            return Err(format!("{REQUIRE_SANDBOX_ENV} must forbid skipping"));
        };
        assert!(
            failure.contains("bwrap")
                && failure.contains("sandboxed_guest_runs")
                && failure.contains(REQUIRE_SANDBOX_ENV),
            "failure must name the test, the prerequisite, and the override, got: {failure}"
        );
        Ok(())
    }

    /// `PATH` lookup finds a binary every POSIX host has.
    #[test]
    fn locate_executable_searches_path() {
        assert!(
            locate_executable("sh", &[]).is_some(),
            "sh must be locatable via PATH on any POSIX host"
        );
    }

    /// Fallback paths still resolve when `PATH` misses.
    #[test]
    fn locate_executable_uses_fallback_paths() {
        assert!(
            locate_executable("smelt-not-on-path", &["/bin/sh"]).is_some(),
            "an absolute fallback must resolve when PATH lookup fails"
        );
    }

    /// A name that exists nowhere resolves to nothing rather than a bogus path.
    #[test]
    fn locate_executable_reports_absent_binaries() {
        assert!(
            locate_executable("smelt-definitely-not-a-real-binary", &[]).is_none(),
            "a nonexistent executable must not resolve"
        );
    }
}
