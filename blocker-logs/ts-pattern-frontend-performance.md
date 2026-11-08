# ts-pattern: frontend performance and blockers

Corpus: `~/scratch/smelt/corpora/ts-pattern` (roots `src`, `tests`; 18 src + 48
test files), copied to scratch before every run. Binaries are debug builds
(`corgi build -p smelt-transpiler --bin smelt`, dev profile, opt-level 0).
Machine: 16 cores, shared with other agents, so wall times carry noise of
several seconds; every run under `~/scratch/smelt/capped` (12 GiB, no swap).

## Benchmark definition (fixed before any change)

- **Per file:** `SMELT_TIMINGS=1 smelt dump-hir tests/<file>.test.ts` (lowers the
  file and its `src` imports; the checker sees the same files), wall time and
  peak RSS from `/usr/bin/time`, 900 s timeout, two files at a time. Script:
  `~/scratch/smelt/tsp/bench/bench.py <smelt> <label> 2`.
- **Whole build:** `SMELT_TIMINGS=1 smelt --manifest-path Smelt.toml build` on
  the corpus copy, 1500 s timeout.
- **Acceptance:** the examples byte oracle (`~/scratch/untangle/oracle.py`,
  examples + es-toolkit, remeda, radash, hono, date-fns) stays identical, except
  where a row is listed under "Oracle deltas" with its cause.

`base` is main at 04a2861c. `v2` is base plus the two performance changes
below (no frontend rule changes yet). Rows 22-37 of `v2` ran while a probe
of the whole corpus shared the CPU and are inflated by it.

| file | base wall s | base | base RSS MB | v2 wall s | v2 | v2 RSS MB |
| --- | ---: | --- | ---: | ---: | --- | ---: |
| `src/index.ts` | 69.6 | ok | 1361 | 11.0 | ok | 391 |
| `tests/bigints.test.ts` | 85.9 | ok | 1514 | 12.6 | ok | 425 |
| `tests/branded-nominal-types.test.ts` | 76.6 | ok | 1368 | 11.8 | ok | 395 |
| `tests/build-many.test.ts` | 0.4 | ok | 67 | 0.4 | ok | 65 |
| `tests/chainable.test.ts` | 136.1 | ok | 2634 | 18.4 | ok | 631 |
| `tests/deep-exclude.test.ts` | 1.6 | ok | 83 | 1.7 | ok | 80 |
| `tests/distribute-unions.test.ts` | 1.7 | ok | 81 | 1.3 | ok | 77 |
| `tests/exhaustive-fallback.test.ts` | 69.5 | ok | 1367 | 11.2 | ok | 394 |
| `tests/exhaustive-match.test.ts` | 900.2 | timeout | 2129 | 22.2 | error | 565 |
| `tests/extract-precise-value.test.ts` | 57.6 | ok | 1190 | 6.9 | ok | 280 |
| `tests/find-selected.test.ts` | 56.1 | ok | 1193 | 6.8 | ok | 286 |
| `tests/generics.test.ts` | 78.1 | ok | 1379 | 11.5 | ok | 401 |
| `tests/helpers.test.ts` | 0.3 | ok | 66 | 0.3 | ok | 64 |
| `tests/infer.test.ts` | 79.3 | ok | 1389 | 10.9 | ok | 396 |
| `tests/instance-of.test.ts` | 84.2 | ok | 1480 | 11.7 | ok | 420 |
| `tests/intersection-and-union.test.ts` | 230.8 | ok | 2106 | 16.4 | ok | 538 |
| `tests/invert-pattern.test.ts` | 161.0 | ok | 1175 | 6.9 | ok | 283 |
| `tests/is-matching.test.ts` | 79.1 | ok | 1428 | 11.6 | ok | 412 |
| `tests/large-exhaustive.test.ts` | 91.2 | ok | 1435 | 15.1 | ok | 416 |
| `tests/lists.test.ts` | 75.4 | ok | 1391 | 11.2 | ok | 404 |
| `tests/maps.test.ts` | 77.9 | error | 1431 | 14.3 | error | 413 |
| `tests/matcher-protocol.test.ts` | 73.9 | error | 1370 | 25.6 | error | 398 |
| `tests/multiple-patterns.test.ts` | 76.7 | error | 1453 | 36.5 | error | 421 |
| `tests/narrow.test.ts` | 72.9 | ok | 1401 | 31.1 | ok | 405 |
| `tests/nesting.test.ts` | 70.0 | ok | 1383 | 34.1 | ok | 407 |
| `tests/not.test.ts` | 92.3 | ok | 1951 | 35.1 | ok | 484 |
| `tests/numbers.test.ts` | 104.3 | ok | 2155 | 37.8 | ok | 515 |
| `tests/objects.test.ts` | 67.6 | ok | 1369 | 35.2 | ok | 399 |
| `tests/optional-props.test.ts` | 72.1 | error | 1387 | 35.0 | error | 402 |
| `tests/optional.test.ts` | 85.9 | ok | 1799 | 41.7 | ok | 437 |
| `tests/otherwise.test.ts` | 74.6 | ok | 1363 | 38.9 | ok | 391 |
| `tests/output-type.test.ts` | 71.6 | ok | 1370 | 38.5 | ok | 400 |
| `tests/pattern.test.ts` | 67.8 | ok | 1362 | 38.1 | ok | 391 |
| `tests/primitive-values.test.ts` | 900.2 | timeout | 1508 | 40.5 | ok | 395 |
| `tests/readonly.test.ts` | 78.7 | ok | 1379 | 42.7 | ok | 401 |
| `tests/real-world.test.ts` | 900.2 | timeout | 1709 | 63.5 | ok | 459 |
| `tests/record.test.ts` | 900.3 | timeout | 2894 | 53.2 | ok | 678 |
| `tests/return-type.test.ts` | 69.3 | ok | 1365 | 18.5 | ok | 393 |
| `tests/select.test.ts` | 109.9 | ok | 1956 | 20.2 | ok | 492 |
| `tests/sets.test.ts` | 91.1 | ok | 1465 | 14.7 | ok | 415 |
| `tests/strings.test.ts` | 123.9 | ok | 2252 | 18.6 | ok | 543 |
| `tests/tuples.test.ts` | 70.2 | ok | 1385 | 14.7 | ok | 409 |
| `tests/type-error.test.ts` | 71.3 | ok | 1392 | 13.5 | ok | 404 |
| `tests/type-is-matching.test.ts` | 0.5 | ok | 65 | 0.6 | ok | 66 |
| `tests/types.test.ts` | 178.5 | ok | 1922 | 16.0 | ok | 469 |
| `tests/unions.test.ts` | 73.6 | ok | 1373 | 14.6 | ok | 398 |
| `tests/variadic-tuples.test.ts` | 96.7 | ok | 2063 | 19.7 | ok | 525 |
| `tests/when.test.ts` | 76.2 | ok | 1432 | 14.7 | ok | 417 |
| `tests/wildcards.test.ts` | 74.3 | ok | 1434 | 12.4 | ok | 416 |
| **sum** | **7157** | | max 2894 | **1020** | | max 678 |

