//! Runtime execution tests for the property-store key fingerprint.
//!
//! `SmeltFieldStore` scans its entries to resolve a property, and every entry
//! carries a `smelt_field_fingerprint` — length, first byte, last byte — that is
//! compared before full key equality so a SAME-LENGTH key is rejected without a
//! `memcmp` into its bytes. That filter is only sound if the fingerprint a
//! lookup computes for its probe key is bit-identical to the one stored when the
//! key was inserted. Generated code always stores `String` and probes `&str`
//! (those are the only two `SmeltPropertyKey` impls, and a record's `get`
//! borrows through `Borrow<str>`), so the two impls disagreeing would make a
//! PRESENT key report absent — silently, and only for some keys.
//!
//! These cases therefore insert through the stored key type and read back
//! through the borrowed one for every key shape the fingerprint has to handle:
//! same-length keys, keys that COLLIDE in the fingerprint (same length, same
//! first byte, same last byte, different middle), the empty key, multi-byte
//! UTF-8 keys, array-index keys (which insert at a computed position rather
//! than appending), a store past `SMELT_FIELD_SCAN_LIMIT` that resolves through
//! the hash index instead of the scan, and delete/re-add, which reindexes.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test field_key_fingerprint_runtime -- --ignored
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
        "generated fingerprint test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-field-key-fingerprint-runtime-{}-{seq}",
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
fn same_length_and_fingerprint_colliding_keys_still_resolve() {
    // The case the fingerprint exists for, and the case that would break it.
    // `group`/`value`/`count` are the same length, so the scan used to compare
    // all of their bytes; `alphaqx`/`alpha1x` additionally share their first and
    // last byte, so they share a FINGERPRINT and must fall through to full key
    // equality rather than resolving to whichever entry comes first.
    let source = r#"
import { test, expect } from "vitest";
function read(value: unknown, key: string): unknown {
  return (value as Record<string, unknown>)[key];
}
test("same-length keys resolve to their own values", () => {
  const record = { group: "g", value: "v", count: "c", flag: true, id: 7 };
  expect(record.group).toBe("g");
  expect(record.value).toBe("v");
  expect(record.count).toBe("c");
  expect(read(record, "group")).toBe("g");
  expect(read(record, "value")).toBe("v");
  expect(read(record, "count")).toBe("c");
  expect(read(record, "id")).toBe(7);
  expect(read(record, "grouq")).toBe(undefined);
  expect(read(record, "valuf")).toBe(undefined);
});
test("keys sharing length, first byte and last byte stay distinct", () => {
  const record: Record<string, number> = { alphaqx: 1, alpha1x: 2, alphazx: 3 };
  expect(record.alphaqx).toBe(1);
  expect(record.alpha1x).toBe(2);
  expect(record.alphazx).toBe(3);
  expect(read(record, "alphaqx")).toBe(1);
  expect(read(record, "alpha1x")).toBe(2);
  expect(read(record, "alphazx")).toBe(3);
  expect(read(record, "alphawx")).toBe(undefined);
});
"#;
    run_fixture(source, "smelt_field_fingerprint_same_length");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn empty_multibyte_and_index_keys_resolve() {
    // The fingerprint reads `bytes.first()`/`bytes.last()`, so the empty key has
    // to be handled without indexing; a multi-byte UTF-8 key has to fingerprint
    // over its BYTES on both sides, not its characters; and an array-index key
    // inserts at a computed position rather than appending, which is the other
    // path that has to attach a fingerprint to a new entry.
    let source = r#"
import { test, expect } from "vitest";
function read(value: unknown, key: string): unknown {
  return (value as Record<string, unknown>)[key];
}
test("the empty key is a key like any other", () => {
  const record: Record<string, number> = { "": 1, a: 2 };
  expect(read(record, "")).toBe(1);
  expect(read(record, "a")).toBe(2);
  expect(Object.keys(record)).toEqual(["", "a"]);
});
test("multi-byte keys resolve through the erased boundary", () => {
  const record: Record<string, string> = { "cafés": "x", "cafés!": "y", "naïve": "z" };
  expect(read(record, "cafés")).toBe("x");
  expect(read(record, "cafés!")).toBe("y");
  expect(read(record, "naïve")).toBe("z");
  expect(read(record, "cafés?")).toBe(undefined);
});
test("array-index keys inserted out of order still resolve", () => {
  const record: Record<string, number> = {};
  record["30"] = 30;
  record["10"] = 10;
  record["20"] = 20;
  record["tail"] = 99;
  expect(Object.keys(record)).toEqual(["10", "20", "30", "tail"]);
  expect(read(record, "10")).toBe(10);
  expect(read(record, "20")).toBe(20);
  expect(read(record, "30")).toBe(30);
  expect(read(record, "tail")).toBe(99);
});
"#;
    run_fixture(source, "smelt_field_fingerprint_key_shapes");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn dictionary_sized_stores_and_reindexing_keep_every_key_reachable() {
    // Past `SMELT_FIELD_SCAN_LIMIT` the store builds a hash index and resolves a
    // key through it, confirming the hit with the fingerprint AND full equality
    // before falling back to the scan. Deleting a key shifts later positions and
    // rebuilds that index, so this walks a 20-key store of same-length names,
    // deletes from the middle, re-adds, and reads every key back.
    let source = r#"
import { test, expect } from "vitest";
function read(value: unknown, key: string): unknown {
  return (value as Record<string, unknown>)[key];
}
test("a dictionary-sized store resolves every same-length key", () => {
  const bag: Record<string, number> = {};
  const names: string[] = [];
  for (let i = 0; i < 20; i++) {
    const name = "k" + String(100 + i);
    names.push(name);
    bag[name] = i;
  }
  for (let i = 0; i < 20; i++) {
    expect(bag[names[i]]).toBe(i);
    expect(read(bag, names[i])).toBe(i);
  }
  expect(read(bag, "k999")).toBe(undefined);
  delete bag[names[5]];
  delete bag[names[12]];
  expect(read(bag, names[5])).toBe(undefined);
  expect(read(bag, names[12])).toBe(undefined);
  for (let i = 0; i < 20; i++) {
    if (i !== 5 && i !== 12) {
      expect(read(bag, names[i])).toBe(i);
    }
  }
  bag[names[5]] = 500;
  expect(read(bag, names[5])).toBe(500);
  expect(Object.keys(bag).length).toBe(19);
});
"#;
    run_fixture(source, "smelt_field_fingerprint_dictionary");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn borrowed_str_probes_agree_with_stored_string_keys() {
    // The `K`/`Q` boundary itself. User-level property reads lower to
    // `get(&String)`, so they compare a `String` fingerprint against a `String`
    // fingerprint and would survive the two impls disagreeing. The RUNTIME
    // reads an object's own fields through `&'static str` literals —
    // `contains_key("__smelt_error")`, `get("name")`, `get("message")` behind
    // `String(err)` — which is `K = String`, `Q = str`. If `str` and `String`
    // computed different fingerprints, those probes would report the marker
    // absent and stringification would silently fall through to
    // `"[object Object]"`. Verified to fail that way when the `str` impl is
    // perturbed, so this case is load-bearing rather than decorative.
    let source = r#"
import { test, expect } from "vitest";
test("an error stringifies through str-probed own keys", () => {
  const err = new Error("boom");
  expect(String(err)).toBe("Error: boom");
});
test("a regexp stringifies through str-probed own keys", () => {
  const pattern = /ab+c/gi;
  expect(String(pattern)).toBe("/ab+c/gi");
});
"#;
    run_fixture(source, "smelt_field_fingerprint_str_probe");
}
