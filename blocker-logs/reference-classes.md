# Reference-class modeling — `Rc<RefCell<Inner>>` for shared-mutable JS classes

Implementation of `specs/reference-class-modeling.md`. JavaScript objects are
reference cells with shared mutable identity; Smelt emitted every class as a
by-value Rust struct, which is a silent miscompile for any class that is mutated
after construction, aliased-and-observed, or lets `this` escape into a stored
closure. This feature classifies such classes and emits them as a thin handle
newtype over `Rc<RefCell<Inner>>` whose `Clone` shares identity.

## Per-phase status

**Phase 1 — classification + reference-class emitter: DONE.**
- New `crates/smelt-codegen-rust/src/classify.rs` computes the reference-class
  set once per crate (stored on `EmitContext`, read via `is_reference_class`).
- `emit_reference_class_storage` (lib.rs) emits `struct Name(Rc<RefCell<
  NameInner>>)` + `struct NameInner { fields }`, a hand-written identity `Clone`
  (`Rc::clone(&self.0)`), and `Default`/`Debug`/`IntoSmeltUnknown` delegating
  through the cell. Callback-field inners reuse the manual `Default`/`Debug`
  storage helpers; generic inners derive `Debug`/`Default` and carry a
  `PhantomData`.

**Phase 2 — method bodies: DONE.**
- All reference-class methods take `&self` uniformly (`emit_method` +
  `method_owner_is_reference_class`).
- Field reads emit `self.0.borrow().<f>.clone()`; writes emit
  `self.0.borrow_mut().<f> = …` (place.rs). `operand_text` no longer double-clones
  a reference-class field read.
- Getters lower to methods, not stored fields (frontend fix, see finding #3).

**Phase 3 — escaping `this` + async: DONE (sync path) / PARTIAL (async).**
- A method whose closure captures `self` binds the receiver once as a cloned
  handle (`let smelt_capture_self = Rc::new(RefCell::new(self.clone()));`) and the
  escaping closure captures it by `Rc::clone`. This clears the E0425
  `smelt_capture_self` cluster (5 → 1 in es-toolkit).
- Async: generated async methods stay on the local executor path (no
  `tokio::spawn`, no silent `Arc<Mutex<_>>` switch), matching the spec. The full
  async promise-cell interaction (`specs/cluster-b-promise-problem.md`) is
  unchanged and remains its own blocker.

**Phase 4 — composition + aliasing: DONE.**
- Reference-class fields share identity through the inner `Rc`; a value class
  (e.g. `Mutex`) holding a reference-class field (`Semaphore`) derives `Clone`
  and shares the handle. Binding aliasing clones the handle (`Rc::clone`).
- This fixes the silent `Mutex::acquire` miscompile: `parameter_type_has_shared_
  mutation_semantics` now returns `false` for reference classes, so delegating to
  `self.semaphore.acquire()` no longer demands a `&mut` the `&self` caller cannot
  supply and no longer decrements a throwaway clone.

## Classification stats

- Rule (V1): reference iff a non-constructor method writes `this.<field>`, OR any
  path writes an instance field on a class binding after construction, OR `this`
  is captured by a closure. Pure aliasing without mutation stays value.
- es-toolkit: `Semaphore` lifted to a reference class; `Mutex` and `CustomCache`
  stay value classes (Mutex only delegates; CustomCache mutates its `SmeltJsMap`
  through interior-mutable method calls, not `this.<field> =` writes).

## Error counts (identical methodology, es-toolkit `e008a281`)

| metric | baseline (main) | after |
| --- | --- | --- |
| total errors | 273 | 268 |
| E0425 (`smelt_capture_self`) | 5 | 1 |
| E0596 (self-borrow) | 6 | 4 |
| E0599 | 15 | 16 |
| E0308 (unrelated) | 170 | 170 |

Net −5 compile errors, plus the *uncounted* correctness win: the generated
`Mutex::acquire` now calls `self.semaphore.acquire()` on the real shared handle
instead of mutating a throwaway clone (verified by reading `dist-smelt/src/
main.rs`). Note the reported "292" in the task brief was a different measurement;
273 is the identical-methodology baseline for this es-toolkit checkout. E0599 rose
by one (a method-resolution residual on a lifted class) — a small follow-up, not a
new cluster.

## Borrow-discipline notes

- Field reads clone the value out of the borrow so the guard is a statement-local
  temporary; writes take `borrow_mut()` only after MIR has reduced the RHS to an
  operand. Because MIR is three-address form, a borrow guard never spans a
  re-entrant call within one emitted statement.
- The one residual hazard is source of the shape `this.x = this.x` (a direct
  self-copy with no intervening temp), which would emit a single statement that
  both borrows and borrow_muts the same cell. This does not occur in the
  fixtures/port (every real mutation goes through a binary op or call and is
  temped); it is noted as a codegen edge to harden later.

## Fixtures (in scratchpad, not committed)

- **Semaphore/Mutex mirror**: 7/7 scenario checks green — fresh/locked/unlocked
  via Mutex→Semaphore delegation (composition mutates the real semaphore), alias
  identity both ways, and a waiter queue whose escaping resolver captures `this`
  and mutates the real semaphore when `release()` runs it.
- **Counter cache** (field-mutating): lifted to reference; alias-through-clone
  increments the shared cell (runtime `2`/`3`).
- **Point value class**: generated `main.rs` and `source_main.rs` are
  byte-identical to the baseline (main) binary — value-class emission unchanged.

## Named blockers / deviations (no `SmeltUnknown`-to-compile)

- **Index-signature classes** (`[key: string]: T`, synthesized
  `__smelt_index_store`) are kept as value classes in V1. Their keyed store is
  accessed by concrete struct field, which is not yet taught to project through
  the shared cell; lifting them would emit `bag.__smelt_index_store` against a
  handle. Aliasing such a class therefore does not share its store yet — a
  documented narrowing, and not a regression from prior behavior.
- **V1 dynamic-property side-store** (`smelt_extra_props`): NOT emitted. The
  fixtures and es-toolkit reference classes use only declared fields; the lazy
  `Option<SmeltObject>` side-store for undeclared/computed property access on
  reference-class inners is deferred rather than emitted unused. Undeclared
  dynamic property access on a reference class is not yet routed.
- **Callback-as-method-parameter forwarding** (`Mutex.acquire(register)` →
  `semaphore.acquire(register)`): a pre-existing callback-ABI mismatch
  (`&dyn Fn` vs `Rc<closure>`), independent of reference-class modeling. The
  Semaphore/Mutex fixture exercises the escaping resolver through the stored
  `deferredTasks` queue instead, which is the reference-class-relevant path.
- **Mutable-base inheritance through `super`** and **`Weak` cycles**: spec
  non-goals, unchanged.
- **Self-copy borrow edge** (`this.x = this.x`): see borrow-discipline notes.

## Validation

- `cargo check --workspace`: clean.
- `cargo clippy` (pedantic, lib) on `smelt-codegen-rust` + `smelt-frontend-ts`:
  clean (no new warnings from touched code).
- `cargo test --workspace --exclude smelt-gui`: green (11 new reference-class
  tests; updated goldens for the now-lifted generic/materialized/`22_mutating_
  method` cases).
- remeda (pinned `3c80f28`): generated crate compiles with 0 errors and
  `cargo test` passes **1789/1789** — no regression.
