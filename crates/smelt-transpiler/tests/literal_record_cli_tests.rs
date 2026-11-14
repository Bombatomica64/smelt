//! Object literals that must keep the map lowering, end to end against Node.
//!
//! An unannotated object literal lowers to a record struct only when every
//! use of its binding keeps the value typed
//! (`smelt-frontend-ts/src/lowering/literal_record_uses.rs`). Each fixture
//! under `tests/fixtures/literal-records/` is one family of uses that
//! disqualifies a literal:
//!
//! - `identity-stays-a-map`: `===`, `Object.is`, `Set`/array membership;
//! - `keys-stay-a-map`: computed keys, `in`, `Object.keys`/`values`/`entries`,
//!   `for..in`, a spread, integer-like keys;
//! - `passed-or-stored-stays-a-map`: an argument of a function that writes
//!   through it or rebuilds it in another key order, a literal stored in a
//!   container or returned, a record-valued property replaced.
//!
//! Two requirements are checked on each, separately:
//!
//! - **behavior**: the generated program prints `expected.stdout`, and
//!   `expected.stdout` is what Node prints for the same source;
//! - **lowering**: every literal binding the fixture names is declared as the
//!   map (`SmeltRecord<JsString, SmeltUnknown>`), and a control binding used
//!   only through typed reads is declared as a record struct, so the fixture
//!   fails if the rule stops applying in either direction.
//!
//! The fixtures live here and not under `examples/`: a literal that stays a
//! map is avoidable erasure by definition, and the examples corpus holds none
//! (`blocker-logs/smelt-unknown-baseline.json`). The literal that takes the
//! struct is `examples/typescript/end-to-end/196_object_literal_takes_its_checker_record`.

mod common;

use std::{fs, path::Path};

use common::{
    SharedDepsCrate, TempProject, TestResult, cargo_run_shared, ensure, ensure_eq,
    ensure_node_agrees, smelt_shared, utf8_path,
};

/// The Rust type a literal that keeps the map lowering is declared at.
const MAP_TYPE: &str = "SmeltRecord<JsString, SmeltUnknown>";

/// What one fixture's generated Rust must declare.
struct Lowering {
    /// Bindings declared from an object literal that must stay maps.
    maps: &'static [&'static str],
    /// Bindings declared from an object literal that must be record structs.
    structs: &'static [&'static str],
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

/// Whether `rust` declares a local `name` whose type starts with `ty`.
fn declares(rust: &str, name: &str, ty: &str) -> bool {
    [format!("let {name}: {ty}"), format!("let mut {name}: {ty}")]
        .iter()
        .any(|declaration| rust.contains(declaration.as_str()))
}

/// Builds and runs the fixture `name`, checks its output against
/// `expected.stdout` and Node, and its generated Rust against `lowering`.
fn verify_fixture(name: &str, lowering: &Lowering) -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/literal-records")
        .join(name);
    let source = fs::read_to_string(root.join("input.ts"))?;
    let expected = fs::read_to_string(root.join("expected.stdout"))?;
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    let shared = SharedDepsCrate::claim(format!("literal_record_{}", name.replace('-', "_")))?;
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
    for binding in lowering.maps {
        ensure(
            declares(&rust, binding, MAP_TYPE),
            format!("{name}: `{binding}` must keep the map lowering (`{MAP_TYPE}`)"),
        )?;
    }
    for binding in lowering.structs {
        ensure(
            declares(&rust, binding, "SmeltRecord") && !declares(&rust, binding, "SmeltRecord<"),
            format!("{name}: `{binding}` must lower to a record struct"),
        )?;
    }

    let actual = cargo_run_shared(&project_path.join("dist/Cargo.toml"), &shared)?;
    ensure_eq(&actual, &expected, format!("unexpected stdout for {name}"))?;
    ensure_node_agrees(&[("main.ts", source.as_str())], "main.ts", &expected, name)
}

/// A literal whose identity is observed keeps the map; `present`, which is
/// only read and compared with `undefined`/`null`, is a struct.
#[test]
fn literal_with_observed_identity_stays_a_map() -> TestResult {
    verify_fixture(
        "identity-stays-a-map",
        &Lowering {
            maps: &["origin", "tracked", "first", "second", "third"],
            structs: &["present"],
        },
    )
}

/// A literal whose key set or key order is observed keeps the map.
#[test]
fn literal_with_observed_keys_stays_a_map() -> TestResult {
    verify_fixture(
        "keys-stay-a-map",
        &Lowering {
            maps: &["counters", "settings", "listed", "mixed"],
            structs: &[],
        },
    )
}

/// A literal passed to a function, stored, returned, or with a record-valued
/// property replaced keeps the map: the callee's writes reach it and the
/// rebuilt value keeps the callee's key order. `replacement` is annotated, so
/// it is a struct with or without the rule.
#[test]
fn literal_passed_or_stored_stays_a_map() -> TestResult {
    verify_fixture(
        "passed-or-stored-stays-a-map",
        &Lowering {
            maps: &["counter", "unordered", "first", "made", "outer"],
            structs: &["replacement"],
        },
    )
}
