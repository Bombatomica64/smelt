//! Runtime execution tests for `JSON.stringify` over ERASED values.
//!
//! `JSON.stringify`'s output is specified by ECMA-262, not by the Rust
//! serializer, and every one of its rules now lives in one place
//! (`Serialize for SmeltUnknown`) because the emitter erases the value at the
//! call. For a UNION that is the only place the answer can be decided at all:
//! the arm is a run-time fact, and the per-arm answer is that tag's arm in the
//! same impl.
//!
//! The cases here are the ones the examples corpus cannot hold, because each
//! needs a value that is erased today and that corpus keeps a
//! zero-avoidable-erasure invariant. Measured against Node 22 before the fix:
//!
//! | value | Node | Smelt (before) |
//! | --- | --- | --- |
//! | `new Uint8Array([1,2,3])` | `{"0":1,"1":2,"2":3}` | `{}` |
//! | `new Int16Array([-1,300])` | `{"0":-1,"1":300}` | `{}` |
//! | `new ArrayBuffer(2)` | `{}` | `{}` (already right) |
//! | `new DataView(new ArrayBuffer(2))` | `{}` | `{}` (already right) |
//! | `Object.keys(new ArrayBuffer(2))` | `[]` | `["0","1"]` |
//! | `Object.values(new ArrayBuffer(2))` | `[]` | `[0,0]` |
//! | `{a: () => 1, b: 1}` | `{"b":1}` | `{"a":"function () { [native code] }","b":1}` |
//! | `[() => 1, 1]` | `[null,1]` | `["function () { [native code] }",1]` |
//! | `{a: Symbol("s"), b: 1}` | `{"b":1}` | `{"a":"Symbol(s)@32","b":1}` |
//!
//! The distinction that fixes the first six is that own indexed properties
//! belong to an ELEMENT-TYPED view and to nothing else: byte storage and a
//! `DataView` address their bytes through accessors, so they have no properties
//! of their own. `smelt_host_buffer_own_elements` is that rule, and
//! `Object.keys`, `Object.values`, `for...in` and this serializer all read it.
//!
//! Each case is a TypeScript Vitest test lowered to a crate and executed with
//! `cargo test`; a green run means every generated `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test json_stringify_runtime -- --ignored
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
        "generated JSON stringify test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-json-stringify-{}-{seq}",
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
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(&crate_dir, &target_dir);
    });
    // The scratch root holds a whole nested cargo target directory, so it is
    // removed on the FAILURE path too; `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it
    // for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_byte_view_serializes_as_its_element_indices() {
    // The width matters: a view decodes at its own element size and
    // signedness, so an `Int16Array` reports `-1` and `300` rather than the
    // four bytes underneath them.
    let source = r#"
