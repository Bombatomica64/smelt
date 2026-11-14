//! Runtime execution tests for methods surviving class-instance erasure.
//!
//! A method read off an erased class instance has to find the method, which is
//! what es-toolkit's `memoize` with a custom cache does. A class's methods ride
//! along as prototype-carried members keyed
//! `__smelt_method:<name>`, which `smelt_get_object_field` and the record
//! projection resolve after the own property misses. The prefix is deliberately
//! distinct from `__smelt_proto:` (used by `Object.create`): a class's methods
//! are NON-enumerable in JavaScript, so they must stay invisible to `for...in`,
//! `Object.keys`, structural equality, hashing and JSON — while `Object.create`
//! inherits *enumerable* properties that `for...in` must still walk.
//!
//! Only running the program proves the receiver is actually bound, that writes
//! through the erased view reach the same instance, and that the hidden members
//! stay hidden. The tier is `#[ignore]`d because it compiles and executes real
//! crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_class_method_runtime -- --ignored
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
        "smelt-erased-class-method-runtime-{}-{seq}",
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
fn erased_class_instances_keep_their_methods() {
    // The shape es-toolkit's `memoize` uses: a custom cache class handed to a
    // parameter that is only known dynamically, then driven entirely through
    // erased member reads. Before prototype members existed, `bag.has(key)`
    // read `undefined`, the call site substituted `|_| false`, and nothing was
    // ever stored — with no diagnostic.
    let source = r#"
import { test, expect } from "vitest";
interface Cache {
  has(key: string): boolean;
  get(key: string): string | undefined;
  set(key: string, value: string): void;
}
class CustomCache implements Cache {
  private data: Map<string, string> = new Map();
  get(key: string): string | undefined {
    return this.data.get(key);
  }
  set(key: string, value: string): void {
    this.data.set(key, value);
  }
  has(key: string): boolean {
    return this.data.has(key);
  }
}
function useCache(cache: unknown, key: string, value: string): string | undefined {
  const bag = cache as Cache;
  if (bag.has(key)) {
    return bag.get(key);
  }
  bag.set(key, value);
  return bag.get(key);
}
test("an erased instance answers its own methods", () => {
  const cache = new CustomCache();
  expect(useCache(cache, "a", "b")).toBe("b");
});
test("a write through the erased view reaches the same instance", () => {
  const cache = new CustomCache();
  useCache(cache, "a", "b");
  expect(cache.has("a")).toBe(true);
  expect(cache.get("a")).toBe("b");
});
test("a read through the erased view sees an earlier typed write", () => {
  const cache = new CustomCache();
  cache.set("a", "b");
  expect(useCache(cache, "a", "zzz")).toBe("b");
});
"#;
    run_fixture(source, "smelt_erased_class_methods");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn prototype_members_stay_out_of_own_property_views() {
    // A class's methods are non-enumerable, so none of JavaScript's own-property
    // views may see them, and two structurally equal instances must still
    // compare equal even though each carries its own bound closures. remeda's
    // `isEmptyish` probes emptiness with a bare `for (const _ in data)`, which
    // is what caught this when the members were first keyed `__smelt_proto:`.
    let source = r#"
import { test, expect } from "vitest";
class Point {
  constructor(public x: number = 1) {}
  shift(): number {
    return this.x + 1;
  }
}
test("Object.keys does not list methods", () => {
  const erased: any = new Point(1);
  expect(Object.keys(erased)).toEqual(["x"]);
});
test("for...in does not walk methods", () => {
  const erased: any = new Point(1);
  const seen: string[] = [];
  for (const key in erased) {
    seen.push(key);
  }
  expect(seen).toEqual(["x"]);
});
test("methods do not break structural equality", () => {
  const first: any = new Point(1);
  const second: any = new Point(1);
  expect(first).toEqual(second);
});
test("the method is still reachable through the erased value", () => {
  const erased: any = new Point(1);
  expect(erased.shift()).toBe(2);
});
"#;
    run_fixture(source, "smelt_prototype_member_visibility");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_create_still_inherits_enumerable_properties() {
    // The counterpart the split prefix protects: properties inherited through
    // `Object.create(proto)` ARE enumerable in JavaScript, so `for...in` must
    // still walk them even though a class's methods must not.
    let source = r#"
import { test, expect } from "vitest";
test("for...in walks inherited enumerable properties", () => {
  const proto: any = { a: 1 };
  const child: any = Object.create(proto);
  child.b = 2;
  const seen: string[] = [];
  for (const key in child) {
    seen.push(key);
  }
  expect(seen.sort()).toEqual(["a", "b"]);
});
test("Object.keys lists only own properties", () => {
  const proto: any = { a: 1 };
  const child: any = Object.create(proto);
  child.b = 2;
  expect(Object.keys(child)).toEqual(["b"]);
});
"#;
    run_fixture(source, "smelt_object_create_enumerability");
}
