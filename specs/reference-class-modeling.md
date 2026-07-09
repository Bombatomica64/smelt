# Reference-class modeling — `Rc<RefCell<Inner>>` for shared-mutable JS classes

Design plan (investigation + compile-verified experiment, 2026-07-09). Derived
from hand-porting es-toolkit's `Semaphore`/`Mutex` to idiomatic Rust and diffing
against what Smelt emits today. The compile-checked reference port lives beside
this file: [`reference-class-modeling-port.rs`](./reference-class-modeling-port.rs).

Sibling spec: [`cluster-b-promise-problem.md`](./cluster-b-promise-problem.md) —
the `SmeltPromise { id, Rc<RefCell<…>> }` shape is the SAME shared-cell pattern
applied to promises; the async-method interaction below depends on it.

## The problem (one sentence)

Smelt emits JS classes as plain by-value Rust structs with `&self`/`&mut self`
methods, but JS objects are **reference cells with shared mutable identity**, so
any class that is mutated-after-construction, aliased, or lets `this` escape into
a stored/returned closure is emitted either as a **silent miscompile** (state
mutated on a throwaway clone) or as non-compiling code (a self that cannot be
shared into an escaping closure).

## Evidence — `Semaphore` / `Mutex` (es-toolkit `src/promise/`)

Source: a counting `Semaphore` (mutable `available`, a FIFO `deferredTasks`
queue of resolver callbacks) and a `Mutex` that composes a `Semaphore` by field
and delegates. What the current whole-crate transpile emits (`dist-smelt`, read
from the generated `main.rs`):

1. **Silent miscompile (not a compile error).** `Mutex::acquire` emits
   `let _t = self.semaphore.clone(); self.semaphore.acquire()` — it decrements
   the permit on a **throwaway clone**. It passes `cargo check` and is wrong at
   runtime: the mutex never locks. Today's value-struct model makes `.clone()`
   *fork* state; JS clone/alias *shares* it. This class of bug does not appear in
   the generated-crate error count — only a failing generated test would catch it.
2. **E0425 `smelt_capture_self` unresolved.** `Semaphore::acquire(&mut self)`
   references an `Rc<RefCell<Semaphore>>`-shaped `smelt_capture_self` that was
   never bound — because the `resolve` callback pushed into `deferredTasks`
   needs to capture a shareable `this`, which `&mut self` cannot provide. The
   emitter already *wants* the handle shape and half-emits it.
3. **Getter emitted as a stored field.** `get isLocked()` became a stored
   `is_locked: bool` field on `Mutex` instead of a computed method.

## The desired shape (compile-verified)

A thin **handle newtype** over `Rc<RefCell<Inner>>`; identity lives only in the
wrapper, so the inner fields stay concrete:

```rust
struct SemaphoreInner { capacity: f64, available: f64, deferred_tasks: VecDeque<ResolveFn> }
struct Semaphore(Rc<RefCell<SemaphoreInner>>);

impl Semaphore {
    fn new(capacity: f64) -> Self { Semaphore(Rc::new(RefCell::new(SemaphoreInner { .. }))) }
    fn acquire(&self, ..) { self.0.borrow_mut().available -= 1.0; .. }   // &self, always
    fn release(&self)     { .. }
    fn available(&self) -> f64 { self.0.borrow().available }             // getter → method
}
impl Clone for Semaphore { /* Rc::clone → shares identity */ }
```

The reference port compiles and passes reference-identity scenarios: an aliased
handle observes mutations made through the original and vice versa; a `Mutex`
delegating to `self.semaphore` mutates the real semaphore, not a clone.

## Five findings that shape the design

1. **`&self` uniformly.** Interior mutability removes the `&self`-vs-`&mut self`
   decision entirely. The E0596 self-borrow cluster AND the E0425 async-`this`
   cluster are the *same* missing abstraction, not two problems.
2. **`Clone` must mean "share identity"** (`Rc::clone`). This is the correctness
   core and it fixes the silent `Mutex` miscompile. The phase goal is not only
   "errors → 0" but "stop emitting code that compiles and lies."
3. **Classify; do not lift every class.** The handle costs a heap alloc +
   refcount + runtime borrow-check. Value records (frozen dataclasses, config
   objects that are constructed and read) stay by-value structs as today. Lift to
   a handle only when required — mirrors the module-globals "lift only if mutated"
   rule that shipped cleanly (see #138).
4. **Fields get simpler.** With identity owned by the outer `Rc`, `deferred_tasks`
   is a plain `VecDeque<ResolveFn>`, not the erased `SmeltList<Rc<dyn Fn>>` with
   the constructor's erase/un-erase round-trip. Identity concerns concentrate in
   one place.
5. **`RefCell` imposes a borrow discipline the emitter MUST follow.** Never hold a
   `borrow()`/`borrow_mut()` across a call that can re-enter the same object, or
   it panics. The port had to write `release()` as *pop-the-task → drop the borrow
   → call it*. This is a concrete, testable codegen rule and the main new hazard
   (runtime panic instead of compile error).

## Classification rule (V1)

Emit a class as a **reference class** (handle newtype) if ANY holds, else keep the
current by-value struct (**value class**):
- a method assigns to `this.<field>` (post-construction mutation), OR
- an instance binding is reassigned or aliased (`const b = a`) and later observed, OR
- `this` (or a method's `self`) is captured by a closure that escapes the method
  (stored in a field, returned, or passed to an async/Promise executor), OR
- the class is constructed then mutated through more than one live binding.

Getters (`get x()`) always lower to methods, never stored fields, regardless of
classification.

Explicit non-goals / named blockers for V1 (no `SmeltUnknown` to compile around):
- inheritance with a mutable base needing shared-`self` through `super` — defer
  with a named error until the hierarchy story is designed;
- `Weak` cycles / self-referential graphs — out of scope.

## Implementation phases

**Phase 1 — value classes unchanged; add the reference-class emitter.**
Classification pass over MIR classes; for reference classes emit
`struct Name(Rc<RefCell<NameInner>>)` + `struct NameInner { fields }`, a hand-
written `impl Clone` that clones the `Rc`, and `Default`/`Debug`/`IntoSmeltUnknown`
delegating through the cell. Constructor builds `NameInner` and wraps.

**Phase 2 — method bodies.** All methods take `&self`; field reads emit
`self.0.borrow().<f>` and writes `self.0.borrow_mut().<f> = …`, each in a narrow
scope. Enforce borrow discipline: compute-then-release before any call that could
re-enter (see `release()`). Getters → `&self` methods.

**Phase 3 — escaping `this` + async.** A closure capturing `this` captures
`self.clone()` (a cheap handle); `async fn m(&self)` can move `self.clone()` into
the returned future — clears the E0425 async-`this` blocker. Interacts with
[`cluster-b-promise-problem.md`](./cluster-b-promise-problem.md): the resolver
stored in `deferredTasks` is a `ResolveFn` over the promise's shared cell.

**Phase 4 — composition + aliasing.** A reference class held as a field of another
class shares identity through the inner `Rc` (fixes the `Mutex`→`Semaphore`
miscompile). Reassignment/aliasing of a reference-class binding clones the handle.

## Verification target

`es-toolkit` `src/promise/{semaphore,mutex}.ts` and their `.spec.ts` files:
generated crate compiles and the semaphore/mutex generated tests pass (the
reference port's scenarios are the acceptance shape). Confirm no regression on the
value-class path (remeda stays 1789/1789 — see `blocker-logs/remeda-regression.md`).
