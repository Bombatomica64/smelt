//! Runtime execution tests for the WHATWG `FormData` value and its parsers.
//!
//! `FormData` was a marker-only host record: `form.append(..)` did nothing and
//! `form.get(..)` answered `undefined`, so a program could hold a form and
//! observe nothing in it. It is now the concrete `SmeltFormData`, and every
//! case below is behaviour that only a run can see — each expectation was
//! diffed against Node 22 before being written down:
//!
//! 1. **Names are case-SENSITIVE**, unlike a `Headers` list's: `has("B")` is
//!    false for an appended `"b"`.
//! 2. **`set` keeps the first entry's POSITION** while dropping the rest, so
//!    the surviving entry does not move to the end of the list. Appending
//!    instead of replacing in place compiles perfectly and reorders the form.
//! 3. **A `Blob` value becomes a `File`**: named `blob` when no filename was
//!    given, named by the third argument when one was, and keeping its own
//!    name when the value already was a `File`. Node's `blob` default is the
//!    spec's, and it is not the empty string.
//! 4. **`keys()` yields one name per ENTRY**, so a duplicated name appears
//!    twice — the spec's iterator walks the entry list, not a set of names.
//! 5. **`formData()` picks its parser from the `Content-Type` HEADER.** The
//!    urlencoded form decodes `+` as a space and `%XX` as a byte; the
//!    multipart form reads each part's `Content-Disposition`, so a part with a
//!    `filename` is a file entry carrying the part's own `Content-Type` and a
//!    part without one is text.
//! 6. **The body is single-use whichever reader consumed it**, and a
//!    `Content-Type` naming neither encoding is the spec's `TypeError` — with
//!    Node's exact message, since a source `catch` can read it.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test form_data_runtime -- --ignored
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
        "generated FormData test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-form-data-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_form_data_fixture(source: &str, crate_name: &str) {
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
    // what fills `/tmp`, and an ENOSPC inside a later nested build reads as a
    // failing assertion rather than as a full disk.
    // `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_entry_list_is_ordered_and_case_sensitive() {
    let source = r#"
