//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_array_pop_method() {
    let source = source_for(
        r#"
let values: number[] = [1, 2];
values.pop();
const item = values.pop();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<f64>"));
    assert!(source.contains(".pop();"));
}

#[test]
fn emits_array_shift_method() {
    let source = source_for(
        r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<String>"));
    assert!(source.contains(".is_empty()"));
    assert!(source.contains("Some("));
    assert!(source.contains(".remove(0)"));
}

#[test]
fn emits_array_is_array_as_static_boolean() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
"#,
    );

    assert!(source.contains(" = true;"));
    assert!(source.contains(" = false;"));
}

#[test]
fn emits_object_projection_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
const rebuilt = Object.fromEntries([["a", 1], ["b", 2]]);
"#,
    );

    assert!(source.contains(".keys().collect::<Vec<_>>();"), "{source}");
    assert!(
        source.contains(".values().collect::<Vec<_>>();"),
        "{source}"
    );
    assert!(source.contains(".iter().collect::<Vec<_>>();"), "{source}");
    assert!(
        source.contains("SmeltRecord::from([(\"a\".to_owned(), 1.0), (\"b\".to_owned(), 2.0)])"),
        "{source}"
    );
}

#[test]
fn emits_object_assign_call() {
    let source = source_for(
        r#"
const source: Record<string, number> = { a: 1 };
const merged = Object.assign({}, source, { b: 2 });
"#,
    );

    assert!(source.contains("let mut assigned = "));
    assert!(source.contains("assigned.extend("));
    assert!(source.contains("assigned"));
}

#[test]
fn emits_object_assign_call_on_callable_target() {
    let source = source_for(
        r#"
const fnValue = (value: number): number => value;
const assigned = Object.assign(fnValue, { lazy: fnValue });
"#,
    );

    assert!(
        source.contains("let assigned") || source.contains("let mut assigned"),
        "{source}"
    );
    assert!(source.contains("_smelt_tmp_"));
}

#[test]
fn emits_object_has_own_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
"#,
    );

    assert!(source.contains(".contains_key(&"));
}

#[test]
fn emits_static_function_with_params_and_return_value() {
    let source = source_for(
        "function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
",
    );

    assert!(source.contains("fn add(a: f64, b: f64) -> f64 {"));
    assert!(source.contains("a.clone() + b.clone()"));
    assert!(source.contains("let _smelt_tmp_1: f64 = add(2.0, 3.0);"));
}

#[test]
fn emits_async_function_and_await() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
",
    );

    assert!(source.contains("async fn lift(value: f64) -> f64 {"));
    assert!(source.contains("async fn run() -> f64 {"));
    assert!(source.contains("let _smelt_tmp_0 = lift(5.0);"));
    assert!(source.contains("_smelt_tmp_1"));
    assert!(source.contains("_smelt_tmp_0.await"));
}

#[test]
fn emits_promise_all_with_tokio_join() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
",
    );

    assert!(source.contains("async fn run() -> (f64, f64) {"));
    assert!(source.contains("tokio::join!(_smelt_tmp_0, _smelt_tmp_1)"));
    assert!(source.contains("Box::pin(async move { tokio::join!(_smelt_tmp_0, _smelt_tmp_1) })"));
}

#[test]
fn emits_if_else_control_flow() {
    let source = source_for(
        "function max(a: number, b: number): number {
  if (a > b) {
    return a;
  }
  return b;
}
const result = max(2, 3);
console.log(result);
",
    );

    assert!(source.contains("if _smelt_tmp_2.clone() {"));
    assert!(source.contains("return a.clone();"));
    assert!(source.contains("return b.clone();"));
}

#[test]
fn emits_switch_as_rust_match() {
    let source = source_for(
        "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(source.contains("match status.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("return \"Waiting\".to_owned();"));
    assert!(source.contains("_ => {"));
}

#[test]
fn emits_switch_inside_closure_as_rust_match() {
    let source = source_for(
        "const label = (status: \"pending\" | \"approved\"): string => {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
  }
};
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(
        source.contains("_smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: String| {"),
        "{source}"
    );
    assert!(source.contains("match closure_arg_0.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("\"Waiting\".to_owned()"));
    assert!(source.contains("_ => {"));
}

