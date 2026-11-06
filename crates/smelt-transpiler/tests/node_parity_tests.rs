//! Valid TypeScript whose generated Rust must print what Node prints.
//!
//! Each `fixtures/node-parity/<name>.ts` runs through a full `smelt build` and a
//! compile of the generated crate, and its stdout is compared with the committed
//! `<name>.stdout` and with Node itself. These programs pin a language rule rather
//! than a corpus shape, so they sit outside the example corpus the typedness and
//! `SmeltUnknown` baselines measure: their `unknown` parameters are the dynamic
//! boundary the rule is about.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure_eq, ensure_node_agrees, smelt_in,
    workspace_root,
};

/// Build fixture `name` and check its stdout against the golden and Node.
fn verify_node_parity(name: &str) -> TestResult {
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/node-parity");
    let source = fs::read_to_string(fixtures.join(format!("{name}.ts")))?;
    let expected = fs::read_to_string(fixtures.join(format!("{name}.stdout")))?;
    ensure_node_agrees(&[("main.ts", &source)], "main.ts", &expected, name)?;
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/main.ts"), &source)?;
    fs::write(
        root.join("Smelt.toml"),
        r#"[project]
name = "node-parity"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "node_parity"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    smelt_in(root, &["build"])?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &expected, format!("{name}: generated Rust stdout"))
}

/// `DOMException` is an `Error`, `File` a `Blob`, a `TypeError` an `Error`, and a
/// union arm answers for its base classes.
#[test]
fn builtin_prototype_chains() -> TestResult {
    verify_node_parity("builtin_prototype_chains")
}

/// A declared member beats the builtin of that name on every receiver shape.
#[test]
fn declared_members_win() -> TestResult {
    verify_node_parity("declared_members_win")
}

/// A value thrown from a callback keeps its class, not just a message.
#[test]
fn thrown_from_callback() -> TestResult {
    verify_node_parity("thrown_from_callback")
}

/// A closure's omitted argument takes its default, per call and in the callee.
#[test]
fn closure_parameter_defaults() -> TestResult {
    verify_node_parity("closure_parameter_defaults")
}
