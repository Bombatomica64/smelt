//! Runtime execution tests for MODELED MEMBERS read off an ERASED host record.
//!
//! A modeled host value that crosses the dynamic boundary — `as any`, an
//! `any`-typed field, a JSON-shaped bag — becomes a marker-bearing record. Its
//! data properties were readable from the record, but its METHODS were not:
//! the member read answered `undefined`, the optional-call adapter substituted
//! a no-op default callback, and the call produced `null`. Eight of nine probes
//! were wrong against Node 22 before this tier existed:
//!
//! | read | Node | Smelt (before) |
//! | --- | --- | --- |
//! | `(headers as any).get("a")` | `b` | `null` |
//! | `(headers as any).set(..)` then `get` | `d` | `null` |
//! | `(headers as any).has("a")` | `true` | `null` |
//! | `(params as any).get("y")` | `2` | `null` |
//! | `String(params as any)` | `x=1&y=2` | `[object Object]` |
//! | `(form as any).get("k")` | `v` | `null` |
//! | `(encoder as any).encode("ab").length` | `2` | `null` |
//! | `(blob as any).size` | `2` | `2` (a data property, already right) |
//!
//! Two things had to be true for the fix, and the mutation case is what proves
//! both:
//!
//! 1. **The member has to resolve.** `smelt_host_method` knew only the abort
//!    surface; each modeled class with an erasure adapter now contributes its
//!    own resolver, and the erased-read helper asks before falling back to the
//!    record's own keys (an own key still wins, as JavaScript's own-property
//!    lookup does).
//! 2. **The value has to be the SAME one.** A resolver that rebuilt the value
//!    from the record would make `set` write to a copy, so the next `get` would
//!    answer the stale record. `Headers`, `URLSearchParams` and `FormData` now
//!    retain their live value in the origin registry on erasure and restore it
//!    on recovery — the shape the shared adapter emitter already used for the
//!    codecs and the `node:http` types.
//!
//! Only the SYNCHRONOUS members are resolved. The async body readers
//! (`response.text()`, `blob.arrayBuffer()`) are not, and keep the erased
//! read's `undefined`.
//!
//! Each case is a TypeScript Vitest test lowered to a crate and executed with
//! `cargo test`; a green run means every generated `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_host_member_runtime -- --ignored
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
        "generated erased-host-member test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-erased-host-member-{}-{seq}",
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
    // removed on the FAILURE path too: leaving one behind per failing case is
    // what fills `/tmp`. `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it for a
    // debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_header_list_resolves_its_members_on_the_same_value() {
    // The mutation is the load-bearing half: a resolver that rebuilt the value
    // from the record would make `set` write to a copy, and the `get` after it
    // would read the stale record. It also has to be visible on the CONCRETE
    // value the program still holds, which is the second block.
    let source = r#"
import { test, expect } from "vitest";
test("an erased header list resolves its members", () => {
  const headers = new Headers({ a: "b" });
  const erased: any = headers;
  expect(erased.get("a")).toBe("b");
  expect(erased.has("a")).toBe(true);
  expect(erased.has("nope")).toBe(false);

  erased.set("c", "d");
  expect(erased.get("c")).toBe("d");
  expect(headers.get("c")).toBe("d");

  erased.append("c", "e");
  expect(headers.get("c")).toBe("d, e");
  erased.delete("c");
  expect(headers.get("c")).toBe(null);

  expect(erased.get("missing")).toBe(null);
});
"#;
    run_fixture(source, "smelt_erased_headers_members");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_parameter_list_resolves_its_members_and_stringifies_as_its_query() {
    // `URLSearchParams` overrides `toString` in the spec, so `String(params)`
    // is the query string — the erased coercion answered `[object Object]`,
    // which is what a record with no override gives.
    let source = r#"
import { test, expect } from "vitest";
test("an erased parameter list resolves its members", () => {
  const params = new URLSearchParams("x=1&y=2");
  const erased: any = params;
  expect(erased.get("y")).toBe("2");
  expect(erased.has("x")).toBe(true);
  expect(String(erased)).toBe("x=1&y=2");

  erased.append("z", "3");
  expect(params.get("z")).toBe("3");
  expect(String(erased)).toBe("x=1&y=2&z=3");
});
"#;
    run_fixture(source, "smelt_erased_params_members");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_form_and_codec_resolve_their_members() {
    // A form's entry values are `string | File`, so the resolver erases each
    // entry rather than assuming text. The codec case is the one whose result
    // is itself a host value: `encode` answers a byte view, and reading its
    // `length` off the erased result is what a caller does next.
    let source = r#"
import { test, expect } from "vitest";
test("an erased form resolves its members", () => {
  const form = new FormData();
  form.append("k", "v");
  const erased: any = form;
  expect(erased.get("k")).toBe("v");
  expect(erased.has("k")).toBe(true);
  erased.append("k2", "v2");
  expect(form.get("k2")).toBe("v2");
});

test("an erased text codec resolves its members", () => {
  const encoder: any = new TextEncoder();
  const bytes = encoder.encode("ab");
  expect(bytes.length).toBe(2);
  const decoder: any = new TextDecoder();
  expect(decoder.decode(bytes)).toBe("ab");
});
"#;
    run_fixture(source, "smelt_erased_form_codec_members");
}
