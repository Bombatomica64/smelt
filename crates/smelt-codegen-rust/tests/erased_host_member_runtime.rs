//! Runtime execution tests for MODELED MEMBERS read off an ERASED host record.
//!
//! A modeled host value that crosses the dynamic boundary — `as any`, an
//! `any`-typed field, a JSON-shaped bag — becomes a marker-bearing record, and its
//! METHODS stay callable on it. The mutation case proves both requirements:
//!
//! 1. **The member resolves.** Each modeled class with an erasure adapter
//!    contributes its own resolver to `smelt_host_method`, and the erased-read
//!    helper asks before falling back to the record's own keys (an own key still
//!    wins, as JavaScript's own-property lookup does).
//! 2. **The value is the SAME one.** `Headers`, `URLSearchParams` and `FormData`
//!    retain their live value in the origin registry on erasure and restore it
//!    on recovery, so `set` followed by `get` sees the write.
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

#[path = "harness/mod.rs"]
mod harness;

use harness::{emit_program, run_generated_tests};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};




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
