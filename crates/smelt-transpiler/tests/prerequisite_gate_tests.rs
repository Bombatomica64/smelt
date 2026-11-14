//! A test that needs a tool must say so when the tool is missing, and must fail
//! under `SMELT_REQUIRE_SANDBOX`.
//!
//! The skip paths are only reachable on a host without the tool, so each probe
//! below is an ignored test that this binary re-runs in a child process whose
//! `PATH` holds nothing. A helper that went back to a bare `return Ok(())` passes
//! the strict run and fails here.

mod common;

use std::{env, process::Command};

use common::{
    TempProject, TestResult, ensure, ensure_node_agrees, ensure_python_agrees, tool_missing,
};
use smelt_specialize::prereq::REQUIRE_SANDBOX_ENV;

/// Reaches the Node oracle.
#[test]
#[ignore = "run by the tests below, in a child with an empty PATH"]
fn probe_node_oracle() -> TestResult {
    ensure_node_agrees(&[("main.ts", "")], "main.ts", "", "probe_node_oracle")
}

/// Reaches the Python oracle.
#[test]
#[ignore = "run by the tests below, in a child with an empty PATH"]
fn probe_python_oracle() -> TestResult {
    ensure_python_agrees("", "", "probe_python_oracle")
}

/// Reaches the probe the sandbox-backed CLI tests use.
#[test]
#[ignore = "run by the tests below, in a child with an empty PATH"]
fn probe_tool() -> TestResult {
    tool_missing("probe_tool", &["bwrap"]).map(drop)
}

/// Runs one probe in a child with an empty `PATH`; returns (success, stderr and stdout).
fn run_probe(probe: &str, strict: bool) -> TestResult<(bool, String)> {
    let project = TempProject::new()?;
    let mut command = Command::new(env::current_exe()?);
    command
        .args(["--ignored", "--exact", "--nocapture", probe])
        .env("PATH", project.path())
        .env_remove(REQUIRE_SANDBOX_ENV);
    if strict {
        command.env(REQUIRE_SANDBOX_ENV, "1");
    }
    let output = command.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    Ok((output.status.success(), text))
}

/// Every probe reports a skip by default and fails in strict mode.
#[test]
fn a_missing_tool_is_a_reported_skip_and_a_strict_failure() -> TestResult {
    for probe in ["probe_node_oracle", "probe_python_oracle", "probe_tool"] {
        let (passed, text) = run_probe(probe, false)?;
        ensure(
            passed,
            format!("{probe} must skip without strict mode: {text}"),
        )?;
        ensure(
            text.contains("SKIP"),
            format!("{probe} skipped silently: {text}"),
        )?;
        let (passed, text) = run_probe(probe, true)?;
        ensure(!passed, format!("{probe} passed in strict mode: {text}"))?;
        ensure(
            text.contains("required prerequisite"),
            format!("{probe}: {text}"),
        )?;
    }
    Ok(())
}
