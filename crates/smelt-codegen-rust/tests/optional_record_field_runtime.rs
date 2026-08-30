//! Runtime execution tests for optional fields in a record-to-interface projection.
//!
//! An object literal passed where an interface is expected is emitted as a
//! record and then projected field by field into the target struct. For a field
//! declared `T | undefined` the projection asked whether the record's value type
//! could render as the INNER `T` — and when it could not, wrote `None` and moved
//! on. A record whose value type is already `Option<T>` (an object literal in
//! which some property is itself optional, so every value is optional-shaped)
//! failed that question, so a property that was plainly present was dropped and
//! the callee saw the field's default.
//!
//! That is the "unmodeled member silently becomes a value" class, and it is
//! invisible to every gate but execution: the projection type-checks, compiles,
//! and quietly answers the default. es-toolkit's `debounce` / `throttle` lost
//! their `{ edges: [...] }` option to it, so both behaved with default edges —
//! `edges: ['leading']` never fired the leading edge and `edges: ['trailing']`
//! fired the leading edge it was told to suppress.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test optional_record_field_runtime -- --ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Lowers `source` through the real pipeline and emits a runnable program crate.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// Runs `cargo test` on the emitted crate; a passing run means every generated
/// `expect(...)` assertion held at runtime.
fn run_generated_tests(crate_dir: &Path, target_dir: &Path) {
    let output = Command::new(env!("CARGO"))
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test");
    assert!(
        output.status.success(),
        "generated optional-record-field test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-optional-record-field-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_optional_interface_field_survives_an_optional_shaped_record() {
    // The reduced es-toolkit `debounce({ edges: ['leading'] })` shape: an options
    // interface whose every member is optional, so the object literal's record
    // value type is optional-shaped too. Before the fix `options.edges` arrived
    // as `undefined` and the callee took its default branch.
    let source = r#"
import { test, expect } from "vitest";
interface Options {
  edges?: Array<"leading" | "trailing">;
  label?: string;
}
function describeOptions({ edges, label }: Options = {}): string {
  const leading = edges != null && edges.includes("leading");
  const trailing = edges == null || edges.includes("trailing");
  return `${label ?? "none"}:${leading}:${trailing}`;
}
test("an optional array option reaches the callee", () => {
  expect(describeOptions({ edges: ["leading"] })).toBe("none:true:false");
  expect(describeOptions({ edges: ["trailing"] })).toBe("none:false:true");
  expect(describeOptions({ edges: ["leading", "trailing"] })).toBe("none:true:true");
  expect(describeOptions({})).toBe("none:false:true");
  expect(describeOptions()).toBe("none:false:true");
});
test("a second optional option in the same literal also reaches the callee", () => {
  expect(describeOptions({ edges: ["leading"], label: "hi" })).toBe("hi:true:false");
  expect(describeOptions({ label: "hi" })).toBe("hi:false:true");
});
"#;
    run_fixture(source, "smelt_optional_record_field_options");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn optional_scalar_and_object_fields_survive_too() {
    // Nothing about the defect was array-specific: any optional field whose
    // record value arrives already optional-shaped was dropped. An absent
    // property must still read as `undefined`, which is what the dropped-field
    // behaviour was indistinguishable from.
    let source = r#"
import { test, expect } from "vitest";
interface Nested {
  n?: number;
}
interface Config {
  count?: number;
  name?: string;
  flag?: boolean;
  nested?: Nested;
}
function readConfig(config: Config): string {
  return [
    config.count === undefined ? "-" : String(config.count),
    config.name === undefined ? "-" : config.name,
    config.flag === undefined ? "-" : String(config.flag),
    config.nested === undefined ? "-" : String(config.nested.n),
  ].join("/");
}
test("optional scalars and nested objects survive the projection", () => {
  expect(readConfig({ count: 3, name: "a", flag: false, nested: { n: 7 } })).toBe("3/a/false/7");
  expect(readConfig({ count: 0 })).toBe("0/-/-/-");
  expect(readConfig({ flag: true })).toBe("-/-/true/-");
  expect(readConfig({})).toBe("-/-/-/-");
});
"#;
    run_fixture(source, "smelt_optional_record_field_scalars");
}
