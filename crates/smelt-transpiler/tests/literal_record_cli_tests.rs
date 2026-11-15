//! Which unannotated object literals become record structs, end to end
//! against Node.
//!
//! An unannotated object literal lowers to a record struct when every use of
//! its binding keeps the value typed or hands it to a slot that shares it
//! (`smelt-frontend-ts/src/lowering/literal_record_uses.rs`). Each fixture
//! under `tests/fixtures/literal-records/` is one family of uses:
//!
//! - `identity-stays-a-map` (the name is historical): a literal compared
//!   with `===`, or handed to `Set.add`/`has`, `push`/`indexOf` or an
//!   `unknown` parameter, is a struct and keeps its identity there; one
//!   aliased at an annotated binding keeps the map;
//! - `keys-stay-a-map`: computed keys, `in`, `for..in`, a spread, integer-like
//!   keys keep the map;
//! - `passed-or-stored-stays-a-map`: a literal passed to callees typed
//!   `Record<string, unknown>`, `any` or a type parameter is a struct (the
//!   callee writes the caller's object, and a rebuilt value keeps the callee's
//!   key order); one stored in a container, returned, or with a record-valued
//!   property replaced keeps the map;
//! - `passed-to-another-record-type-stays-a-map`: a literal passed to a
//!   parameter of another record type keeps the map.
//! - `stored-at-another-record-type-stays-a-map`: a literal stored in or
//!   returned at a slot of another record type, or whose record-valued
//!   property is replaced by another record type, keeps the map.
//!
//! Two requirements are checked on each, separately:
//!
//! - **behavior**: the generated program prints `expected.stdout`, and
//!   `expected.stdout` is what Node prints for the same source;
//! - **lowering**: every literal binding the fixture names is declared as the
//!   map (`SmeltRecord<JsString, SmeltUnknown>`) or as a record struct, as
//!   listed, so the fixture fails if the rule moves in either direction.
//!
//! # Expectations corrected when call arguments stopped disqualifying
//!
//! - **Old requirement**: a literal passed to ANY call other than
//!   `console.*`, `JSON.stringify` and the deep-equality matchers keeps the
//!   map, and so does one compared with `===` (`origin`, `tracked`, `first`,
//!   `second` of the identity fixture; `counter`, `unordered` of the
//!   passed-or-stored one were asserted to be maps).
//! - **Evidence**: that requirement existed because a struct handed to a slot
//!   of another type was copied. It no longer is where the slot holds a view
//!   (`smelt-codegen-rust/src/emitter/live_view.rs`): the same programs,
//!   lowered as structs, print what Node prints, which each fixture's
//!   `expected.stdout` (unchanged) checks.
//! - **New requirement**: those six literals are structs; the behavior
//!   requirement is the same one, on the same output.
//!
//! The fixtures live here and not under `examples/`: a literal that stays a
//! map is avoidable erasure by definition, and the examples corpus holds none
//! (`blocker-logs/smelt-unknown-baseline.json`). The literal that takes the
//! struct is `examples/typescript/end-to-end/197_object_literal_takes_its_checker_record`.

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

/// A literal whose identity is observed is a struct: `origin` and `tracked`
/// are handed to collection methods that find them again, `first` and
/// `second` are compared with `===`, `present` with `undefined`/`null`.
/// `third` is aliased at an annotated `unknown` binding, a use the rule does
/// not model, and keeps the map.
#[test]
fn literal_with_observed_identity_keeps_its_identity() -> TestResult {
    verify_fixture(
        "identity-stays-a-map",
        &Lowering {
            maps: &["third"],
            structs: &["origin", "tracked", "first", "second", "present"],
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

/// A literal stored in a container, returned, or with a record-valued
/// property replaced keeps the map. `counter` and `unordered` are only passed
/// to callees whose parameters hold a view (`Record<string, unknown>`, `any`,
/// a type parameter) and are structs: the callee's writes reach them and the
/// rebuilt value keeps the callee's key order. `replacement` is annotated.
#[test]
fn literal_passed_or_stored_stays_a_map() -> TestResult {
    verify_fixture(
        "passed-or-stored-stays-a-map",
        &Lowering {
            maps: &["first", "made", "outer"],
            structs: &["counter", "unordered", "replacement"],
        },
    )
}

/// A literal stored in or returned at a slot of another record type, or whose
/// record-valued property is replaced by a value of another record type,
/// keeps the map. `replacement` is annotated with a type alias, so it is a
/// struct named after the alias, not one of the literal structs this checks.
#[test]
fn literal_stored_at_another_record_type_stays_a_map() -> TestResult {
    verify_fixture(
        "stored-at-another-record-type-stays-a-map",
        &Lowering {
            maps: &["kept", "made", "outer"],
            structs: &[],
        },
    )
}

/// A literal passed to a parameter of another record type keeps the map, which
/// the callee's record adopts; `viewed`, passed only to a `Record<..>`
/// parameter, is a struct.
#[test]
fn literal_passed_to_another_record_type_stays_a_map() -> TestResult {
    verify_fixture(
        "passed-to-another-record-type-stays-a-map",
        &Lowering {
            maps: &["moved", "first"],
            structs: &["viewed"],
        },
    )
}
