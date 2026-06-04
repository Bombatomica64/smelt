# Remeda Remaining Failure Sweeps

Baseline: `blocker-logs/remeda-sumby-full-after.md`

- Passing: 1526
- Failing: 263
- Failing test groups: 67

These are root-cause-oriented working groups, not mutually exclusive buckets.
Files may move between groups after a focused investigation.

## Recommended Sweeps

### 1. Data-last, purry, lazy-pipeline, and callback forwarding

Estimated scope: 70-90 failures.

Representative groups:

- `constant`, `identity`, `filter`, `first`, `flat`, `flatMap`, `forEach`
- `map`, `mapWithFeedback`, `partition`, `reduce`, `split`, `tap`
- `unique`, `uniqueBy`, `uniqueWith`, `when`, `zipWith`

Common symptoms:

- Data-first tests pass while corresponding data-last or piped tests fail.
- Lazy execution counts or early termination differ.
- Extra callback arguments or optional initial arguments are lost.
- Results fail `toBe` or `toStrictEqual` without a panic.

Likely shared areas:

- Purry/data-last overload lowering.
- Callback argument forwarding and callable ABI adaptation.
- Lazy pipeline composition and early termination.
- Optional argument presence versus absent values.

### 2. Dynamic object shape, keys, paths, and property semantics

Estimated scope: 55-70 failures.

Representative groups:

- `evolve`, `groupBy`, `groupByProp`, `mergeAll`, `mergeDeep`
- `omit`, `omitBy`, `pickBy`, `pullObject`, `setPath`
- `hasProp`, `isEmptyish`, `isObjectType`, `isPlainObject`

Common symptoms:

- Missing field panics.
- `unknown is not object`, `unknown is not null`, or property-presence mismatches.
- Symbol, prototype, optional, numeric, and reserved-name keys behave incorrectly.
- Nested updates or transformations lose shape.

Likely shared areas:

- Record/object spread, computed keys, and property-presence lowering.
- Path traversal and nested immutable update semantics.
- Dynamic object enumeration, prototype, symbol, `length`, and `size` behavior.
- Concrete record-to-dynamic-object boundary adapters.

### 3. Equality, identity, ordering, and collection membership

Estimated scope: 35-50 failures.

Representative groups:

- `isDeepEqual`, `isShallowEqual`, `isStrictEqual`, `isIncludedIn`
- `firstBy`, `intersection`, `intersectionWith`
- Parts of `unique`, `uniqueBy`, `uniqueWith`, `sort`, `sortBy`, `rankBy`

Common symptoms:

- Reference identity and structural equality are conflated.
- Arrays, objects, sets, functions, `null`, and `undefined` compare incorrectly.
- Comparator results for booleans, objects, and `valueOf` differ.

Likely shared areas:

- JS strict/reference equality versus structural test equality.
- Dynamic value identity representation.
- Comparator normalization and ordering of dynamic values.
- Set and multiset semantics.

### 4. Timers, closures, and mutable callback state

Estimated scope: 47-50 failures.

Representative groups:

- `debounce`
- `funnel`
- `funnel_lodash_debounce`
- `funnel_lodash_debounce_with_cached_value`
- `funnel_lodash_throttle_with_cached_value`
- `funnel_remeda_debounce`
- `funnel_reference_batch`

Common symptoms:

- Leading/trailing calls, cancellation, flushing, and cached return values fail.
- Recursive and delayed calls observe stale state.
- Full runs are substantially slower around these tests.

Likely shared areas:

- Timer runtime scheduling and cancellation.
- Closure capture mutation and shared state.
- Cached callback return values.
- Recursive/reentrant callback invocation.

### 5. Optional values, indexing, iterability, and narrowing

Estimated scope: 35-55 failures, overlapping heavily with sweeps 1 and 2.

Representative groups:

- `first`, `last`, `length`, `nthBy`, `split`, `zipWith`
- `randomBigInt`, `uniqueBy`
- Parts of `map`, `partition`, and lazy pipelines

Observed panic families in the full output:

- `unknown is not iterable`
- `unknown is not array`
- `optional value was absent after narrowing`
- `index out of bounds`
- `negative index out of bounds`

Likely shared areas:

- Preserve concrete iterable/list/tuple types through generic helpers.
- Optional presence narrowing across branches and callbacks.
- JS negative/out-of-range indexing behavior.
- Typed iterable adapters rather than `SmeltUnknown`.

### 6. Numeric domains and randomness

Estimated scope: 20-30 failures.

Representative groups:

- `randomBigInt`, `randomInteger`, `median`, `shuffle`
- Parts of `sort`, `sortBy`, and `rankBy`

Common symptoms:

- Big integers are lowered through `f64`/`i64`.
- Range generation and random-result validation fail.
- Random collections hit iterable-boundary panics.

Likely shared areas:

- BigInt representation and arithmetic.
- Numeric conversion rules and range boundaries.
- Random integer/BigInt runtime behavior.

## Raw Runtime Symptom Counts

The full `--nocapture` run contained:

- 83 `toBe` assertion failures.
- 74 `toStrictEqual` assertion failures.
- 3 `toHaveLength` assertion failures.
- Repeated panics including `missing field`, `unknown is not iterable`,
  `unknown is not array`, `optional value was absent after narrowing`, and
  index-out-of-bounds failures.

Assertion categories describe the visible failure, not the root cause. Focused
reports are still required before implementing each sweep.

## Suggested Order

1. Data-last/purry/callback forwarding.
2. Optional/indexing/typed iterable preservation.
3. Dynamic object shape and property semantics.
4. Equality and identity.
5. Timers and mutable closure state.
6. BigInt and numeric domains.

The first three sweeps have the broadest overlap and are most likely to remove
failures across many test files at once.
