# The iterator protocol: hand-written iterators

Status: implemented (resume forwarding, `yield*` over hand-written iterators and adapter identity: the
iterator-protocol completion). Replaces the refusal #552 added for item 4 of the round-3 audit ("iterating an instance whose
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
| `Iterable<T>` | `Type::Function(() => Generator { T, undefined, undefined })` (its `[Symbol.iterator]`) | `Rc<dyn Fn() -> SmeltGenerator<T, (), ()>>` |

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
  `{ next, return, throw }` is not built as a record. Each member is lowered as a function answering
  `IteratorResult<T, R>` (their contextual type), in source order, and becomes an argument of the adapter below. A
  function-valued `[Symbol.iterator]` member is accepted and not kept.
* **Instance of a class (or record/interface value) whose `next()` answers `IteratorResult<T, R>`**, in an
  `Iterator<T>` slot (`[Symbol.iterator](): IterableIterator<T> { return this; }`, `drain(new Range())`) or
  answered by `[Symbol.iterator]()` to a consumer or to `yield*`: the adapter over its methods.
* **A generator of another type** in an `Iterator<T>` slot (a `Generator<T>` whose `TNext` is erased): the adapter
  over its own `next`/`return`/`throw`.
* **Step literal `{ value, done }` in an `IteratorResult<T, R>` slot**: `done: false` (or no `done`) builds
  `Yielded(value)`, `done: true` builds `Complete(value)` (`value` evaluated and dropped when `R` is `undefined`),
  and a computed `done` evaluates both properties once, in source order, then picks the arm at run time
  (HIR ops `GeneratorYielded` / `GeneratorCompleted`).

The adapter is the runtime carrier `SmeltGenerator::from_protocol` (HIR/MIR op `GeneratorFromProtocol`), built by an
immediately called synthesized closure over the iterator that hands it one closure per protocol method:

```text
(it) => from_protocol(
  it,                                         // the adapted object: its identity
  () => it.next(),     (sent: N) => it.next(sent),
  () => it.return(),   (value: R) => it.return(value),
  (error: unknown) => it.throw(error),
)
```

(those the iterator has; the `(sent)`/`(value)` forms only when the method declares a parameter and the channel is
not `undefined`). Each resume command the carrier receives is then exactly one call of the matching method, as
JavaScript's consumers make it:

* `Next(None)` calls `next()` (`for...of`, spread, destructuring, `gen.next()`), `Next(Some(v))` calls `next(v)`
  (`gen.next(v)`, `yield*` forwarding). The resume command carries whether a value was sent
  (`SmeltGeneratorCommand::Next(Option<N>)`, `Return(Option<R>)`): a generator body reads `None` as its resume
  type's default, as before, and a hand-written `next(sent?)` sees `undefined`. The first `gen.next(v)` reaches the
  iterator too (only a generator body discards the value that starts it).
* `Return(..)` calls `return()` / `return(v)` and answers its step (which may even yield); an iterator without
  `return` completes with the value sent. `for...of` leaving early, and a destructuring that took what it binds,
  send `Return(None)`: that is `IteratorClose`, and a `done` step or a throwing `next()` never reach it.
* `Throw(e)` calls `throw(e)`; without `throw` it closes the iterator and throws the `TypeError` `yield*` throws
  ("The iterator does not provide a 'throw' method.").

