//! Runtime execution tests for erased *callable object* identity.
//!
//! A JavaScript function can carry own properties (`Object.assign(fn, { .. })`).
//! Smelt erases such a value to `SmeltUnknown::Object { __smelt_call, ..props }`,
//! and when it is narrowed to a concrete `Rc<dyn Fn(..)>` the sibling properties
//! have nowhere to live on the bare `Rc` — so the emitted prelude remembers the
//! originating object in the thread-local callable table (`SMELT_CALLABLES`),
//! keyed by the address of the callback allocation, and looks it back up when
//! that callback is erased again. The typed origin of an erased adapter and the
//! function's identity token live in the same entry.
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
//! handed to a later callback. The `Weak` lives IN the entry it reserves
//! (`callable_registry`'s `SmeltAddressTable`), and an amortized sweep removes
//! entries whose allocation died — guard and metadata together — so the table
//! stays bounded and a recycled address finds no entry at all. The probes
//! below force that sweep and check both halves: a swept address carries no
//! inherited entry, and the table (and what its entries retained) does not
//! grow with the number of erasures.
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
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        append_probe(&crate_dir, probe);
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

    /// Allocations needed to push the callable table past any sweep threshold
    /// it can have reached in this test thread.
    const SWEEP_PRESSURE: usize = 4 * 4096;

    /// Register `count` short-lived callables, forcing at least one sweep.
    fn churn_callable_table(count: usize) {
        for index in 0..count {
            let transient: ::std::rc::Rc<dyn Fn(f64) -> f64> =
                ::std::rc::Rc::new(move |value| value + index as f64);
            let _ = smelt_canonical_function_identity(&transient);
        }
    }

    #[test]
    fn a_swept_address_can_be_reused_without_reviving_the_dead_entry() {
        let object = SmeltUnknown::Object(SmeltObject::new(vec![(
            "tag".to_owned(),
            SmeltUnknown::Number(1.0),
        )]));
        let live: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(|value| value * 2.0);
        smelt_register_callable_object(&live, object.clone());
        let dead: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(|value| value);
        let dead_key = smelt_callable_object_key(&dead);
        smelt_register_callable_object(&dead, object);
        drop(dead);
        // While the entry exists its guard keeps the address reserved.
        assert!(SMELT_CALLABLES.with(|table| table.borrow().get(dead_key).is_some()));
        churn_callable_table(SWEEP_PRESSURE);
        // The sweep removed the dead entry, guard included, so the address is
        // free again (a churned callable may now own it, with an entry of its
        // own that carries no callable object) — and a live entry survived it.
        assert!(
            SMELT_CALLABLES.with(|table| table.borrow().get(dead_key).is_none_or(|meta| meta.callable_object.is_none())),
            "a sweep left a dead callable's entry in the table"
        );
        assert!(smelt_lookup_callable_object(&live).is_some(), "a sweep removed a live callable's entry");
        for bump in 0..CANDIDATES {
            let candidate: ::std::rc::Rc<dyn Fn(f64) -> f64> =
                ::std::rc::Rc::new(move |value| value + bump as f64);
            if smelt_callable_object_key(&candidate) == dead_key {
                assert!(
                    smelt_lookup_callable_object(&candidate).is_none(),
                    "a callback allocated on a swept address revived the dead callback's callable object"
                );
                assert_eq!(
                    smelt_function_identity_of(dead_key),
                    dead_key,
                    "a callback allocated on a swept address inherited the dead callback's identity"
                );
            }
        }
    }

    #[test]
    fn the_callable_table_stays_bounded_and_releases_what_it_retained() {
        type Erased = ::std::rc::Rc<
            dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>,
        >;
        type Typed = ::std::rc::Rc<dyn Fn(f64) -> f64>;
        // Shared by every captured closure below: its strong count is the number
        // of closures the table (or anything else) still keeps alive.
        let witness = ::std::rc::Rc::new(());
        let mut peak = 0;
        for index in 0..100_000_usize {
            let captured = ::std::rc::Rc::clone(&witness);
            // One full erase/narrow round trip, as an erasure adapter runs it:
            // typed origin, erased adapter linked to its identity, the callable
            // object recovered on the way back, and a `length`.
            let typed: Typed = ::std::rc::Rc::new(move |value| { let _ = &captured; value + index as f64 });
            let identity = smelt_canonical_function_identity(&typed);
            let origin = typed.clone();
            let erased: Erased = ::std::rc::Rc::new(move |_| Ok(SmeltUnknown::Number(origin(1.0))));
            smelt_register_function_origin(&erased, typed.clone());
            smelt_link_function_identity_key(&erased, identity);
            smelt_register_function_length_once(&erased, 1.0);
            let restored = smelt_restore_function_origin::<Typed>(&erased).expect("origin");
            let object = SmeltUnknown::Object(SmeltObject::new(vec![("__smelt_call".to_owned(), SmeltUnknown::Function(erased.clone()))]));
            let forwarder: Typed = ::std::rc::Rc::new(move |value| restored(value));
            smelt_register_callable_object(&forwarder, object);
            smelt_link_function_identity(&forwarder, &erased);
            assert!(smelt_same_function_identity(&forwarder, &typed), "a linked forwarder lost its function identity");
            peak = peak.max(SMELT_CALLABLES.with(|table| table.borrow().len()));
        }
        // A dead entry lives at most until the next sweep, and a sweep runs
        // whenever the table doubles past its live size: with nothing alive
        // between iterations the table never holds more than a couple of
        // sweep thresholds, whatever the number of erasures.
        assert!(peak <= 4 * SMELT_ADDRESS_TABLE_MIN_SWEEP, "callable table grew to {peak} entries over 100000 erasures");
        churn_callable_table(SWEEP_PRESSURE);
        // Everything those erasures captured was released: the entries do not
        // keep their own keys (or the typed origins they recorded) alive.
        assert_eq!(
            ::std::rc::Rc::strong_count(&witness) - 1,
            0,
            "captured closures are still alive after a sweep"
        );
    }

    #[test]
    fn reference_record_identities_are_stable_per_cell_and_bounded() {
        let kept = ::std::rc::Rc::new(::std::cell::RefCell::new(0_u64));
        let kept_id = smelt_reference_object_identity(&kept);
        let mut peak = 0;
        for index in 0..100_000_u64 {
            let cell = ::std::rc::Rc::new(::std::cell::RefCell::new(index));
            let first = smelt_reference_object_identity(&cell);
            // Every erasure of one cell is the same JavaScript object.
            assert_eq!(first, smelt_reference_object_identity(&::std::rc::Rc::clone(&cell)));
            assert_ne!(first, kept_id, "a fresh cell was handed a live cell's object id");
            peak = peak.max(SMELT_REFERENCE_OBJECT_IDENTITIES.with(|table| table.borrow().len()));
        }
        assert_eq!(kept_id, smelt_reference_object_identity(&kept), "a sweep changed a live cell's object id");
        assert!(peak <= 4 * SMELT_ADDRESS_TABLE_MIN_SWEEP, "reference identity table grew to {peak} entries");
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

/// A class whose field arrows capture the instance, and a listener that
/// captures the emitter it is registered on: the two `Rc` cycles
/// `emitter::weak_receiver` breaks.
const RECEIVER_CYCLE_FIXTURE: &str = r#"
import { test, expect } from "vitest";
import { EventEmitter } from "node:events";

class Ctx {
  status = 200;
  json = (body: string): string => `${this.status}:${body}`;
  constructor(code: number) {
    this.status = code;
  }
}

function bump(c: Ctx): void {
  c.status = c.status + 1;
}

function describe(value: unknown): string {
  return typeof value;
}

// Passing the instance where `unknown` is expected erases it to an object
// view, and each field arrow to an erased adapter over the typed closure.
function show(c: Ctx): string {
  return describe(c);
}

function attach(): EventEmitter {
  const e = new EventEmitter();
  e.on("ping", () => {
    e.emit("pong");
  });
  return e;
}

test("field arrows and self-listeners keep their behaviour", () => {
  const c = new Ctx(1);
  bump(c);
  expect(c.json("a")).toBe("2:a");
  expect(show(c)).toBe("object");
  let pongs = 0;
  const e = attach();
  e.on("pong", () => {
    pongs += 1;
  });
  e.emit("ping");
  expect(pongs).toBe(1);
});
"#;

/// Rust appended to the emitted crate: each cycle's object is freed once its
/// last outside handle goes, and kept while an escaped closure needs it.
const RECEIVER_CYCLE_PROBE: &str = r#"
#[cfg(test)]
mod smelt_receiver_cycle_probe {
    use super::*;

    #[test]
    fn an_instance_whose_field_arrows_capture_it_is_freed() {
        for code in 0..1000 {
            let instance = Ctx::new(f64::from(code));
            let cell = ::std::rc::Rc::downgrade(&instance.0);
            let json = instance.0.borrow().json.clone();
            assert_eq!(json("x".to_owned()), format!("{code}:x"));
            drop(json);
            drop(instance);
            assert!(cell.upgrade().is_none(), "a Ctx with self-capturing field arrows leaked");
        }
    }

    /// The erased view's adapters die with the call, but the callable table
    /// keeps an entry for each until its next sweep. That entry must not count
    /// as a holder of the typed closure: a strong origin there made the field
    /// look shared when the last handle went, and the drop check pinned the
    /// instance for good (one Hono 4 `Context` per request).
    #[test]
    fn an_instance_erased_through_an_object_view_is_freed() {
        for code in 0..1000 {
            let instance = Ctx::new(f64::from(code));
            let cell = ::std::rc::Rc::downgrade(&instance.0);
            assert_eq!(show(instance.clone()), "object");
            drop(instance);
            assert!(cell.upgrade().is_none(), "a Ctx erased through an object view was pinned");
        }
    }

    #[test]
    fn an_escaped_field_arrow_keeps_its_instance_alive() {
        let instance = Ctx::new(7.0);
        let cell = ::std::rc::Rc::downgrade(&instance.0);
        let json = instance.0.borrow().json.clone();
        drop(instance);
        assert!(cell.upgrade().is_some(), "the instance died under an escaped field arrow");
        assert_eq!(json("y".to_owned()), "7:y");
    }

    #[test]
    fn an_emitter_whose_listener_captures_it_is_freed() {
        for _ in 0..1000 {
            let emitter = attach();
            let listeners = ::std::rc::Rc::downgrade(&emitter.listeners);
            drop(emitter);
            assert!(listeners.upgrade().is_none(), "an emitter with a self-capturing listener leaked");
        }
    }
}
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn closures_stored_on_their_receiver_do_not_keep_it_alive() {
    run_fixture(RECEIVER_CYCLE_FIXTURE, "receiver_cycle_runtime", RECEIVER_CYCLE_PROBE);
}
