# Typedness report: methodology

Design adapted from Bun's `--aot` (oven-sh/bun#45631),
`test/aot/typedness/run-typedness.ts` and `typedness.ts`, where each program
has one recorded line ("N layouts, G given, O open, places X of Y, reads A of
B") and may never become less typed than it.

## What is measured

`smelt typedness-report` (and the `smelt-typedness.json` sidecar `smelt build`
writes next to the generated `src/`) measures **optimized MIR**, the input codegen
reads, per source file:

| Kind | Counted |
| --- | --- |
| values | every local of every function and closure: parameters, bindings, temporaries |
| reads | every assignment of a property/element read: `Use` of a field/index place, `OptionalField`, `OptionalIndex`, `ListIndex`, `TupleIndex`, `DictGet`; classified by the type of the local it lands in |
| calls | every `Call` terminator and every `ClosureCall`/`ClosureCallSpread` statement (lowering picks between the two by context: inside a guarded `try` a closure call is a terminator); classified by its result type and, for a call through a value, by the callee value's type (the worse wins). Statements counted since round 6 of TS7 parity phase 2 |

Each entry is:

- **concrete** — its Rust type has no erased part;
- **generic** — it mentions a type parameter the Rust output carries as a real
  generic, with no erased part;
- **erased** — its Rust type mentions `SmeltUnknown` or `SmeltErasedFunction`
  anywhere (so `SmeltList<SmeltUnknown>` is erased: conservative, the metric can
  only under-state typedness).

Typed share = (concrete + generic) / total, per kind.

## Same predicates as codegen

The type of each entry is rendered with `FunctionEmitter::type_text` in an
emitter for its function, after the crate-wide generic decision
(`EmitContext::populate_generic_functions`) is applied exactly as `emit` applies
it. Union concreteness, erased type parameters, unresolved classes, `never`,
Bun's `int32` (`Type::FixedInt` renders `i32`, concrete) and strict JSON decode
targets (concrete records) are therefore decided by the code that decides them
for the generated crate. Closures are rendered in their owning function's
emitter because their bodies are emitted inside it.

## Inputs

- sidecar `.json`, or a directory holding `smelt-typedness.json` (directly or as
  the parent of a `src/` root): every file is a program;
- `Smelt.toml`: lowered in-process, every source file is a program;
- `.ts`/`.py`: lowered in-process with its local imports; the whole crate is one
  program named by the path as given (how the examples corpus is measured
  without a Rust build); a file that fails to lower is recorded `null` (refused).

## Ratchet

A program is no less typed than its record when, for values, reads and calls,
`erased(now) <= erased(then)`. The total is judged the same way. New programs
are not judged; a measured program that is now refused is a regression.
`--fail-on-regression` exits 1 listing each program and kind whose erased count
rose. `--update-baseline` rewrites the baseline taking only records that are no
less typed (Bun's `AOT_PROGRAMS_UPDATE=1`), dropping programs that no longer
exist. The typed share `(concrete+generic)/total` is still reported per kind and
in the change table, as information.

### Why the share no longer judges

- **Old requirement** (Bun's `isNoLessTyped`, used until the let-temp elision
  pass): `erased(now) <= erased(then)` **and**
  `typed(now) * total(then) >= typed(then) * total(now)`.
- **New requirement**: `erased(now) <= erased(then)` only.
- **Evidence**: counts are per MIR entry. MIR lowered every `let x = e` as
  `%t = e; %x = move %t`, so each binding was counted twice. The let-temp
  elision pass (`crates/smelt-mir/src/opt/let_temp_elision.rs`) removes the
  redundant temp; on es-toolkit this lowered the typed share of 115 files while
  no file's erased count rose. The generated Rust lost only a
  `let _smelt_tmp_N = ...;` per binding, its types unchanged. Under the old
  rule that improvement could not be snapshotted, and two records had already
  needed hand edits for the same reason (`ts7-parity-phase2-round2-hono.md`).
- **Why the guarantee is preserved**: the north star asks for as few erased
  values as possible. A value, read or call that loses its type is classified
  erased, so any type loss still raises an erased count and still blocks. What
  the share additionally blocked was only the removal of typed entries, which is
  not a typing regression.

| Baseline | Corpus | CI |
| --- | --- | --- |
| `typedness-baseline.json` | `examples/typescript/end-to-end/*/input.ts` | blocking |
| `typedness-baseline-es-toolkit.json` | es-toolkit build sidecar | blocking |
| `typedness-baseline-remeda.json` | remeda build sidecar | advisory |

## Limits

- Counts are per MIR entry, not per source expression: an optimization that
  removes temporaries changes totals and shares; the ratchet judges only the
  erased count, which such an optimization can only keep or lower.
- The measurement does not compile the Rust; it predicts it with the emitter's
  own type rendering.
