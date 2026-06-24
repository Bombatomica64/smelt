//! Integration tests for recoverable, categorized diagnostics
//! (`smelt check --message-format json`).

mod common;

use std::fs;

use common::{TempProject, TestResult, ensure, smelt, utf8_path};

/// `check --message-format json` keeps lowering past the first failing file and
/// reports every module's diagnostics, each tagged with a structured category.
#[test]
fn check_json_recovers_and_categorizes_across_files() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "diag-recovery"
version = "0.1.0"

[sources]
entries = ["src/a.ts", "src/b.ts"]

[output]
target = "./dist"
crate-name = "diag_recovery"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // First file references an unknown user symbol; second references a known
    // builtin Smelt does not model. A fail-fast pass would only report the first.
    fs::write(project_path.join("src/a.ts"), "console.log(missingAlpha);\n")?;
    fs::write(project_path.join("src/b.ts"), "console.log(Reflect);\n")?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    // Both files surface (recovery past the first failure).
    ensure(json.contains("missingAlpha"), "missing first-file diagnostic")?;
    ensure(json.contains("Reflect"), "missing second-file diagnostic")?;
    // Categories are decided in the frontend and serialized in kebab-case.
    ensure(
        json.contains("\"unresolved-reference\""),
        "expected unresolved-reference category for unknown symbol",
    )?;
    ensure(
        json.contains("\"missing-stdlib\""),
        "expected missing-stdlib category for known builtin",
    )?;
    Ok(())
}