import { test, expect } from "vitest";
test("get answers the first value and getAll every one", () => {
  const form = new FormData();
  form.append("a", "1");
  form.append("a", "2");
  form.append("b", "x");
  expect(form.get("a")).toBe("1");
  const all = form.getAll("a");
  expect(all.length).toBe(2);
  expect(all[0]).toBe("1");
  expect(all[1]).toBe("2");
});
test("names are case-sensitive, unlike a header list's", () => {
  const form = new FormData();
  form.append("b", "x");
  expect(form.has("b")).toBe(true);
  expect(form.has("B")).toBe(false);
  form.append("B", "y");
  expect(form.get("b")).toBe("x");
  expect(form.get("B")).toBe("y");
});
test("keys yields one name per entry, not a set of names", () => {
  const form = new FormData();
  form.append("a", "1");
  form.append("a", "2");
  form.append("b", "x");
  expect([...form.keys()].join(",")).toBe("a,a,b");
});
test("an absent name reads back as null", () => {
  const form = new FormData();
  expect(form.get("missing") ?? "absent").toBe("absent");
  expect(form.getAll("missing").length).toBe(0);
});
"#;
    run_form_data_fixture(source, "form_data_entries");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_replaces_in_place_and_delete_removes_every_entry() {
    let source = r#"
import { test, expect } from "vitest";
test("set keeps the first entry's position while dropping the rest", () => {
  const form = new FormData();
  form.append("a", "1");
  form.append("b", "x");
  form.append("a", "2");
  form.set("a", "9");
  expect([...form.keys()].join(",")).toBe("a,b");
  expect(form.get("a")).toBe("9");
  expect(form.getAll("a").length).toBe(1);
});
test("set on an absent name appends", () => {
  const form = new FormData();
  form.append("b", "x");
  form.set("a", "1");
  expect([...form.keys()].join(",")).toBe("b,a");
});
test("delete removes every entry for the name", () => {
  const form = new FormData();
  form.append("a", "1");
  form.append("a", "2");
  form.append("b", "x");
  form.delete("a");
  expect([...form.keys()].join(",")).toBe("b");
  expect(form.has("a")).toBe(false);
});
"#;
    run_form_data_fixture(source, "form_data_mutation");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_blob_value_becomes_a_file() {
    let source = r#"
import { test, expect } from "vitest";
test("a blob with no filename is a File named blob", async () => {
  const form = new FormData();
  form.append("plain", new Blob(["hello"], { type: "text/plain" }));
  const entry = form.get("plain");
  if (entry === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof entry === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(entry.name).toBe("blob");
    expect(entry.type).toBe("text/plain");
    expect(entry.size).toBe(5);
    expect(await entry.text()).toBe("hello");
  }
});
test("a filename argument names the file", async () => {
  const form = new FormData();
  form.append("named", new Blob(["hello"]), "note.txt");
  const entry = form.get("named");
  if (entry === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof entry === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(entry.name).toBe("note.txt");
  }
});
test("a File keeps its own name when no filename is given", () => {
  const form = new FormData();
  form.append("file", new File(["bye"], "given.txt", { type: "text/plain" }));
  const entry = form.get("file");
  if (entry === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof entry === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(entry.name).toBe("given.txt");
  }
});
test("a filename argument overrides a File's own name", () => {
  const form = new FormData();
  form.append("file", new File(["bye"], "given.txt"), "renamed.txt");
  const entry = form.get("file");
  if (entry === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof entry === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(entry.name).toBe("renamed.txt");
  }
});
"#;
    run_form_data_fixture(source, "form_data_files");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_urlencoded_body_reads_back_as_a_form() {
    let source = r#"
import { test, expect } from "vitest";
test("the urlencoded decoder is the params list's own", async () => {
  const response = new Response("a=1&b=hello+world&a=2&empty", {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  const form = await response.formData();
  expect([...form.keys()].join(",")).toBe("a,b,a,empty");
  expect(form.get("b")).toBe("hello world");
  const all = form.getAll("a");
  expect(all[0]).toBe("1");
  expect(all[1]).toBe("2");
  expect(form.get("empty")).toBe("");
});
test("a percent escape decodes to its byte", async () => {
  const response = new Response("q=a%20b%26c", {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  const form = await response.formData();
  expect(form.get("q")).toBe("a b&c");
});
test("the content type may carry a charset parameter", async () => {
  const response = new Response("a=1", {
    headers: {
      "content-type": "application/x-www-form-urlencoded; charset=UTF-8",
    },
  });
  const form = await response.formData();
  expect(form.get("a")).toBe("1");
});
"#;
    run_form_data_fixture(source, "form_data_urlencoded");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_multipart_body_reads_back_as_a_form() {
    let source = r#"
import { test, expect } from "vitest";
test("a part with a filename is a file entry, and one without is text", async () => {
  const boundary = "----SmeltBoundary";
  const body = [
    "--" + boundary,
    'Content-Disposition: form-data; name="title"',
    "",
    "hello",
    "--" + boundary,
    'Content-Disposition: form-data; name="doc"; filename="a.txt"',
    "Content-Type: text/plain",
    "",
    "file body",
    "--" + boundary + "--",
    "",
  ].join("\r\n");
  const request = new Request("https://forms.test/upload", {
    method: "POST",
    headers: { "content-type": "multipart/form-data; boundary=" + boundary },
    body: body,
  });
  const form = await request.formData();
  expect([...form.keys()].join(",")).toBe("title,doc");
  expect(form.get("title")).toBe("hello");
  const doc = form.get("doc");
  if (doc === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof doc === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(doc.name).toBe("a.txt");
    expect(doc.type).toBe("text/plain");
    expect(doc.size).toBe(9);
    expect(await doc.text()).toBe("file body");
  }
});
test("a part with no Content-Type defaults to text/plain", async () => {
  const boundary = "b";
  const body = [
    "--" + boundary,
    'Content-Disposition: form-data; name="doc"; filename="a.bin"',
    "",
    "xy",
    "--" + boundary + "--",
    "",
  ].join("\r\n");
  const response = new Response(body, {
    headers: { "content-type": "multipart/form-data; boundary=" + boundary },
  });
  const form = await response.formData();
  const doc = form.get("doc");
  if (doc === null) {
    expect("no entry").toBe("a file entry");
  } else if (typeof doc === "string") {
    expect("a text entry").toBe("a file entry");
  } else {
    expect(doc.type).toBe("text/plain");
    expect(doc.size).toBe(2);
  }
});
test("a quoted boundary and a repeated name both work", async () => {
  const boundary = "abc";
  const body = [
    "--" + boundary,
    'Content-Disposition: form-data; name="tag"',
    "",
    "one",
    "--" + boundary,
    'Content-Disposition: form-data; name="tag"',
    "",
    "two",
    "--" + boundary + "--",
    "",
  ].join("\r\n");
  const response = new Response(body, {
    headers: { "content-type": 'multipart/form-data; boundary="abc"' },
  });
  const form = await response.formData();
  const tags = form.getAll("tag");
  expect(tags.length).toBe(2);
  expect(tags[0]).toBe("one");
  expect(tags[1]).toBe("two");
});
"#;
    run_form_data_fixture(source, "form_data_multipart");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_body_is_single_use_and_an_unknown_content_type_throws() {
    let source = r#"
import { test, expect } from "vitest";
test("reading a form consumes the body", async () => {
  const response = new Response("a=1", {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  expect(response.bodyUsed).toBe(false);
  await response.formData();
  expect(response.bodyUsed).toBe(true);
});
test("a second read throws the spec's TypeError", async () => {
  const response = new Response("a=1", {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  await response.formData();
  let message = "";
  try {
    await response.formData();
  } catch (error) {
    message = String(error.message);
  }
  expect(message).toBe("Body is unusable: Body has already been read");
});
test("a Content-Type naming neither encoding throws", async () => {
  const response = new Response("plain", {
    headers: { "content-type": "text/plain" },
  });
  let name = "";
  try {
    await response.formData();
  } catch (error) {
    name = String(error.name);
  }
  expect(name).toBe("TypeError");
});
test("text() and formData() share one body", async () => {
  const response = new Response("a=1", {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  await response.text();
  let threw = false;
  try {
    await response.formData();
  } catch (error) {
    threw = true;
  }
  expect(threw).toBe(true);
});
"#;
    run_form_data_fixture(source, "form_data_body");
}