The carrier does not latch completion: a `next()` after a `done` step calls the hand-written `next()` again, as
JavaScript does (a generator body still answers `done` for good). Identity: the carrier keeps the adapted object
(when it is a reference class's handle), so `===` compares that object's cell address
(`SmeltGenerator::identity`, `binary_ops::reference_identity_expr`): `obj[Symbol.iterator]() === obj` holds for a
class adapted at its annotated `[Symbol.iterator](): IterableIterator<T> { return this; }`, and two adapters of one
object are `===`.

Consumers go through one helper, `iteration_generator` (`[Symbol.iterator]()` as a generator, or the adapter over
what it answers): `for...of`, array spread, `Array.from`, `new Set(x)`, `new Map(x)`, array destructuring and
`yield*`. Destructuring without a rest element takes at most as many values as it binds and then closes the
iterator (op `GeneratorTake`, runtime `SmeltGenerator::try_take_yields`), so `const [a, b] = naturals()`
terminates; with a rest element it drains it. An iterable passed for an `Iterable<T>` parameter (lowered as a
list) is drained into that list. `obj[Symbol.iterator]()` written by the source is the typed method call.

### `yield*`

In a synchronous generator, `yield* d` over any synchronous carrier (a generator, an adapted hand-written iterator)
is lowered as ECMA-262 states it, a block expression

```text
{ const d = <carrier>; let step = d.next();
  while (!step.done) step = <yield step.value, delegating to d>;
  step.value }
```

whose suspension (HIR `GeneratorYieldDelegated`, MIR `GeneratorYield { delegate: Some(d), .. }`) forwards the
command the outer generator is resumed with to `d`: `next(v)` and `throw(e)` answer `d`'s next step (a throw from
`d` takes the catch or `finally` around the suspension), and a `return(v)` that `d` completes ends the outer
generator with `d`'s value through the outer `finally` blocks, as a `return` at that point would; a `return(v)`
that `d` answers with a yield continues the delegation. Being a `yield` in MIR, the step reuses the catch and
`finally` edges every `yield` has (definite assignment, reachability). `yield*` over a list, a string, a tuple, a
union, and in an async generator keeps the `GeneratorDelegate` loop.

### `Iterable<T>`

`Iterable<T>` is the interface `{ [Symbol.iterator](): Iterator<T> }`, and Smelt lowers a one-method interface as
that method: an `Iterable<T>` is the thunk `() => Iterator<T>` (`lowering/stmt/iterable_values.rs`). It used to be
`T[]`, so an argument was drained into a list at the call: an infinite iterable never returned, and the producer's side
effects all ran before the consumer's.

* A value in an `Iterable<T>` slot (an argument, a return, an annotated binding) becomes a thunk over it, evaluated
  once where the slot is. Each call of the thunk answers a fresh iterator: an array's live element iterator (op
  `ListIterator`), a `Set`'s members, a `Map`'s entries, a string's code points, a class's `[Symbol.iterator]()`
  (adapted as above), or a generator itself (its own iterator, so a second iteration finds it exhausted, as in
  JavaScript). The value is lowered with the hint `T[]` first, so a literal takes its element types from `T`.
* Every consumer reaches an iterable through `iteration_generator`, which calls the thunk: `for...of`, spread,
  `Array.from`, `new Set`/`new Map`, destructuring, `yield*`, and `xs[Symbol.iterator]()` written by the source. A
  consumer that iterates twice gets two iterators, so an array restarts.
* An erased value read as an `Iterable<T>` (`x as Iterable<T>`) becomes a thunk that iterates it through the erased
  protocol each time it is called (its elements extracted to `T` as `T[]` would extract them): the erased boundary
  the source wrote stays where it was, and laziness is not kept across it.
* A generator's function-typed parameters (an `Iterable<T>` among them) are owned, since they move into its producer
  (`compute_owned_callback_params`).
* Beside an array arm in a union (`ArrayLike<T> | Iterable<T>`), inside an intersection (`P & Iterable<P>`), and as
  a type guard's or assertion's target (`value is Iterable<unknown>`), `Iterable<T>` keeps its array view `T[]`
  (`iterable_member_as_array`). A union of an array with a function value, or an intersection with one, has no
  concrete representation and would erase; a narrowing names a view of the value it tests, never a conversion. A
  value converted for such a slot is drained into the array there, as every `Iterable<T>` was before.

The thunk is precise: no erasure, `T` as declared. Like every interface-typed value it is not the object it was made
from: `Array.isArray(xs)` of an `Iterable<number>` parameter given an array is `false`, and `===` compares thunks.
A function value of type `() => Iterator<T>` is never an iterable in TypeScript (iterating one, or passing one where
an iterable is expected, is a type error), so the representations share it.

## 3. Why it is typed

