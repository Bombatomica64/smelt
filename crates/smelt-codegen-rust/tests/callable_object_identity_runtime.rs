//! Runtime execution tests for erased *callable object* identity.
//!
//! A JavaScript function can carry own properties (`Object.assign(fn, { .. })`).
//! Smelt erases such a value to `SmeltUnknown::Object { __smelt_call, ..props }`,
//! and when it is narrowed to a concrete `Rc<dyn Fn(..)>` the sibling properties
//! have nowhere to live on the bare `Rc` — so the emitted prelude remembers the
//! originating object in the thread-local `SMELT_CALLABLE_OBJECTS` registry,
//! keyed by the address of the callback allocation, and looks it back up when
//! that callback is erased again. `SMELT_FUNCTION_ORIGINS`,
//! `SMELT_FUNCTION_IDENTITIES` and `SMELT_FUNCTION_LENGTHS` key on an address the
//! same way.
//!
//! Keying on an address is only sound if the address cannot be recycled while a
//! registry still names it. It can be: an `Rc` allocation is freed when its last
//! strong handle drops, and the allocator hands that block to the next
//! allocation of the same size. A freshly built callback landing on a dead
//! callback's address then inherits the dead one's entries and is erased back as
//! SOMEBODY ELSE'S callable object.
//!
//! That is not theoretical. It is what made remeda's lazy `pipe` fail
//! intermittently under `cargo test`'s thread-per-test scheduling — roughly
//! three runs in twenty. `map(cb)`'s lazy evaluator was allocated on a recycled
//! address, `smelt_lookup_callable_object` answered with a PREVIOUS operation's
//! `{ __smelt_call: dataLast, lazy, lazyArgs }`, `prepareLazyFunction` kept that
//! object's `__smelt_call`, and `pipe` then invoked `dataLast(item)` — routing
//! one ITEM into the ARRAY parameter of `map`'s data-first implementation and
//! panicking with `unknown is not array`.
//!
//! The fix reserves every keyed address with a `Weak`: the block stays allocated
//! (the value is still dropped, so captured state is released) and can never be
//! handed to a later callback.
//!
//! Reproducing the failure through the source language is not practical — it
//! needs an exact allocator parity that TypeScript gives no handle on. So the
//! test below drives the emitted registries directly: it emits a real crate from
//! a remeda-shaped fixture, appends a probe module that frees a registered
//! callback and then allocates fresh ones of the same type until one lands on
//! the freed address, and asserts that such a callback carries no inherited
//! entry. Without the address reservation the very first candidate reuses the
//! address and the lookup answers with the dead callback's object.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test callable_object_identity_runtime -- --ignored
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

/// Appends a hand-written probe module to the emitted crate's `main.rs`.
///
/// The probe needs to control every allocation between a free and the reuse of
/// its address, which no TypeScript spelling can express, so it is written in
/// Rust against the prelude the emitter just produced.
fn append_probe(crate_dir: &Path, probe: &str) {
    let main = crate_dir.join("src").join("main.rs");
    let mut source = std::fs::read_to_string(&main).expect("read generated main.rs");
    source.push('\n');
    source.push_str(probe);
    std::fs::write(&main, source).expect("write generated main.rs");
}

/// Runs `cargo test` on the emitted crate; a passing run means every generated
/// `expect(...)` assertion and every appended probe assertion held at runtime.
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
        "generated callable-object identity test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-callable-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate, append `probe`, and run the crate's tests.
fn run_fixture(source: &str, crate_name: &str, probe: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    append_probe(&crate_dir, probe);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
}

/// A remeda-shaped program: a callable object handed to an erased function whose
/// typed result is a callback of the SAME `Rc` type as the coerced argument.
///
/// This is the exact emission pair the bug lives in — the argument coercion runs
/// `smelt_register_callable_object`, the result erasure runs
/// `smelt_lookup_callable_object` — so the emitted crate is guaranteed to
/// contain both registries for the probe to drive.
const FIXTURE: &str = r#"
import { test, expect } from "vitest";

