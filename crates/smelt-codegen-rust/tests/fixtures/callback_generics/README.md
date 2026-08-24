# Callback-generics fixture corpus

Rescued repro projects from the callback-generics campaign (PRs #202, #203).

## Why these exist

That campaign shipped six defects that reached a verification round before being
caught. Five of the six were **the emitter disagreeing with itself about one
value** — a passthrough branch claiming an argument the borrowed-callback branch
owned, an adapter declaring its parameter at the substituted type but converting
its body from the unsubstituted one, an adapter leaving its return
unsubstituted, a call site using the callee's declared return type where the
emitted call evaluates to the substituted one, and a parameter that is both a
monomorphizing composite and mutably borrowed. The sixth was a generated `F0`
colliding with a source class named `F0`.

Every one was found by *constructing a source shape the three compat corpora
(es-toolkit, remeda, radash) do not contain* and compiling the generated Rust.
The corpora were green through all six. These fixtures are that shape space,
curated and made permanent.

## Layout

One TypeScript program per file. The file stem is the shape it exercises, and
the header comments record the area and the defect class it guards:

```ts
// Fixture: callback_returns_type_param
// Area: adapter_substitution
// Guards: adapter return left unsubstituted while its parameters were substituted.
```

Areas:

| Area | What varies |
| --- | --- |
| `passthrough_ladder` | which branch owns an argument: omitted/optional/narrowed callbacks, and mutable composites that also monomorphize |
| `adapter_substitution` | where `T` is reachable — callback return, composite return, second type parameter, `Promise`/list/optional wrappers |
| `site_pinning` | how a call site pins the callee: concrete, erased, function item, unannotated, two sites at once |
| `callback_shape` | the callback's own shape: rest, default, optional, higher-order, list-of-callbacks, owned-by-return, reassigned |
| `containers` | the generic container: `Map`, `Set`, `Iterable`, `WeakMap` |
| `dispatch` | how the call is reached: hops, recursion, methods, static methods, `async`, overloads, throwing bodies |
| `naming_collisions` | source names that collide with generated or reserved Rust names (`F0`, `gen`) |

## How they run

`crates/smelt-codegen-rust/tests/compile_corpus.rs`, test
`callback_generics_fixtures_compile`. It lowers each fixture through the real
pipeline, emits a crate via `emit_crate`, and runs `cargo check` on it, sharing
one `CARGO_TARGET_DIR` across the corpus.

```sh
just test-callback-generics-fixtures
# or one fixture:
SMELT_CORPUS_ONLY=callback_returns_type_param \
  cargo test -p smelt-codegen-rust --test compile_corpus -- --ignored callback_generics_fixtures_compile
```

## Fixtures that do not compile

Some fixtures are recorded in `EXPECTED_FIXTURE_FAILURES` with their error count
and cause, rather than being deleted — a shape that is broken is exactly the
shape worth keeping. The tier fails in **both** directions: when a fixture that
must compile stops compiling, and when a recorded failure starts compiling (fix
it, then delete its record) or its record names a fixture that no longer exists.

Error-count drift is printed but does not fail the tier; rustc's grouping of
diagnostics is not a stable interface, pass/fail is.

## The generated grid next door

These fixtures were hand-written; `crates/smelt-codegen-rust/tests/shape_grid.rs`
*enumerates* the same space instead. It crosses eight axes (generic arity, where
the type parameter occurs, the callback's spelling, the return shape,
mutability, what the call site hands over, how many sites, and whether the
calling function is itself generic) and renders every legal point as a small
TypeScript program, then transpiles and `cargo check`s it. The two are complementary and neither replaces the other: the grid is
systematic but only knows the shapes someone thought to make an axis, while
these fixtures carry specific, irregular shapes — a source class named `Box`, a
function named `gen`, a generic class, a `Map` receiver — that no axis crossing
would ever produce.

A fixture whose shape the grid now covers exactly is a fixture to delete. Check
before adding one.

## Adding a fixture

Add a `.ts` file with the three header lines. Nothing else registers it — the
tier reads the directory. Earn the slot: it should exercise a shape no existing
fixture and none of the compat corpora already covers.

## Provenance

Curated from 133 agent-built repro projects, of which 99 were kept as 100
fixtures (one project was split in two: its shape probe and the `gen`-keyword
collision it happened to also trip over). The 34 dropped were 15 byte-identical
duplicates (the `head_`/`nn_` counterfactual copies), 8 throughput benchmarks
rather than shape probes (up to 200 generated functions each), and 11 strict
subsets of a surviving sibling.
