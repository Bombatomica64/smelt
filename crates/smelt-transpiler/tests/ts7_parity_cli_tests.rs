//! CLI tests for `smelt ts7-parity-report`: Smelt's HIR types compared with
//! the embedded TypeScript 7 checker's types on tiny programs.
//!
//! Each program pins one verdict family: a fully typed program agrees
//! everywhere, a conditional type whose arms differ is resolved by TypeScript
//! at the call site but erased by Smelt's own type lowering (the parity gap
//! the report exists to measure), and a source `unknown` is TypeScript's own
//! dynamic boundary, never counted against Smelt.

#![cfg(feature = "ts7")]

mod common;

use std::fs;

use common::{TempProject, TestResult, ensure, ensure_eq, smelt_in};

/// Runs the report on one program and returns its JSON.
fn report(source: &str) -> TestResult<serde_json::Value> {
    let project = TempProject::new()?;
    fs::create_dir_all(project.path())?;
    fs::write(project.path().join("prog.ts"), source)?;
    let stdout = smelt_in(project.path(), &["ts7-parity-report", "prog.ts", "--format", "json"])?;
    Ok(serde_json::from_str(&stdout)?)
}

/// `totals[name]` as a number.
fn total(report: &serde_json::Value, name: &str) -> TestResult<u64> {
    report
        .get("totals")
        .and_then(|totals| totals.get(name))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("missing totals.{name}").into())
}

/// Group keys of one verdict family (`erased`, `wider`, `mismatches`).
fn group_keys(report: &serde_json::Value, family: &str) -> Vec<String> {
    report
        .get(family)
        .and_then(serde_json::Value::as_array)
        .map(|groups| groups.iter().filter_map(|group| group["key"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

#[test]
fn fully_typed_program_agrees_everywhere() -> TestResult {
    let report = report(
        r"function add(a: number, b: number): number {
  return a + b;
}
const xs: number[] = [1, 2, 3];
const doubled = xs.map((x) => add(x, x));
const label: string = `n=${doubled.length}`;
console.log(label, doubled[0] ?? 0);
",
    )?;
    ensure(total(&report, "agree")? >= 15, "the program's expressions are compared")?;
    for gap in ["smelt-erased", "wider", "mismatch", "ts-dynamic"] {
        ensure_eq(&total(&report, gap)?, &0, format!("no `{gap}` rows in a fully typed program"))?;
    }
    let checker_errors = report.get("checker_errors").and_then(serde_json::Value::as_object);
    ensure(checker_errors.is_some_and(serde_json::Map::is_empty), "valid TypeScript 7")
}

#[test]
fn conditional_type_resolved_by_typescript_is_a_smelt_gap() -> TestResult {
    let report = report(
        r#"type Pick1<T> = T extends string ? string[] : number;
function probe<T extends string | number>(x: T): Pick1<T> {
  return (typeof x === "string" ? [x] : 1) as Pick1<T>;
}
const words = probe("a");
console.log(words);
"#,
    )?;
    ensure(total(&report, "smelt-erased")? > 0, "Smelt erases the conditional's instantiation")?;
    let erased = group_keys(&report, "erased");
    ensure(
        erased.iter().any(|key| key == "Unknown ← Array<string>"),
        format!("TypeScript resolves `probe(\"a\")` to string[]: {erased:?}"),
    )
}

#[test]
fn source_unknown_is_typescript_dynamic_not_a_gap() -> TestResult {
    let report = report(
        r#"const parsed: unknown = JSON.parse("1");
console.log(typeof parsed);
"#,
    )?;
    ensure(total(&report, "ts-dynamic")? > 0, "the `unknown` binding is TypeScript-dynamic")?;
    ensure_eq(&total(&report, "smelt-erased")?, &0, "a source `unknown` never counts as Smelt erasure")
}
