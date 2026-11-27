# The iterator protocol: hand-written iterators

Status: implemented. Replaces the refusal #552 added for item 4 of the round-3 audit ("iterating an instance whose
`[Symbol.iterator]()` returns a hand-written iterator object"). Code: `crates/smelt-frontend-ts/src/lowering/stmt/iterator_protocol.rs`.

## 0. The problem

JavaScript iterates an object (`for...of`, spread, `Array.from`, array destructuring, `new Set(x)`, `new Map(x)`)
through its `[Symbol.iterator]()`: it calls `next()` until a step is `done`, and calls `return()` when the
consumer leaves early (`IteratorClose`: a `break`, a `return` or a throw out of a `for...of`, a destructuring
that took what it binds). Before this change Smelt modelled only a *generator* answering `[Symbol.iterator]()`;
any other iterator object was refused, and the lib types `Iterator<T>`, `IteratorResult<T>`, `IterableIterator<T>`
were opaque classes that erased to `SmeltUnknown`.

```ts
class Countdown {
  constructor(private from: number) {}
  [Symbol.iterator](): Iterator<number> {
    let n = this.from;
    return {
      next: () => (n > 0 ? { value: n--, done: false } : { value: 0, done: true }),
      return: () => { console.log("closed"); return { value: 0, done: true }; },
    };
  }
}
for (const v of new Countdown(3)) { if (v < 2) break; }   // calls return()
```

## 1. Representation

A hand-written port gives the protocol one typed shape: a trait object behind a shared handle for "an iterator of
`T`", and an enum for a step. Smelt already has both, as the carrier of generators:

| TypeScript (lib name, not shadowed by the source) | HIR | Rust |
|---|---|---|
| `Iterator<T, R, N>`, `IterableIterator<T, R, N>`, `IteratorObject<T, R, N>` | `Type::Generator { yield: T, return: R, next: N }` | `SmeltGenerator<T, R, N>` |
| `IteratorResult<T, R>`, `IteratorYieldResult<T>` | `Type::GeneratorResult { yield: T, return: R }` | `SmeltGeneratorResult<T, R>` (`Yielded(T)` / `Complete(R)`) |
| `Iterable<T>` (unchanged) | `Type::List(T)` | `SmeltList<T>` |

`SmeltGenerator` is a shared handle to one `FnMut(command) -> step` state machine: exactly an object with `next()`
(and `return()`, the `Return` command). So an `Iterator<T>` value is one whatever produced it: a `function*`, a
collection iterator, or a hand-written iterator object, and every consumer already drives it lazily.

Defaults. The lib leaves `TReturn` and `TNext` at `any`. Smelt reads both as `undefined` (`()`): an open
`TReturn` is the `undefined` that `{ value: undefined, done: true }` carries, and an open `TNext` is the nothing
`for...of` passes to `next()`. A generator function annotated with one of these interfaces takes the same open
`TNext`, so it flows into an `Iterator<T>` slot as the type it has there. (`Generator<T>`'s open `TNext` stays the
erased resume channel it was.)

## 2. Adapters

`lowering::stmt::iterator_protocol` turns a hand-written iterator into that carrier:

* **Iterator object literal in an `Iterator<T, R>` slot** (a return, an argument, an annotated binding):
  `{ next, return }` is not built as a record. `next` and `return` are lowered as functions answering
  `IteratorResult<T, R>` (their contextual type), and become the arguments of the adapter below. A function-valued
  `[Symbol.iterator]` or `throw` member is accepted and not kept.
* **Instance of a class (or record/interface value) whose `next()` answers `IteratorResult<T, R>`**, in an
  `Iterator<T>` slot (`[Symbol.iterator](): IterableIterator<T> { return this; }`, `drain(new Range())`) or
  answered by `[Symbol.iterator]()` to a consumer: the adapter over its methods.
* **A generator of another type** in an `Iterator<T>` slot (a `Generator<T>` whose `TNext` is erased): the adapter
  over its own `next`/`return`.
* **Step literal `{ value, done }` in an `IteratorResult<T, R>` slot**: `done: false` (or no `done`) builds
  `Yielded(value)`, `done: true` builds `Complete(value)` (`value` evaluated and dropped when `R` is `undefined`),
  and a computed `done` evaluates both properties once, in source order, then picks the arm at run time
  (HIR ops `GeneratorYielded` / `GeneratorCompleted`).

The adapter is a synthesized generator closure, called once with the iterator (or its `next`/`return` closures):