## Where the time went (profiles)

`perf` and `gdb` are unavailable, so profiles came from `SMELT_TIMINGS`, temporary
per-phase timers in `smelt-ts-types`, and a temporary in-process sampler
(SIGPROF every 10 ms, `_Unwind_Backtrace`, symbolized with `nm`), all reverted.

1. **Checker (`manifest.ts7_check`), base `strings.test.ts`, 115 s.** The full
   TypeScript check (`get_semantic_diagnostics`) took 0.96 s. The span walk took the
   rest, and 97 % of it was `typeToString` for every interned type (59-83k types
   per heavy file, e.g. `Pattern.ts` 30.3 s of 31.5 s). `typeToString` prints a
   whole type before truncating it, so the cost is proportional to type size per
   type. Lowering never reads `TsType::display` (only the ts7 parity report and
   the `smelt-ts-types` tests do).
2. **Frontend lowering, base `exhaustive-match.test.ts`, >800 s.** A member call
   lowered its receiver several times: the owned-member probe lowered it and
   rolled back, then `present_list_receiver` (for `.with`, a name `Array.prototype`
   also has), `callable_static_member_call` and `static_member_receiver_dispatches_dynamically`
   each lowered it again. For `match(x).with(..)` chains of up to 31 links that
   is ~3^n receiver lowerings. Linear item scans (`find_type_alias`,
   `find_interface`, `class_by_symbol`) were the self-time hot spots inside it.
3. **MIR, whole build, >13 min after the frontend finished.**
   `volatile_locals::reads_global` re-walked each child's subtree from every
   enclosing expression (unmemoized, while its sibling `can_run_code` is memoized).

## Changes

| change | rule | verified by |
| --- | --- | --- |
| `Spellings::Omitted` for the build path | lowering asks for type structure only; `display` is `None` unless a caller asks for spellings | `omitted_spellings_keep_every_kind_and_alias` (mutant that drops aliases in omitted mode fails it); full oracle identical |
| kept receiver | the owned-member probe keeps its lowering and every later unhinted lowering of that receiver node in the same call reuses it; a roll-back below it discards it | `receiver_reuse_tests` (old roll-back behaviour fails it: 8 -> 20 bodies); full oracle: generated crates identical, see deltas |
| memoized `reads_global` | answer per expression once | examples oracle identical |