#[test]
fn emits_uncaught_throw_as_result() {
    let source = source_for(
        "function fail(): void {
  throw \"boom\";
}
fail();
",
    );

    assert!(source.contains("fn fail() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("return Err(std::io::Error::new("));
    assert!(source.contains("fn main() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("let _ = fail()?;"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_fetch_as_reqwest_get_text_future() {
    let source = source_for(
        "async function load(): Promise<string> {
  return await fetch(\"https://example.com\");
}
",
    );

    assert!(source.contains(
            "reqwest::get(\"https://example.com\".to_owned()).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn emits_python_requests_get_as_blocking_reqwest_text() {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
            py_frontend::to_hir(
                "import requests\n\ndef load() -> str:\n    return requests.get(\"https://example.com\")\n",
                FileId(0),
                &mut ctx,
            )
            .is_ok(),
            "HIR"
        );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };

    assert!(source.contains(
            "reqwest::blocking::get(\"https://example.com\".to_owned()).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn injects_reqwest_dependency_for_http_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[
            GeneratedDep::Tokio,
            GeneratedDep::Stdlib(BackendDependency::Reqwest),
        ],
    );

    assert!(manifest.contains("tokio = { version = \"1\""));
    assert!(manifest.contains("reqwest = { version = \"0.12\""));
}

#[test]
fn injects_serde_json_dependency_for_json_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::SerdeJson)],
    );

    assert!(manifest.contains("serde_json = \"1\""));
}

#[test]
fn injects_rand_dependency_for_random_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Rand)],
    );

    assert!(manifest.contains("rand = \"0.9\""));
}

#[test]
fn injects_regex_dependency_for_regex_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Regex)],
    );

    assert!(manifest.contains("regex = \"1\""));
}

#[test]
fn injects_chrono_dependency_for_date_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Chrono)],
    );

    assert!(manifest.contains("chrono = \"0.4\""));
}

#[test]
fn emits_date_fns_date_parts() {
    let source = source_for(
        r#"
const date = new Date(2014, 8, 2, 11, 55, 0);
const timestamp = date.getTime();
const year = date.getFullYear();
const month = date.getMonth();
const day = date.getDate();
date.setFullYear(year, month, day + 1);
date.setMonth(0, 1);
date.setDate(2);
"#,
    );

    assert!(source.contains("chrono::NaiveDate::from_ymd_opt"));
    assert!(source.contains(".year() as f64"));
    assert!(source.contains(".month0() as f64"));
    assert!(source.contains(".day() as f64"));
    assert!(source.contains("normalized_year"));
    assert!(source.contains("normalized_month0"));
    assert!(source.contains("chrono::Duration::days"));
}

#[test]
fn emits_invalid_date_parts_without_panicking() {
    let source = source_for(
        r#"
export function isExists(year: number, month: number, day: number): boolean {
  const date = new Date(year, month, day);
  return (
    date.getFullYear() === year &&
    date.getMonth() === month &&
    date.getDate() === day
  );
}
"#,
    );

    assert!(source.contains("unwrap_or(i64::MIN)"));
    assert!(source.contains("map_or(f64::NAN"));
    assert!(source.contains("timestamp_ms.is_finite()"));
    assert!(!source.contains("timestamp out of range"));
}

#[test]
fn preserves_invalid_date_through_setters_and_iso_conversion() {
    let source = source_for(
        r#"
export function keepInvalid(): string {
  const date = new Date(NaN);
  date.setHours(0);
  return date.toISOString();
}
"#,
    );

    assert!(source.contains("timestamp_ms.is_finite()"), "{source}");
    assert!(source.contains("else { i64::MIN }"), "{source}");
    assert!(
        source.contains("timestamp_ms == i64::MIN as f64 { f64::NAN }"),
        "{source}"
    );
    assert!(source.contains("\"Invalid Date\".to_owned()"), "{source}");
}

#[test]
fn emits_overflowing_time_setters_as_duration_arithmetic() {
    let source = source_for(
        r#"
export function nextHour(): number {
  const date = new Date(2020, 0, 1, 23, 30, 0, 0);
  return date.setHours(date.getHours() + 1);
}
"#,
    );

    assert!(
        source.contains("chrono::Duration::milliseconds"),
        "{source}"
    );
    assert!(source.contains("* 3_600_000"), "{source}");
    assert!(
        !source.contains("with_hour"),
        "setHours must preserve JavaScript overflow semantics: {source}"
    );
}
