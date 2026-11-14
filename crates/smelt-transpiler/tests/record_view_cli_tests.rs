//! Records seen through an erased slot, end to end against Node.
//!
//! A generated record is one JavaScript object. A slot of another type that
//! receives it holds a view of that object, not a copy
//! (`smelt-codegen-rust/src/emitter/live_view.rs`,
//! `smelt-runtime/src/runtime/live_view.rs`). The views whose slot type is
//! itself typed (`Record<string, number>`, a structural subset, a type
//! parameter) are examples 197-199 under `examples/typescript/end-to-end/`.
//! The fixtures here are the ones whose SOURCE asks for an erased slot
//! (`unknown`, `any`, `Record<string, unknown>`, a type parameter the callee
//! reads properties through), which the examples corpus cannot hold: the
//! erased slot itself is counted there, and its avoidable erasure is pinned at
//! zero (`blocker-logs/smelt-unknown-baseline.json`).
//!
//! - `shared-through-unknown`: a callee typed `unknown` keeps the value and
//!   writes it through a cast back; identity both ways;
//! - `shared-as-an-open-dictionary`: a callee typed `Record<string, unknown>`
//!   writes, adds, deletes and keeps the value; key order afterwards;
//! - `written-through-any`: a callee typed `any` writes, adds and deletes
//!   properties and keeps the value;
//! - `written-through-an-erased-type-parameter`: `T extends { x: number }`,
//!   erased in a callee that reads `p.x`;
//! - `plain-object-cast-to-a-record`: an erased-born object narrowed to a
//!   record type, and a value a generic callee rebuilds in another key order.
//!
//! Two requirements are checked on each, separately:
//!
//! - **behavior**: the generated program prints `expected.stdout`, and
//!   `expected.stdout` is what Node prints for the same source;
//! - **type preservation**: every record binding the fixture names is declared
//!   at its struct type and the struct is the shared-cell handle, so the
//!   fixture fails if sharing were bought by turning the record into a map.

mod common;

use std::{fs, path::Path};

use common::{
    SharedDepsCrate, TempProject, TestResult, cargo_run_shared, ensure, ensure_eq,
    ensure_node_agrees, smelt_shared, utf8_path,
};

/// What one fixture's generated Rust must declare.
struct Typing {
    /// `(binding, struct type)`: a local that must be declared at that record
    /// struct.
    records: &'static [(&'static str, &'static str)],
}

/// Every `.rs` file under `dir`, concatenated.
fn generated_rust(dir: &Path) -> TestResult<String> {
    let mut text = String::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&fs::read_to_string(path)?);
        }
    }
    Ok(text)
}

/// Whether `rust` declares a local `name` of exactly the type `ty`.
fn declares(rust: &str, name: &str, ty: &str) -> bool {
    [
        format!("let {name}: {ty} ="),
        format!("let mut {name}: {ty} ="),
        format!("let {name}: {ty};"),
        format!("let mut {name}: {ty};"),
    ]
    .iter()
    .any(|declaration| rust.contains(declaration.as_str()))
}

/// Builds and runs the fixture `name`, checks its output against
/// `expected.stdout` and Node, and its generated Rust against `typing`.
fn verify_fixture(name: &str, typing: &Typing) -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/record-views")
        .join(name);
    let source = fs::read_to_string(root.join("input.ts"))?;
    let expected = fs::read_to_string(root.join("expected.stdout"))?;
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    let shared = SharedDepsCrate::claim(format!("record_view_{}", name.replace('-', "_")))?;
    let crate_name = shared.name();
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            "[project]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n\
             [output]\ntarget = \"./dist\"\ncrate-name = \"{crate_name}\"\nbuild = false\n\n\
             [runtime]\nclone-strategy = \"aggressive\"\n"
        ),
    )?;
    fs::write(project_path.join("src/main.ts"), &source)?;
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt_shared(&["--manifest-path", &manifest_arg, "build"], &shared)?;

    let rust = generated_rust(&project_path.join("dist/src"))?;
    for (binding, record) in typing.records {
        ensure(
            declares(&rust, binding, record),
            format!("{name}: `{binding}` must be declared at the record struct `{record}`"),
        )?;
        ensure(
            rust.contains(&format!(
                "struct {record}(::std::rc::Rc<::std::cell::RefCell<{record}Inner>>);"
            )),
            format!("{name}: `{record}` must be a shared-cell record struct"),
        )?;
    }

    let actual = cargo_run_shared(&project_path.join("dist/Cargo.toml"), &shared)?;
    ensure_eq(&actual, &expected, format!("unexpected stdout for {name}"))?;
    ensure_node_agrees(&[("main.ts", source.as_str())], "main.ts", &expected, name)
}

/// A callee typed `unknown` keeps the caller's record and writes it through a
/// cast back; the kept alias reads the caller's later writes.
#[test]
fn record_shared_through_unknown_is_the_callers_record() -> TestResult {
    verify_fixture(
        "shared-through-unknown",
        &Typing {
            records: &[("point", "Point"), ("other", "Point")],
        },
    )
}

/// A callee typed `Record<string, unknown>` writes, adds, deletes and keeps
/// the caller's record, which enumerates the result in JavaScript's order.
#[test]
fn record_shared_as_an_open_dictionary_is_the_callers_record() -> TestResult {
    verify_fixture(
        "shared-as-an-open-dictionary",
        &Typing {
            records: &[("point", "Point"), ("back", "Point")],
        },
    )
}

/// A callee typed `any` writes, extends and keeps the caller's record.
#[test]
fn record_written_through_any_is_the_callers_record() -> TestResult {
    verify_fixture(
        "written-through-any",
        &Typing {
            records: &[("point", "Point")],
        },
    )
}

/// A callee generic over an erased `T extends { x: number }` writes the
/// caller's record and returns it.
#[test]
fn record_written_through_an_erased_type_parameter_is_the_callers_record() -> TestResult {
    verify_fixture(
        "written-through-an-erased-type-parameter",
        &Typing {
            records: &[("point", "Point"), ("returned", "Point")],
        },
    )
}

/// A plain object narrowed to a record type becomes that record, and a value
/// a callee rebuilt keeps the callee's key order.
#[test]
fn plain_object_cast_to_a_record_is_that_record() -> TestResult {
    verify_fixture(
        "plain-object-cast-to-a-record",
        &Typing {
            records: &[("typed", "Point"), ("sorted", "Mixed")],
        },
    )
}