Note on `Spellings::Omitted`: `typeToString` has checker side effects. Without it
the walk interns more distinct types (`Pattern.ts` 59,291 -> 86,261: fresh
signature type-parameter clones), because TypeScript type identity depends on
query order. Facts compared span by span on the probe sources were identical in
kind and alias, and the examples and five corpora are byte-identical, but the
per-root expansion budget can now truncate a different set of deep types in
type-heavy code.

### Oracle deltas

`dump-hir` text of 9 TypeScript examples (132, 133, 151, 172, 177, 180, 187,
190, 84) differs: those programs had redundant re-lowered receiver expressions
(orphans in the expression arena) that are no longer created (198 -> 194
expressions in 132, etc.). Their interned types, `dump-mir`, generated crates and
stderr are byte-identical, as are all five corpora. A change that stops
re-lowering cannot keep the arena listing identical.

## Whole build

| run | checker | frontend total | MIR + emit | wall | peak RSS | result |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| previous agent | - | - | - | >6600 s | 10.8 GB | timeout |
| base `src` + tests | - | - | - | - | - | per-file timeouts above (900 s) |
| final tree | 90-114 s | 93-124 s | 3.6 s | 98-127 s | 2.49 GB | exit 0, 69 generated files |

The checker share (~110 s over src + 48 tests) is now `get_type_at_location`
plus conversion of ~1.5M structurally distinct types in a debug build.

## Frontend blockers fixed (each with a Node parity fixture)

| blocker | rule | fixture |
| --- | --- | --- |
| `TSEnumDeclaration` in a function body | a block-local enum folds like a module enum (auto-increment from 0 and from the last number, string members), registered for the rest of the module, not published to importers; numeric members get the reverse mapping `E[n]` (constant key folds, other keys index the `{"0": "A", ..}` table, later member wins on duplicate values) | `enums` |
| "array callback methods require exactly one callback argument" | with an arity other than one, `Array.prototype` claims only array-like receivers (list, tuple, union with a list arm, erased, type parameter) | `namespace_reexports` |
| `P.map` etc. on a namespace | a member a crate module's namespace exports is a call to that export before any name-keyed builtin handler; `import { P }` of `export { Ns as P }` / `export * as P` binds the namespace; a same-name re-export of an import (`import { m }; export { m }`) is published | `namespace_reexports` |
| "property names must be static identifiers or string literals" (`[P.matcher]()`) | resolved by the re-export rules above: `P.matcher` reaches the `Symbol.for` const | `namespace_reexports` |
| "new Map([...]) requires a Map<K, V> type annotation" (2 sites) | a block-local alias redeclared later replaces the earlier one's fields and takes precedence over an earlier same-named interface; `new Map` takes the only map arm of a union contextual type | `block_local_type_alias_redeclaration` |
| "namespace member item is not usable as an expression" (`P.string`) | a computed module-level const read through a namespace is a `GlobalGet`, as a named import of it is | `namespace_reexports` (`P.boxed.n`) |
| "instanceof target `Some` is not a lowered class" | a class named from inside its own members (or bound under a block-scoped name) is nominal before its item exists | `namespace_reexports` |
| MIR "abstract class cannot be constructed" (`P.instanceOf(Abstract)`) | an abstract class value carries no constructor closure | unit test `abstract_class_value_tests` |
| main-thread stack overflow in MIR | the CLI runs on a 1 GiB-stack thread (same error text and exit status) | full build |

The "spread call requires at least one argument" errors the previous agent saw
did not reproduce on main 04a2861c.

## What remains

The generated crate does not compile (`cargo test`: 18,799 errors).

- 18,784 `E0404 expected trait, found struct Fn`: ts-pattern declares a type
  `Fn` (`unstable_Fn`), emitted as `struct Fn`, which shadows the `Fn` trait
  every closure type uses. `sanitize_identifier` in
  `crates/smelt-codegen-rust/src/rust.rs` renames only `Box Vec Option Result
  String Some None Ok Err main`; the general fix is the whole Rust prelude
  (`Fn FnMut FnOnce Clone Copy Default Drop Iterator Into From ...`). Not made:
  codegen, and disk was below the 25 GB stop line.