type Step = (item: number) => number;

// Mirrors remeda's `lazy` implementation: takes one callback and returns a
// brand-new callback of the same type.
function wrap(inner: Step): Step {
  return (item: number): number => inner(item) + 1;
}

// A callable object: a function carrying its own properties, like the value
// `purry` hands to `pipe`.
function makeCallable(offset: number): unknown {
  const base: Step = (item: number): number => item + offset;
  return Object.assign(base, { tag: offset });
}

function eraseWrap(): unknown {
  return wrap;
}

function callErased(erased: unknown, argument: unknown): unknown {
  const callable = erased as (value: unknown) => unknown;
  return callable(argument);
}

test("wrapping a callable object through the erased boundary keeps its behaviour", () => {
  const erasedWrap = eraseWrap();
  let total = 0;
  for (let index = 1; index <= 256; index += 1) {
    const callable = makeCallable(index);
    const wrapped = callErased(erasedWrap, callable) as Step;
    total += wrapped(0);
  }
  // Each `wrapped(0)` is `makeCallable(index)(0) + 1`, i.e. `index + 1`, so the
  // total is sum(1..256) + 256.
  expect(total).toBe(33152);
});
"#;

/// Rust appended to the emitted crate, driving the identity registries directly.
const PROBE: &str = r#"
#[cfg(test)]
mod smelt_identity_registry_probe {
    //! Deterministic probes for address reuse in the emitted identity registries.
    //!
    //! Each probe registers an entry for a callable, drops it, then allocates
    //! fresh callables of the SAME type until one lands on the freed address.
    //! Reaching that address at all means the registry key was recycled; the
    //! assertion then checks the recycled key carries no inherited entry. With
    //! the addresses reserved no candidate ever reaches the address and the probe
    //! is satisfied trivially — which is the point: the reservation is what makes
    //! the aliasing unreachable.

    use super::*;

    /// How many fresh allocations to try before giving up on hitting the address.
    const CANDIDATES: usize = 64;

    #[test]
    fn a_recycled_callback_address_does_not_inherit_a_stale_callable_object() {
        let object = SmeltUnknown::Object(SmeltObject::new(vec![(
            "tag".to_owned(),
            SmeltUnknown::Number(1.0),
        )]));
        let dead: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(|value| value);
        let dead_key = smelt_callable_object_key(&dead);
        smelt_register_callable_object(&dead, object);
        drop(dead);
        for bump in 0..CANDIDATES {
            let candidate: ::std::rc::Rc<dyn Fn(f64) -> f64> =
                ::std::rc::Rc::new(move |value| value + bump as f64);
            if smelt_callable_object_key(&candidate) == dead_key {
                assert!(
                    smelt_lookup_callable_object(&candidate).is_none(),
                    "a fresh callback reused a registered address and inherited a dead callback's callable object"
                );
            }
        }
    }

    #[test]
    fn a_recycled_erased_function_address_does_not_inherit_a_stale_origin() {
        type Erased = ::std::rc::Rc<
            dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>,
        >;
        type Typed = ::std::rc::Rc<dyn Fn(f64) -> f64>;

        let origin: Typed = ::std::rc::Rc::new(|value| value + 100.0);
        let dead: Erased = ::std::rc::Rc::new(|_| Ok(SmeltUnknown::Null));
        let dead_key = smelt_erased_function_key(&dead);
        smelt_register_function_origin(&dead, origin);
        drop(dead);
        for bump in 0..CANDIDATES {
            let candidate: Erased =
                ::std::rc::Rc::new(move |_| Ok(SmeltUnknown::Number(bump as f64)));
            if smelt_erased_function_key(&candidate) == dead_key {
                assert!(
                    smelt_restore_function_origin::<Typed>(&candidate).is_none(),
                    "a fresh erased function reused a registered address and inherited a dead function's typed origin"
                );
            }
        }
    }
}
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_recycled_callback_address_does_not_inherit_a_stale_registry_entry() {
    run_fixture(FIXTURE, "callable_object_recycled_address", PROBE);
}
