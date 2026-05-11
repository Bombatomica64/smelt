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

    assert!(source.contains(".keys().cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".values().cloned().collect::<Vec<_>>();"));
    assert!(
        source.contains(
            ".iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>();"
        )
    );
    assert!(source.contains("HashMap::from([(\"a\".to_owned(), 1.0), (\"b\".to_owned(), 2.0)])"));
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

    assert!(source.contains("fn add(arg_0: f64, arg_1: f64) -> f64 {"));
    assert!(source.contains("arg_0.clone() + arg_1.clone()"));
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

    assert!(source.contains("async fn lift(arg_0: f64) -> f64 {"));
    assert!(source.contains("async fn run() -> f64 {"));
    assert!(source.contains("let _smelt_tmp_0 = lift(5.0);"));
    assert!(source.contains("let _smelt_tmp_1: f64 = _smelt_tmp_0.await;"));
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
    assert!(
            source.contains(
                "let _smelt_tmp_2: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = (f64, f64)>>> = Box::pin(async move { tokio::join!(_smelt_tmp_0, _smelt_tmp_1) });"
            )
        );
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
    assert!(source.contains("return arg_0.clone();"));
    assert!(source.contains("return arg_1.clone();"));
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

    assert!(source.contains("match arg_0.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("return \"Waiting\".to_owned();"));
    assert!(source.contains("_ => unreachable!(),"));
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
    assert!(source.contains("let _smelt_tmp_0: () = fail()?;"));
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