The element type is the one the source declares (`Iterator<number>`: `SmeltGenerator<f64, (), ()>`;
`ListIter<T>`'s `next(): IteratorResult<T>`: `SmeltGeneratorResult<T, ()>`, generic). The adapter only moves
values of that type from `next()`'s step to the generator's `yield`, and the step literals are checked against
the declared `T`/`R` (refused when not assignable). Nothing erases: the iterator types no longer reach Rust as
`SmeltUnknown`, and no erasure is recorded for them.

## 4. Unannotated protocol methods

An unannotated `[Symbol.iterator]()` that answers an object literal, and an unannotated `next()` that answers step
literals, take their protocol type from the structure the TypeScript checker infers for their result
(`lowering/checker_types/iterator_shape.rs`). The rule is the checker's own (`getIterationTypesOfIterator`): an
iterator is an object whose `next` is a nullary function, with no members besides `return`, `throw` and
`[Symbol.iterator]`; each of `next`'s results is a `{ value, done }` object with a boolean `done`. The yield type is
the union of the values of the steps whose `done` is not `true`, the return type that of the steps whose `done` is not
`false`, and a value that is only `undefined` leaves `TReturn` open. The result is spelled as the annotation would
name it, `Iterator<T, R>` or `IteratorResult<T, R>`, and lowered through the ordinary annotation path, so the method
is typed exactly as if annotated. The checker widens a returned literal's `done: false` to `boolean`, so `{ value: s,
done: false } : { value: undefined, done: true }` yields `string | undefined` there, as `for...of` types it.
`[Symbol.iterator]() { return this; }` infers the polymorphic `this`, which lowers to the enclosing class (the
checker's constraint), as `: this` does. Without checker facts nothing states the step type, and such a class is
still refused.

## 5. Not covered

* An iterator with **state members** besides `next`/`return` (`{ i: 0, next() { return this.i++ … } }`), or whose
  `next`/`return` reads its own `this`. Smelt lowers every object literal with function-valued members as an erased
  record (`Dict<string, unknown>`), not only iterators, so no typed adapter can drive it. A literal written in an
  `Iterator<T>` slot is refused as before. One held in a variable and then returned into that slot is now refused too
  (it reached rustc as an erased record where a `SmeltGenerator` was expected). Typing it needs object literals with
  methods to lower as typed records whose methods bind `this`, a redesign of object-literal lowering in general. Keep
  the state in closures (section 0) or a class.
* **Identity through a by-value class.** The carrier takes its identity from a reference class's handle. A class that
  is never compared by `===` and has no other reason to be a reference (its `next()` writes a field, as almost every
  iterator's does) is a by-value struct with no identity, so two adapters of it compare as two objects
  (`a[Symbol.iterator]() === a[Symbol.iterator]()` with no `===` on `a` itself). A generator adapted to another
  carrier type has no object to keep either.
* **Done values through an open `TReturn` slot.** An annotated `next(): IteratorResult<T>` (the lib leaves `TReturn` at
  `any`) takes its return type from the steps its body returns (section 4's rule over the checker's type of each
  returned step literal), so `return { value: 0, done: true }` keeps the `0` (`open_step_return_type`). A value that
  then flows through a slot whose `TReturn` the source leaves at `any` (`Iterator<T>`, `IterableIterator<T>`) is
  dropped there: the source typed the completion `any`, keeping it would need the erased channel (`SmeltUnknown`),
  and every read of it in TypeScript is already `any`-typed. Declare `TReturn` to keep it
  (`Iterator<T, number>`). A completion read after a carrier finished answers the return type's default
  (`""` for a `string`), where JavaScript answers `undefined`.
* **`next(value)` into a generator body.** A `yield` evaluates to its resume type's default when resumed without a
  value (`gen.next()`, `for...of`), where JavaScript gives `undefined`; that is TypeScript's own unsoundness
  (`Generator<T, R, number>.next()` is accepted) and changes nothing for a `TNext` that includes `undefined`, which
  lowers to an `Option`. Async iterators (`[Symbol.asyncIterator]`, `for await`) are unchanged, and `yield*` in an
  async generator does not forward `return(v)`.

## 6. Related fixes the adapter needed

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