```text
function* (it) {
  while (true) {
    const step = it.next();
    if (step.done) return;          // a done step never closes the iterator
    let open = true;
    try { yield step.value; open = false; }
    finally { if (open) it.return(); }
  }
}
```

A consumer that leaves early resumes the generator with `Return` at the `yield`, as it closes any generator, so
the `finally` calls the iterator's `return()`. A normal resume clears `open` first; a `done` step and a `next()`
that throws leave outside the `try`. That is `IteratorClose`, by construction, for every consumer that already
closes generators (`for...of`'s `finally`). An iterator without `return()` gets no `try`.

Consumers go through one helper, `iteration_generator` (`[Symbol.iterator]()` as a generator, or the adapter over
what it answers): `for...of`, array spread, `Array.from`, `new Set(x)`, `new Map(x)`, and array destructuring.
Destructuring without a rest element takes at most as many values as it binds and then closes the iterator (new
op `GeneratorTake`, runtime `SmeltGenerator::try_take_yields`), so `const [a, b] = naturals()` terminates; with a
rest element it drains the iterator. An iterable passed for an `Iterable<T>` parameter (lowered as a list) is
drained into that list. `obj[Symbol.iterator]()` written by the source is the typed method call.

## 3. Why it is typed

The element type is the one the source declares (`Iterator<number>`: `SmeltGenerator<f64, (), ()>`;
`ListIter<T>`'s `next(): IteratorResult<T>`: `SmeltGeneratorResult<T, ()>`, generic). The adapter only moves
values of that type from `next()`'s step to the generator's `yield`, and the step literals are checked against
the declared `T`/`R` (refused when not assignable). Nothing erases: the iterator types no longer reach Rust as
`SmeltUnknown`, and no erasure is recorded for them.

## 4. Not covered

* An **unannotated** `[Symbol.iterator]()` (or `next()`) returning an object literal: Smelt types an object literal
  with function-valued members, and an inferred record return, as an erased record, so the step type is unknown.
  Iterating such a class is refused with a message asking for `[Symbol.iterator](): Iterator<T>`. Typing it needs
  record return inference from the checker (or the checker's iteration types), a separate change.
* An iterator object literal with **state members** besides `next`/`return` (`{ i: 0, next() { return this.i++ … } }`),
  or whose `next`/`return` reads its own `this`: refused; keep the state in closures (as above) or a class.
* **Identity**: the adapter is a new object, so `it[Symbol.iterator]() === it` is `false` for an adapted iterator,
  and `next()` called directly on an adapted `Iterator<T>` after it completed answers `done` without calling the
  hand-written `next()` again (the carrier remembers completion).
* **Done values with an open `TReturn`**: `{ value: 0, done: true }` in an `IteratorResult<number>` (whose `TReturn`
  the lib leaves at `any`) drops the `0`, so reading `.value` of that done step answers `undefined`. No consumer of
  the protocol reads it; declare `TReturn` to keep it.
* `throw()` of a hand-written iterator is never called (no synchronous consumer calls it); `next(value)` arguments
  are not forwarded through the adapter. Async iterators (`[Symbol.asyncIterator]`, `for await`) are unchanged.
* `Iterable<T>` stays a list: an iterable passed for an `Iterable<T>` parameter is drained eagerly, so an
  infinite one does not terminate there (as for generators before).

## 5. Related fixes the adapter needed

* A synchronous generator that can throw propagates a throwing call in its body with `?` (its producer already
  answers `Result`), instead of panicking inside the `genawaiter` producer: the panic poisoned the generator, so the
  consumer's closing `return()` aborted with "`async fn` resumed after panicking" (a hand-written `next()` that
  throws).
* A `yield` inside `try`/`finally` inside a loop: the `Throw` arm's binding of the pending throw is `let mut`, since
  the continuation it inlines assigns it again (E0384 before).
* An `IteratorResult<A, R>` read at another step type (`IteratorYieldResult<[number, string]>` of `entries().next()`)
  converts each arm's payload, as tuples and lists do.
* `obj[Symbol.iterator]()` on an instance whose class declares it is the typed method call (it was an erased value
  call, which could not land in a typed `Iterator<T>`).

Fixed separately: emitting a function with many sequential `for...of` loops over generators took time exponential
in their number (the `next()` call's handler leaves the loop for the `finally`, and each such exit re-emitted the
rest of the function: 6 loops took ~90 s). The Rust emitter now rejoins a loop's early exits at their common
post-dominator behind a labelled block (`smelt-codegen-rust` `control_flow/loop_exit_join.rs`).
