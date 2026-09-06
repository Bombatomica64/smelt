//! Runtime execution tests for the WHATWG fetch types.
//!
//! `Headers` is modeled as the concrete generated `SmeltHeaders` type rather
//! than a tagged record, and the whole value of that choice is *behavioural*:
//! the spec's observable semantics have to hold at runtime, and every one of
//! them is invisible to the compile-only corpus and to string goldens.
//!
//! 1. **Names are case-insensitive.** `set("X-Trace", ..)` is read back by
//!    `get("x-trace")`. A map keyed by the source spelling type-checks, compiles
//!    and answers `null` for the other case.
//! 2. **`get` joins every value with `", "`.** Two `Accept` headers read back as
//!    one comma-joined string; a last-write-wins map silently loses the first.
//! 3. **`get` of an absent name is `null`, not `""`.** The lowered type is
//!    `Option<String>`, so the difference is observable to the caller rather
//!    than collapsed into a falsy string.
//! 4. **`set` replaces every value, `append` keeps them.** These differ only
//!    after a repeat, which a single-value model cannot express.
//! 5. **`Set-Cookie` is never combined.** `getSetCookie()` yields one entry per
//!    cookie and iteration keeps them separate, which is the spec's one
//!    carve-out from the combining rule above.
//! 6. **Iteration is sorted by name**, not insertion-ordered, and values are
//!    combined per name.
//! 7. **A `Headers` is a reference value.** Two bindings to one header list
//!    observe each other's mutations, and `new Headers(other)` is a copy that
//!    does not.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test fetch_types_runtime -- --ignored
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
        "generated fetch-type test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-fetch-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_fetch_fixture(source: &str, crate_name: &str) {
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
fn headers_reads_follow_the_whatwg_semantics() {
    let source = r#"
import { test, expect } from "vitest";
test("header names are case-insensitive in both directions", () => {
  const headers = new Headers({ "Content-Type": "text/plain" });
  expect(headers.get("content-type")).toBe("text/plain");
  expect(headers.get("CONTENT-TYPE")).toBe("text/plain");
  expect(headers.has("Content-Type")).toBe(true);
  headers.set("X-Trace", "abc");
  expect(headers.get("x-trace")).toBe("abc");
});
test("get joins every value for a name with a comma and a space", () => {
  const headers = new Headers();
  headers.append("accept", "text/html");
  headers.append("Accept", "application/json");
  expect(headers.get("accept")).toBe("text/html, application/json");
});
test("get of an absent name is null, not the empty string", () => {
  const headers = new Headers({ "content-type": "text/plain" });
  expect(headers.get("missing")).toBe(null);
  expect(headers.has("missing")).toBe(false);
});
test("set replaces every value while append keeps them", () => {
  const headers = new Headers();
  headers.append("accept", "one");
  headers.append("accept", "two");
  expect(headers.get("accept")).toBe("one, two");
  headers.set("accept", "three");
  expect(headers.get("accept")).toBe("three");
});
test("delete removes every value for a name", () => {
  const headers = new Headers();
  headers.append("accept", "one");
  headers.append("accept", "two");
  headers.delete("Accept");
  expect(headers.get("accept")).toBe(null);
  expect(headers.has("accept")).toBe(false);
});
test("values are normalized by stripping surrounding whitespace", () => {
  const headers = new Headers();
  headers.set("x-value", "  spaced  ");
  expect(headers.get("x-value")).toBe("spaced");
});
"#;
    run_fetch_fixture(source, "headers_reads");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn headers_set_cookie_is_never_combined() {
    let source = r#"
import { test, expect } from "vitest";
test("getSetCookie returns one entry per cookie", () => {
  const headers = new Headers();
  headers.append("Set-Cookie", "a=1");
  headers.append("set-cookie", "b=2");
  const cookies = headers.getSetCookie();
  expect(cookies.length).toBe(2);
  expect(cookies[0]).toBe("a=1");
  expect(cookies[1]).toBe("b=2");
});
test("iteration keeps each set-cookie entry separate", () => {
  const headers = new Headers();
  headers.append("set-cookie", "a=1");
  headers.append("set-cookie", "b=2");
  headers.set("accept", "text/html");
  const names: string[] = [];
  const values: string[] = [];
  for (const [name, value] of headers.entries()) {
    names.push(name);
    values.push(value);
  }
  expect(names.join(",")).toBe("accept,set-cookie,set-cookie");
  expect(values.join("|")).toBe("text/html|a=1|b=2");
});
test("a header list with no cookies has no set-cookie entries", () => {
  const headers = new Headers({ accept: "text/html" });
  expect(headers.getSetCookie().length).toBe(0);
});
"#;
    run_fetch_fixture(source, "headers_set_cookie");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn headers_iteration_is_sorted_and_combined() {
    let source = r#"
import { test, expect } from "vitest";
test("keys are sorted and deduplicated", () => {
  const headers = new Headers();
  headers.append("x-last", "1");
  headers.append("accept", "2");
  headers.append("accept", "3");
  expect([...headers.keys()].join(",")).toBe("accept,x-last");
});
test("values follow the sorted names and are combined per name", () => {
  const headers = new Headers();
  headers.append("x-last", "1");
  headers.append("accept", "2");
  headers.append("accept", "3");
  expect([...headers.values()].join("|")).toBe("2, 3|1");
});
"#;
    run_fetch_fixture(source, "headers_iteration");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn headers_is_a_reference_value_and_the_constructor_copies() {
    let source = r#"
import { test, expect } from "vitest";
test("two bindings to one header list share its mutations", () => {
  const headers = new Headers({ accept: "text/html" });
  const alias = headers;
  alias.set("accept", "application/json");
  expect(headers.get("accept")).toBe("application/json");
});
test("new Headers(other) copies, so later writes are not shared", () => {
  const source = new Headers({ accept: "text/html" });
  const copy = new Headers(source);
  copy.set("accept", "application/json");
  expect(source.get("accept")).toBe("text/html");
  expect(copy.get("accept")).toBe("application/json");
});
test("a sequence-of-pairs initializer appends in order", () => {
  const headers = new Headers([
    ["accept", "text/html"],
    ["Accept", "application/json"],
  ]);
  expect(headers.get("accept")).toBe("text/html, application/json");
});
test("an empty constructor starts with no headers", () => {
  const headers = new Headers();
  expect([...headers.keys()].length).toBe(0);
  expect(headers.get("accept")).toBe(null);
});
"#;
    run_fetch_fixture(source, "headers_reference");
}
