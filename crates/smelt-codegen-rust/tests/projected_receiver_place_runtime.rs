//! Runtime execution tests for receivers that are not already a local.
//!
//! A MIR place is rooted at a local, so an iterable or an assignment target
//! whose receiver is a *projection* (`a.b`, `m[k]`), a call result, or a value
//! still typed optional needs a temporary to root it. Two lowering sites
//! demanded a local outright instead and rejected the program:
//!
//! * `for (const x of a.b)` — "field and index reads currently require a local
//!   receiver". Hono's trie router (`for (const child of node.#patterns)`) could
//!   not be lowered at all, which is what stopped the whole crate from being
//!   emitted.
//! * `a[i] = v` where `a`'s type is still `T[] | undefined` — "only local,
//!   field, and index expressions can be assigned". `tsc` accepts the source
//!   because the preceding assignment narrows `a`, but the frontend's narrowing
//!   does not always reach the write target, so the target arrived as an
//!   `OptionalIndex`. Hono's trie router again (`partOffsets[p] = offset`).
//!
//! Both now lower the way a hand-written Rust team would: bind the receiver (or
//! its unwrapped value) to a temporary and project through it. What the fixtures
//! below check is the part a compile cannot: that the temporary still refers to
//! the same object, so a write through it is visible in the original and an
//! iteration over it sees the same elements.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test projected_receiver_place_runtime -- --ignored
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
        "generated projected-receiver test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-projected-receiver-place-runtime-{}-{seq}",
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
fn a_for_of_iterates_a_receiver_that_is_not_a_local() {
    // Hono's `for (const child of node.#patterns)` shape, in each spelling the
    // iterable can take: a public field, a private field, an index read, and a
    // call result. Each was a hard rejection; the loop body mutating an element
    // is what proves the temporary still names the same objects.
    let source = r#"
import { test, expect } from "vitest";

class Node {
  public patterns: number[] = [1, 2, 3];
  #hidden: number[] = [10, 20];
  readonly buckets: Record<string, number[]> = { a: [4, 5] };

  hiddenTotal(): number {
    let total = 0;
    for (const value of this.#hidden) {
      total += value;
    }
    return total;
  }

  all(): number[] {
    return this.patterns;
  }
}

test("a for...of over a field read sums its elements", () => {
  const node = new Node();
  let total = 0;
  for (const value of node.patterns) {
    total += value;
  }
  expect(total).toBe(6);
});

test("a for...of over a private field read sums its elements", () => {
  expect(new Node().hiddenTotal()).toBe(30);
});

test("a for...of over an index read sums its elements", () => {
  const node = new Node();
  let total = 0;
  for (const value of node.buckets.a) {
    total += value;
  }
  expect(total).toBe(9);
});

test("a for...of over a call result sums its elements", () => {
  const node = new Node();
  let total = 0;
  for (const value of node.all()) {
    total += value;
  }
  expect(total).toBe(6);
});

test("mutating through a projected receiver is visible in the original", () => {
  const node = new Node();
  for (const value of node.patterns) {
    node.buckets.a.push(value);
  }
  expect(node.buckets.a.length).toBe(5);
  expect(node.buckets.a[4]).toBe(3);
});
"#;
    run_fixture(source, "smelt_projected_receiver_for_of");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_write_through_a_narrowed_optional_receiver_lands_in_the_original() {
    // Hono's `partOffsets[p] = offset` shape: the binding's declared type is
    // still `T[] | undefined` at the write, and only the preceding assignment
    // makes it present. The write must land in the object the binding holds --
    // an unwrapped COPY that is not shared would type-check, compile, and lose
    // every element.
    let source = r#"
import { test, expect } from "vitest";

interface Row {
  count: number;
}

function offsetsFor(path: string, len: number): number[] {
  let partOffsets: number[] | undefined = undefined;
  if (!partOffsets) {
    partOffsets = [];
    let offset = path[0] === "/" ? 1 : 0;
    for (let p = 0; p < len; p++) {
      partOffsets[p] = offset;
      offset += 2;
    }
  }
  return partOffsets;
}

test("an index write through an optional-typed binding is kept", () => {
  expect(offsetsFor("/a/b", 3)).toStrictEqual([1, 3, 5]);
  expect(offsetsFor("a/b", 2)).toStrictEqual([0, 2]);
});

test("a field write through an optional-typed binding is kept", () => {
  let row: Row | undefined = undefined;
  row = { count: 0 };
  row.count = 7;
  expect(row.count).toBe(7);
});

test("an index write through a definite assertion is kept", () => {
  const rows: Record<string, number[]> = { a: [] };
  const bucket: number[] | undefined = rows.a;
  bucket![0] = 9;
  expect(rows.a[0]).toBe(9);
});
"#;
    run_fixture(source, "smelt_projected_receiver_optional_write");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_write_through_a_projected_value_receiver_lands_in_the_original() {
    // H31: the family with NO rustc error at all. A MIR place is rooted at a
    // local, so `this.children[key] = child` copies `this.children` into a
    // temporary and writes through that. For a Smelt collection HANDLE the copy
    // shares storage and the write lands; for a VALUE representation -- a plain
    // `HashMap`/`Vec` field, a value-class struct -- it is a deep copy, so the
    // write went nowhere and the program printed a different answer than Node
    // while compiling perfectly. The receiver is now committed back through its
    // own place, so both representations agree.
    //
    // Two levels of nesting are the point: `this.branches[b].leaves[k] = leaf`
    // copies `this.branches[b]` (itself rooted in a copy of `this.branches`),
    // so the inner level has to commit before the outer one or the outer store
    // writes back a stale value.
    let source = r#"
import { test, expect } from "vitest";

class Leaf {
  pattern = "";
  hits: number[] = [];
}

class Branch {
  leaves: Record<string, Leaf> = {};
  label = "";
}

class Tree {
  branches: Record<string, Branch> = {};
  rows: number[][] = [[0], [0]];

  insert(branch: string, key: string, pattern: string): void {
    this.branches[branch] = new Branch();
    this.branches[branch].label = branch;
    this.branches[branch].leaves[key] = new Leaf();
    this.branches[branch].leaves[key].pattern = pattern;
    this.branches[branch].leaves[key].hits[0] = 1;
  }

  patternOf(branch: string, key: string): string {
    const found = this.branches[branch];
    if (!found) {
      return "no-branch";
    }
    const leaf = found.leaves[key];
    return leaf ? leaf.pattern : "no-leaf";
  }

  bump(row: number, column: number, value: number): void {
    this.rows[row][column] = value;
  }
}

test("a write through a projected receiver reaches the object", () => {
  const tree = new Tree();
  tree.insert("a", "k", "p1");
  expect(tree.patternOf("a", "k")).toBe("p1");
  expect(tree.patternOf("b", "k")).toBe("no-branch");
  expect(tree.patternOf("a", "zz")).toBe("no-leaf");
});

test("each level of a nested projection keeps its own write", () => {
  const tree = new Tree();
  tree.insert("a", "k", "p1");
  expect(tree.branches["a"].label).toBe("a");
  expect(tree.branches["a"].leaves["k"].pattern).toBe("p1");
  expect(tree.branches["a"].leaves["k"].hits[0]).toBe(1);
});

test("a second insert does not lose the first", () => {
  const tree = new Tree();
  tree.insert("a", "k", "p1");
  tree.insert("b", "j", "p2");
  expect(tree.patternOf("a", "k")).toBe("p1");
  expect(tree.patternOf("b", "j")).toBe("p2");
});

test("an index write through a nested list projection is kept", () => {
  const tree = new Tree();
  tree.bump(1, 0, 42);
  expect(tree.rows[1][0]).toBe(42);
  expect(tree.rows[0][0]).toBe(0);
});

test("a write through a projected local receiver is kept", () => {
  // The same shape with no `this` in sight: the receiver is a projection of a
  // plain local, which is how every non-method spelling of H31 arrives.
  const outer: Record<string, Record<string, number>> = { a: {} };
  outer.a["k"] = 5;
  outer["a"]["j"] = 6;
  expect(outer.a["k"]).toBe(5);
  expect(outer.a["j"]).toBe(6);
});
"#;
    run_fixture(source, "smelt_projected_value_receiver_write");
}