import { test, expect } from "vitest";
test("a byte view serializes as its element indices", () => {
  expect(JSON.stringify(new Uint8Array([1, 2, 3]))).toBe('{"0":1,"1":2,"2":3}');
  expect(JSON.stringify(new Int16Array([-1, 300]))).toBe('{"0":-1,"1":300}');
  expect(JSON.stringify({ payload: new Uint8Array([9]) })).toBe('{"payload":{"0":9}}');
  expect(JSON.stringify([new Uint8Array([9]), 1])).toBe('[{"0":9},1]');
});
"#;
    run_fixture(source, "smelt_json_view_indices");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn byte_storage_and_a_data_view_have_no_own_properties() {
    // `ArrayBuffer.isView(new DataView(..))` is true, yet a `DataView` has no
    // indexed properties: the own-property face is the ELEMENT-TYPED view's,
    // which is why the rule is keyed on the element type and not on the view
    // role.
    let source = r#"
import { test, expect } from "vitest";
test("byte storage and a data view have no own properties", () => {
  const buffer = new ArrayBuffer(2);
  expect(JSON.stringify(buffer)).toBe("{}");
  expect(JSON.stringify(new DataView(buffer))).toBe("{}");
  expect(JSON.stringify(Object.keys(buffer))).toBe("[]");
  expect(JSON.stringify(Object.values(buffer))).toBe("[]");
  expect(JSON.stringify(Object.keys(new DataView(buffer)))).toBe("[]");

  const view = new Uint8Array([1, 2]);
  expect(JSON.stringify(Object.keys(view))).toBe('["0","1"]');
  expect(JSON.stringify(Object.values(view))).toBe("[1,2]");
});
"#;
    run_fixture(source, "smelt_json_storage_no_own_properties");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_non_json_value_is_omitted_as_a_property_and_null_in_an_array() {
    // JSON has no functions, symbols or `undefined`: as a property each is
    // dropped, and in an array each becomes `null`. Serializing the native-code
    // string, or a symbol's description, invented data JavaScript never writes.
    let source = r#"
import { test, expect } from "vitest";
test("a non-JSON value is omitted as a property and null in an array", () => {
  const callback = () => 2;
  expect(JSON.stringify({ keep: 1, drop: callback })).toBe('{"keep":1}');
  expect(JSON.stringify([callback, 1])).toBe("[null,1]");

  const marker = Symbol("s");
  expect(JSON.stringify({ keep: 1, drop: marker })).toBe('{"keep":1}');
  expect(JSON.stringify([marker, 1])).toBe("[null,1]");

  expect(JSON.stringify({ keep: 1, drop: undefined })).toBe('{"keep":1}');
  expect(JSON.stringify([undefined, 1])).toBe("[null,1]");
});
"#;
    run_fixture(source, "smelt_json_non_json_values");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_union_with_a_byte_view_arm_serializes_per_arm() {
    // The shape Hono's `crypto.ts` hands `JSON.stringify`: a union of JSON
    // values, a byte view and byte storage. The arm is a run-time fact, so the
    // per-arm answer is the erased carrier's arm for that tag — which is what
    // makes one call site produce four different, correct answers.
    let source = r#"
import { test, expect } from "vitest";
type Data = string | number | Uint8Array | ArrayBuffer;
function encode(value: Data): string {
  return JSON.stringify(value);
}
test("a union with a byte view arm serializes per arm", () => {
  expect(encode("s")).toBe('"s"');
  expect(encode(7)).toBe("7");
  expect(encode(new Uint8Array([1, 2]))).toBe('{"0":1,"1":2}');
  expect(encode(new ArrayBuffer(2))).toBe("{}");
});
"#;
    run_fixture(source, "smelt_json_union_arms");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_operand_that_is_already_erased_serializes_through_the_runtime_rule() {
    // The emitter's own `JsonStringify` arm used to REJECT an operand whose
    // static type was already `SmeltUnknown`, which is exactly the case the
    // runtime rule exists for: erasing it kept nothing concrete, and rejecting
    // it refused to serialize the one shape `Serialize for SmeltUnknown` was
    // written to decide. Hono's whole crate stopped in the emitter on it —
    // `utils/crypto.ts` reassigns `data: string | boolean | number | JSONValue
    // | ArrayBufferView | ArrayBuffer` across a narrowing, so the operand's
    // static type is the erased union rather than any one arm.
    //
    // These cases live in the tier rather than in the examples corpus for the
    // same reason the rest of this file does: an `unknown` parameter and an
    // erased union are erased BY CONSTRUCTION, and the corpus holds a hard
    // zero-avoidable-erasure invariant. Every expectation below is Node 22's.
    let source = r#"
import { test, expect } from "vitest";

function stringifyUnknown(value: unknown): string {
  return JSON.stringify(value);
}

function stringifyGeneric<T>(value: T): string {
  return JSON.stringify(value);
}

type JSONValue = string | number | boolean | null;
type Data = string | boolean | number | JSONValue | ArrayBufferView | ArrayBuffer;

function createHashish(value: Data): string {
  let data: Data = value;
  if (typeof data === "boolean") {
    data = data ? 1 : 0;
  }
  return JSON.stringify(data);
}

test("an `unknown` operand serializes by its runtime tag", () => {
  expect(stringifyUnknown(1)).toBe("1");
  expect(stringifyUnknown(1.5)).toBe("1.5");
  expect(stringifyUnknown("text")).toBe('"text"');
  expect(stringifyUnknown(true)).toBe("true");
  expect(stringifyUnknown(null)).toBe("null");
  expect(stringifyUnknown([1, "a", false])).toBe('[1,"a",false]');
  expect(stringifyUnknown({ a: 1, b: "two" })).toBe('{"a":1,"b":"two"}');
});
test("a generic operand is legal for every instantiation", () => {
  expect(stringifyGeneric(2.5)).toBe("2.5");
  expect(stringifyGeneric("g")).toBe('"g"');
  expect(stringifyGeneric(false)).toBe("false");
});
test("a union reassigned across a narrowing serializes per arm", () => {
  expect(createHashish("s")).toBe('"s"');
  expect(createHashish(3)).toBe("3");
  expect(createHashish(true)).toBe("1");
  expect(createHashish(false)).toBe("0");
  expect(createHashish(new Uint8Array([1, 2]))).toBe('{"0":1,"1":2}');
  expect(createHashish(new ArrayBuffer(2))).toBe("{}");
});
"#;
    run_fixture(source, "smelt_json_stringify_erased_operand");
}