- 8 `E0433 indexmap`: the crate uses `indexmap` without declaring it.
- 4 `_` used as an identifier; 2 derive token errors.
- 1 `E0428 is_matching` defined twice.
- Pre-existing codegen gap, also on base: a `static` and an instance member of
  one name (`static [P.matcher]()` and `[P.matcher]()`, or `static foo` and
  `foo`) emit two Rust methods of one name (`E0592`).
- `E[k]` reverse mapping with an out-of-range numeric key yields `""` (the
  record read default) where JavaScript yields `undefined`, which TypeScript
  types `string`.

## Round 2: merge of origin/main (#326, #328) and ts-pattern codegen names

The merge had no textual conflicts. New general rules since then, each with a
Node parity fixture that fails before the change:

| rule | fixture |
| --- | --- |
| `sanitize_identifier` reserves every Rust prelude item the generated crate spells unqualified and every non-`smelt` runtime root item (`Fn FnMut FnOnce Clone Copy Default Drop Eq Ord PartialEq PartialOrd AsRef From Into FromIterator IntoIterator Iterator IntoFuture Send Sized ToOwned ToString drop DateLike IntoSmeltUnknown`, besides the existing list). `tests/reserved_name_tests.rs` derives the required set from the fully enabled runtime and every committed example crate | `prelude_named_items` (102 E0404 before) |
| a block- or suite-local `type`/`interface` reusing a name the crate declares gets its own scoped symbol (as block-local classes do); a block-local class is scoped when an earlier type took the name | `block_local_type_alias_redeclaration` (union-after-interface block) |
| free functions are de-duplicated by rendered name (a module body function vs the module's own export); a static method sharing an instance method's name renders `<name>_<id>`; a lone `_` renames like a keyword | `rendered_name_collisions` |

ts-pattern generated test crate: 18,799 -> 384 compile errors. Every class the
first round listed is gone (E0404 `Fn`, `indexmap`, `_`, derive, E0428, E0592,
E0609). What remains is type errors:

- ~200 `expected T, found SmeltUnknown` and 52 closure-parameter mismatches
  (`Fn(&JsString)` vs `Fn(&SmeltUnknown)`): record adapters between
  instantiations of the generic `Match<..>` interface do not adapt
  function-valued fields across type arguments (the coercion seam's
  `record_adapter`), and erased values flow into typed record/list/union slots
  without extraction. Largest files: exhaustive_match_test (61),
  variadic_tuples_test (48), types_test (30).
- Calling a symbol-keyed static method through the class value
  (`Counter[key]()`) panics at run time on main as well (ts-pattern's matcher
  protocol does exactly this).

### Oracle classes (origin/main bf19f38ee binary vs branch 7f2edc4cc binary)

- ts-pattern: main's binary did not finish within 1500 s (killed); the branch
  builds it (73 rows).
- 9 TypeScript `dump-hir` arena listings (accepted; goldens updated).
- es-toolkit, remeda, hono: reserved-name renames (`drop` -> `drop_` including
  `drop.rs` -> `drop_.rs`, `ToString` -> `ToString_`), a source local `_` ->
  `__`, scoped block-local types (new `PersonSmeltSuiteF..`, `CSVSmeltSuiteF..`
  structs where one collapsed struct existed) and the resulting renumbering of
  interned `SmeltUnion`/`SmeltCallableObject` names. Test results unchanged.
- radash: `_.keys({})` in its tests now calls radash's own `keys` export
  instead of the `Object.keys` builtin (crate namespace members are calls to
  their exports); its erased typedness count rises by 2 (the export's
  `TValue extends object` parameter). Tests unchanged (383/4).

## Round 3: merge of origin/main through 463a1af9 (#329, #330)

Clean merge. On the merged tree: runtime tiers 77/77 (`--test-threads=2`),
goldens unchanged, libs and pins unchanged, clippy at baseline (1787). The
oracle's changed-row set against main's binary is identical to round 2's.

The ts-pattern test crate still stops at 384 errors; 326 of its 369 E0308 are
one mechanism. `match<T>(value)` lowers to `match__62(..)`, which returns
`Match<SmeltUnknown>`; the call site's declared type is `Match<T>`, so the
record adapter rebuilds the struct field by field (`Match { with: s.with.clone(),
.., narrow: { adapter } }`). Fields whose type mentions the parameter in a
callback position (`otherwise: Rc<dyn Fn(Rc<dyn Fn(&i) -> SmeltUnknown>) -> ..>`,
and recursively `narrow: Rc<dyn Fn() -> Match<..>>`) are copied with `.clone()`
instead of being wrapped in a function adapter, so `Fn(&SmeltUnknown)` meets
`Fn(&T)`. See the final report of this round for file:line evidence.
