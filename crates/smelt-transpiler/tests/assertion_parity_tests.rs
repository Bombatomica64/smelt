//! Generated tests must pass and fail exactly where the same tests do under
//! vitest, for chai, `fast-check` and the vitest matchers.
//!
//! Every case in `fixtures/assertion-parity` is named for its intended outcome:
//! `F_` must fail and `T_` must pass. A `F_` case is a deliberately false
//! assertion, so the suite proves a wrong value fails the Rust test rather than
//! passing silently.
//!
//! The real libraries are the oracle: the fixtures also run under vitest at the
//! versions pinned below, and the two outcome sets must be equal.

mod common;

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use serde_json::Value;

use common::{TempProject, TestResult, ensure, ensure_eq, npm_available, smelt_in, workspace_root};

/// The npm packages of the oracle, pinned so a release cannot move the verdicts.
const ORACLE_PACKAGES: &[&str] = &[
    "vitest@3.2.4",
    "chai@5.2.0",
    "fast-check@3.23.2",
    "@fast-check/vitest@0.2.1",
];

/// Copy the fixtures into `root/src`.
fn write_fixtures(root: &Path) -> TestResult {
    let fixtures =
        workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/assertion-parity");
    fs::create_dir_all(root.join("src"))?;
    for dir_entry in fs::read_dir(fixtures)? {
        let file = dir_entry?;
        fs::copy(file.path(), root.join("src").join(file.file_name()))?;
    }
    Ok(())
}

/// Each generated test's name and whether it passed.
fn generated_outcomes(root: &Path) -> TestResult<BTreeMap<String, bool>> {
    fs::write(
        root.join("Smelt.toml"),
        r#"[project]
name = "assertion-parity"
version = "0.1.0"

[sources]
roots = ["src"]
test-globs = ["**/*.test.ts"]
entries = ["src/index.ts"]

[output]
target = "./dist"
crate-name = "assertion_parity"
build = false
"#,
    )?;
    // `@fast-check/vitest` has no installed types here.
    smelt_in(root, &["build", "--allow-type-errors"])?;
    let binaries = common::build_generated_tests(
        Command::new("cargo")
            .current_dir(root)
            .env("RUSTFLAGS", "-Awarnings"),
        &root.join("dist/Cargo.toml"),
    )?;
    let output = common::run_generated_tests(&binaries, |run| {
        run.current_dir(root);
    })?;
    let stdout = String::from_utf8(output.stdout)?;
    let outcomes: BTreeMap<String, bool> = stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("test __smelt_module_")?;
            let (name, status) = rest.split_once(" ... ")?;
            Some((name.to_owned(), status == "ok"))
        })
        .collect();
    ensure(
        !outcomes.is_empty(),
        format!(
            "no generated tests ran\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(outcomes)
}

/// Whether a generated test name carries the `F_`-prefixed title.
fn title_of(name: &str, marker: &str) -> bool {
    name.contains(marker)
}

/// A false assertion fails the Rust test, and a true one passes it.
#[test]
fn false_assertions_fail_and_true_ones_pass() -> TestResult {
    let project = TempProject::new()?;
    write_fixtures(project.path())?;
    let outcomes = generated_outcomes(project.path())?;
    for (name, passed) in &outcomes {
        let expect_failure = title_of(name, "_f_");
        ensure(
            title_of(name, "_f_") || title_of(name, "_t_"),
            format!("{name} names no outcome"),
        )?;
        ensure_eq(
            passed,
            &!expect_failure,
            format!("generated outcome of {name}"),
        )?;
    }
    Ok(())
}

/// Reads vitest's JSON report under `root` into a map from lower-cased case
/// title to whether that case passed.
fn vitest_verdicts(root: &Path) -> TestResult<BTreeMap<String, bool>> {
    let report: Value = serde_json::from_str(&fs::read_to_string(root.join("vitest.json"))?)?;
    let mut oracle = BTreeMap::new();
    for file in report
        .get("testResults")
        .and_then(Value::as_array)
        .ok_or("vitest report has no testResults")?
    {
        for case in file
            .get("assertionResults")
            .and_then(Value::as_array)
            .ok_or("vitest report has no assertionResults")?
        {
            oracle.insert(
                // `@fast-check/vitest` appends the run's seed to the title.
                case.get("title")
                    .and_then(Value::as_str)
                    .ok_or("a case has no title")?
                    .split(" (")
                    .next()
                    .unwrap_or_default()
                    .to_lowercase(),
                case.get("status").and_then(Value::as_str) == Some("passed"),
            );
        }
    }
    Ok(oracle)
}

/// The generated verdicts equal the verdicts of the real libraries under vitest.
#[test]
fn outcomes_match_vitest() -> TestResult {
    if !npm_available() {
        return common::tool_missing("outcomes_match_vitest", &["npm"]).map(|_| ());
    }
    let project = TempProject::new()?;
    let root = project.path();
    write_fixtures(root)?;
    fs::write(
        root.join("package.json"),
        r#"{"name":"oracle","private":true,"type":"module"}"#,
    )?;
    let install = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund", "--silent"])
        .args(ORACLE_PACKAGES)
        .current_dir(root)
        .output()?;
    ensure(
        install.status.success(),
        format!(
            "npm install failed: {}",
            String::from_utf8_lossy(&install.stderr)
        ),
    )?;
    Command::new("npx")
        .args([
            "vitest",
            "run",
            "--reporter=json",
            "--outputFile=vitest.json",
        ])
        .current_dir(root)
        .output()?;
    let oracle = vitest_verdicts(root)?;
    let generated = generated_outcomes(root)?;
    ensure_eq(
        &generated.len(),
        &oracle.len(),
        "the two runs cover the same cases",
    )?;
    for (name, passed) in &generated {
        let (_, verdict) = oracle
            .iter()
            .find(|(title, _)| name.ends_with(title.as_str()))
            .ok_or_else(|| format!("no vitest case for generated test {name}"))?;
        ensure_eq(passed, verdict, format!("verdict of {name}"))?;
    }
    Ok(())
}
