//! CLI tests for erasure reasons: severities, diagnostics, the
//! `smelt-erasure.json` sidecar and its section in `smelt-unknown-report`.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631),
//! `src/sema/aot_rules.rs` (allow/warn/deny rules) and
//! `docs/bundler/aot-adoption.mdx`.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, ensure, smelt_in};

/// A program erasing one value per reason class: an `any` parameter
/// (avoidable, line 1 col 18) and `JSON.parse` (legitimate, line 4).
const PROGRAM: &str = "function show(x: any): void {\n  console.log(x);\n}\nconst data = JSON.parse('{\"a\":1}');\nshow(data);\n";

/// Write a one-file project with the given extra manifest text.
fn project(extra_manifest: &str) -> TestResult<TempProject> {
    let project = TempProject::new()?;
    fs::create_dir_all(project.path())?;
    fs::write(project.path().join("main.ts"), PROGRAM)?;
    fs::write(
        project.path().join("Smelt.toml"),
        format!(
            "[project]\nname = \"erasure-cli\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"main.ts\"]\n\n[output]\ntarget = \"out\"\n\n[runtime]\nclone-strategy = \"aggressive\"\n{extra_manifest}"
        ),
    )?;
    Ok(project)
}

/// Outcome of one run: success, stdout, stderr.
type RunOutcome = (bool, String, String);

/// Run smelt in `dir`, returning (success, stdout, stderr).
fn run(dir: &Path, args: &[&str]) -> TestResult<RunOutcome> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(dir)
        .args(args)
        .output()?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

/// By default every reason is `allow`: nothing is printed, the build passes,
/// and the sidecar still records both decisions with their classes.
#[test]
fn allow_is_silent_and_the_sidecar_records_everything() -> TestResult {
    let project = project("")?;
    let (ok, _, stderr) = run(project.path(), &["build"])?;
    ensure(ok, format!("build failed: {stderr}"))?;
    ensure(
        !stderr.contains("erasure/"),
        format!("unexpected diagnostics: {stderr}"),
    )?;
    let sidecar = fs::read_to_string(project.path().join("out/smelt-erasure.json"))?;
    let json: serde_json::Value = serde_json::from_str(&sidecar)?;
    let records = json["records"].as_array().ok_or("records")?;
    let any = records
        .iter()
        .find(|record| record["reason"] == "explicit-any")
        .ok_or("explicit-any record")?;
    ensure(
        any["file"] == "main.ts" && any["line"] == 1_u64 && any["column"] == 18_u64,
        format!("{any}"),
    )?;
    ensure(any["class"] == "avoidable", format!("{any}"))?;
    let external = records
        .iter()
        .find(|record| record["reason"] == "external-type")
        .ok_or("external-type record")?;
    ensure(
        external["line"] == 4_u64 && external["class"] == "legitimate",
        format!("{external}"),
    )?;
    Ok(())
}

/// A manifest `warn` prints the diagnostic and the build still passes.
#[test]
fn manifest_warn_prints_and_passes() -> TestResult {
    let project = project("\n[erasure]\nexplicit-any = \"warn\"\n")?;
    let (ok, _, stderr) = run(project.path(), &["build"])?;
    ensure(ok, format!("build failed: {stderr}"))?;
    ensure(
        stderr.contains("warning[erasure/explicit-any]: main.ts:1:18: "),
        format!("missing warning: {stderr}"),
    )?;
    ensure(
        !stderr.contains("external-type"),
        format!("allowed reason printed: {stderr}"),
    )
}

/// A CLI `deny` fails both `build` and `check`, overriding the manifest.
#[test]
fn cli_deny_fails_build_and_check() -> TestResult {
    let project = project("\n[erasure]\nexplicit-any = \"allow\"\n")?;
    for command in ["build", "check"] {
        let (ok, _, stderr) = run(project.path(), &[command, "--erasure", "explicit-any=deny"])?;
        ensure(!ok, format!("{command} passed despite deny"))?;
        ensure(
            stderr.contains("error[erasure/explicit-any]: main.ts:1:18: "),
            format!("{command}: missing error: {stderr}"),
        )?;
    }
    Ok(())
}

/// `check --message-format json` carries erasure diagnostics with severity.
#[test]
fn json_check_includes_severity() -> TestResult {
    let project = project("")?;
    let stdout = smelt_in(
        project.path(),
        &[
            "check",
            "--message-format",
            "json",
            "--erasure",
            "external-type=warn",
        ],
    )?;
    let json: serde_json::Value = serde_json::from_str(&stdout)?;
    let diagnostics = json.as_array().ok_or("array")?;
    ensure(
        diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == "erasure/external-type"
                && diagnostic["severity"] == "warning"
                && diagnostic["line"] == 4_u64
        }),
        format!("missing diagnostic: {stdout}"),
    )?;
    ensure(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["code"] != "erasure/explicit-any"),
        format!("allowed reason reported: {stdout}"),
    )
}

