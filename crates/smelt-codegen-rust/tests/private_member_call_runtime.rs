//! Runtime execution tests for ES private-name METHOD calls (`recv.#m(args)`).
//!
//! A private field READ (`this.#count`) has lowered for a long time, but the
//! call dispatch matched only `StaticMemberExpression` callees, so a private
//! method call fell past every arm and reported "call expression is not lowered
//! yet". Private names live in their own namespace, which is the whole reason
//! the AST spells them with a `PrivateIdentifier`; semantically, though,
//! `recv.#m(args)` is an ordinary member call:
//!
//! ```js
//! class Counter {
//!   #count = 0;
//!   #bump(by) { this.#count += by; return this.#count; }   // private method
//!   add(by) { return this.#bump(by); }                      // private call
//! }
//! ```
//!
//! Both spellings now route through one `member_call` helper, so a private call
//! inherits the receiver, argument, optional-access and method-resolution
//! behaviour of the public path rather than a parallel implementation.
//!
//! Compiling is not enough here: the interesting failure mode for a call that
//! was previously *rejected* is a call that now lowers to the WRONG receiver or
//! drops arguments. These fixtures therefore assert observable results —
//! accumulated state, argument arity, cross-instance access, and recursion —
//! and each one comes from a shape the pinned Hono checkout actually writes
//! (`context.ts`, `router/reg-exp-router/{router,prepared-router}.ts`,
//! `router/trie-router/node.ts`).
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test private_member_call_runtime -- --ignored
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
        "generated private-member-call test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-private-member-call-runtime-{}-{seq}",
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
fn a_private_method_call_reaches_the_body_with_its_arguments() {
    // The minimal shape, and the one that used to be rejected outright: a
    // public method delegating to a private one. Accumulated state across two
    // calls is what distinguishes "the call happened with `by`" from "the call
    // happened with a default".
    let source = r"
import { test, expect } from 'vitest';

class Counter {
  #count: number = 0;

  #bump(by: number): number {
    this.#count = this.#count + by;
    return this.#count;
  }

  add(by: number): number {
    return this.#bump(by);
  }

  get value(): number {
    return this.#count;
  }
}

test('a private call returns what its body computed', () => {
  const counter = new Counter();
  expect(counter.add(3)).toBe(3);
});
test('a private call mutates the receiver it was called on', () => {
  const counter = new Counter();
  counter.add(3);
  counter.add(4);
  expect(counter.value).toBe(7);
});
test('two receivers do not share private state', () => {
  const first = new Counter();
  const second = new Counter();
  first.add(10);
  expect(second.value).toBe(0);
});
";
    run_fixture(source, "private_call_arguments");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_private_method_call_carries_every_argument_position() {
    // Hono's `#addPath(method, path, handler, indexes, map)` is a five-argument
    // private call. A member-call path that dropped or reordered arguments
    // would still compile, so the assertion is on the assembled string.
    let source = r"
import { test, expect } from 'vitest';

class Joiner {
  #sep: string = '|';
  #tail: number = 3;

  #join(a: string, b: string, c: number, d: boolean): string {
    return `${a}${this.#sep}${b}${this.#sep}${c}${this.#sep}${d}`;
  }

  run(): string {
    return this.#join('one', 'two', 3, true);
  }

  runWithPrivateArguments(): string {
    // A private FIELD read in ARGUMENT position: a second place private names
    // appear, and one the argument lowering rejected separately from the call
    // itself (`call argument kind is not lowered yet`).
    return this.#join(this.#sep, 'two', this.#tail, true);
  }
}

test('a private call preserves argument order and arity', () => {
  expect(new Joiner().run()).toBe('one|two|3|true');
});
test('a private field read may be a call argument', () => {
  expect(new Joiner().runWithPrivateArguments()).toBe('||two|3|true');
});
";
    run_fixture(source, "private_call_arity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_private_method_call_targets_any_instance_of_its_own_class() {
    // ES private names are CLASS-scoped, not instance-scoped: a method may
    // reach a private member of another instance, which is how Hono's
    // trie-router walks `nextNode.#children`. The receiver of a private call
    // is therefore an ordinary lowered expression, not an implicit `this`.
    let source = r"
import { test, expect } from 'vitest';

class Node {
  #label: string;
  #child: Node | null = null;

  constructor(label: string) {
    this.#label = label;
  }

  attach(child: Node): void {
    this.#child = child;
  }

  #describe(): string {
    return `<${this.#label}>`;
  }

  describeChild(): string {
    const child = this.#child;
    if (child === null) {
      return 'none';
    }
    return child.#describe();
  }
}

test('a private call on another instance reads that instance state', () => {
  const parent = new Node('parent');
  parent.attach(new Node('child'));
  expect(parent.describeChild()).toBe('<child>');
});
test('the same call reports the receiver it was given', () => {
  const parent = new Node('parent');
  parent.attach(new Node('other'));
  expect(parent.describeChild()).toBe('<other>');
});
";
    run_fixture(source, "private_call_cross_instance");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_private_method_may_call_itself_and_other_private_methods() {
    // Hono's `#insertPath` recurses and `#addWildcard` is called from a public
    // `add`. Recursion is the case where a wrong receiver diverges instead of
    // returning a wrong value, so the fixture bounds it by construction.
    let source = r"
import { test, expect } from 'vitest';

class Walker {
  #double(value: number): number {
    return value + value;
  }

  #countdown(from: number, acc: number): number {
    if (from <= 0) {
      return acc;
    }
    return this.#countdown(from - 1, this.#double(acc));
  }

  run(from: number): number {
    return this.#countdown(from, 1);
  }
}

test('a private method may recurse', () => {
  expect(new Walker().run(3)).toBe(8);
});
test('a private method may call a sibling private method', () => {
  expect(new Walker().run(1)).toBe(2);
});
test('a zero-depth recursion returns the accumulator untouched', () => {
  expect(new Walker().run(0)).toBe(1);
});
";
    run_fixture(source, "private_call_recursion");
}