/// A type parameter the backend erases at a call site is recorded as
/// `erased-generic` with the parameter name, attributed to the source file.
#[test]
fn backend_records_erased_generics() -> TestResult {
    let project = project("")?;
    // Old fixtures: a plain `pick<T>` method, then an OVERRIDDEN one, which
    // the backend erased. Both are now Rust generics (`fn pick<T>`; an
    // override, abstract or not, is reached through the polymorphic class
    // handle, `codegen::class_polymorphism`). The backend still erases a
    // generic where a call site cannot instantiate it: a packed rest parameter
    // destroys positional alignment (`call_generics::static_call_monomorphization`,
    // rule 2), so the call passes erased values. The requirement is unchanged:
    // a backend erasure names its reason.
    //
    // The detail is the demotion-reason report. Old requirement: the detail
    // was the bare parameter name (`T`), which could not say whose `T` was
    // demoted or why. New requirement: it names the owner and the refusal:
    // the callee's `T` left unbound at its caller, and the gate's reason for
    // a function's own parameter (bounded, or no inference source).
    fs::write(
        project.path().join("main.ts"),
        "function firstOf<T>(...xs: T[]): T {\n  return xs[0];\n}\nconsole.log(firstOf(1, 2));\nfunction sizeOf<T extends { size: number }>(x: T): number {\n  return x.size;\n}\nconsole.log(sizeOf({ size: 3 }));\nfunction make<R>(): R[] {\n  return [];\n}\nconsole.log(make<number>().length);\n",
    )?;
    let (ok, _, stderr) = run(project.path(), &["build"])?;
    ensure(ok, stderr)?;
    let sidecar = fs::read_to_string(project.path().join("out/smelt-erasure.json"))?;
    let json: serde_json::Value = serde_json::from_str(&sidecar)?;
    let records = json
        .get("records")
        .and_then(serde_json::Value::as_array)
        .ok_or("records")?;
    for detail in [
        "T of callee first_of: unbound at this caller",
        "T of size_of: bounded parameter without a liftable bound trait",
        "R of make: no inference source in the emitted signature",
    ] {
        ensure(
            records.iter().any(|record| {
                record.get("reason").and_then(serde_json::Value::as_str) == Some("erased-generic")
                    && record.get("origin").and_then(serde_json::Value::as_str) == Some("backend")
                    && record.get("detail").and_then(serde_json::Value::as_str) == Some(detail)
                    && record.get("file").and_then(serde_json::Value::as_str) == Some("main.ts")
            }),
            format!("no erased-generic record `{detail}`: {sidecar}"),
        )?;
    }
    Ok(())
}

/// An unknown reason name is rejected.
#[test]
fn unknown_reason_is_rejected() -> TestResult {
    let project = project("")?;
    let (ok, _, stderr) = run(project.path(), &["build", "--erasure", "no-such=warn"])?;
    ensure(!ok && stderr.contains("unknown erasure reason"), stderr)
}

/// `smelt-unknown-report` reads the sidecar beside the scanned `src/` and
/// explains avoidable erasure by reason and source line.
#[test]
fn unknown_report_reads_the_sidecar() -> TestResult {
    let project = project("")?;
    let (ok, _, stderr) = run(project.path(), &["build"])?;
    ensure(ok, stderr)?;
    let markdown = smelt_in(project.path(), &["smelt-unknown-report", "out/src"])?;
    ensure(
        markdown.contains("## Avoidable erasure by reason and source line"),
        markdown.clone(),
    )?;
    ensure(
        markdown.contains("| `main.ts:1` | `explicit-any` |"),
        markdown.clone(),
    )?;
    let json = smelt_in(
        project.path(),
        &["smelt-unknown-report", "out/src", "--format", "json"],
    )?;
    let report: serde_json::Value = serde_json::from_str(&json)?;
    ensure(
        report["erasure"]["avoidable"].as_u64() >= Some(1),
        json.clone(),
    )?;
    // The legitimate JSON.parse decision is counted by reason but never
    // listed among the avoidable lines.
    let lines = report["erasure"]["avoidable_lines"]
        .as_array()
        .ok_or("lines")?;
    ensure(
        lines.iter().all(|line| line["reason"] != "external-type"),
        json,
    )
}

/// A closure whose return the checker infers as `any` records `explicit-any`,
/// not `missing-annotation`: the return is not missing, it is the author's
/// `any` flowing out of the body (`return target(...args)` with `target:
/// any`). Old requirement: such a closure recorded `missing-annotation`
/// (#534 found the shared function-expression path declining the checker's
/// `any`). New requirement: the record names the `any` that decided it. Both
/// reasons are avoidable, so the record moves between avoidable pools and
/// the avoidable total is unchanged. A closure whose return the checker
/// cannot name at all (here none) would keep `missing-annotation`.
#[test]
fn checker_inferred_any_closure_return_is_explicit_any() -> TestResult {
    let project = TempProject::new()?;
    fs::create_dir_all(project.path())?;
    fs::write(
        project.path().join("main.ts"),
        "function keep(callback: any): any {\n  return callback;\n}\nfunction forward(target: any): any {\n  return keep(function (...args: number[]) {\n    return target(...args);\n  });\n}\nconsole.log(forward((a: number) => a + 1)(2));\n",
    )?;
    fs::write(
        project.path().join("Smelt.toml"),
        "[project]\nname = \"erasure-any-return\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"main.ts\"]\n\n[output]\ntarget = \"out\"\n",
    )?;
    let (ok, _, stderr) = run(project.path(), &["build"])?;
    ensure(ok, format!("build failed: {stderr}"))?;
    let sidecar = fs::read_to_string(project.path().join("out/smelt-erasure.json"))?;
    let json: serde_json::Value = serde_json::from_str(&sidecar)?;
    let records = json
        .get("records")
        .and_then(serde_json::Value::as_array)
        .ok_or("records")?;
    let returns: Vec<_> = records
        .iter()
        .filter(|record| record["detail"] == "function expression return")
        .collect();
    ensure(
        !returns.is_empty()
            && returns
                .iter()
                .all(|record| record["reason"] == "explicit-any"),
        format!("function expression return records: {returns:?}"),
    )?;
    Ok(())
}
