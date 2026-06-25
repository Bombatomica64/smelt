# Remeda deep-failure clusters — refactor plans

The 14 Remeda failures that predate (and are independent of) the distinct-`undefined`
work. Each is a structural/representation gap, not a bounded patch. Grouped into
five clusters with root cause + approach + risk. Baseline: these are the residual
failures after `c4d6b257` (distinct-`undefined` producer sweep).

---

## Cluster A — Reference identity for value-typed arrays/objects (BIG)

**Tests (5–6):** `isShallowEqual::shallow_inequality_arrays_of_arrays`,
`isShallowEqual::shallow_inequality_objects_of_arrays`,
`tap::data_first_should_return_input_value`,
`tap::data_last_should_return_input_value`,
`constant::returns_identity_doesn_t_clone`, (likely) `mapWithFeedback::…same_accumulator…`.

**Root cause.** Typed arrays lower to Rust `Vec<T>` — a **value** type with no
identity. JS arrays/objects are **reference** types. So:
- `tap(DATA)` round-trips `DATA` through `SmeltUnknown` and rebuilds a fresh `Vec`;
  `toBe(DATA)` (reference, `same_js_key`) on two id-less `Vec`s collapses to a
  constant `false`.
- `isShallowEqual([a],[a])` must compare the nested `a` by reference, but a `Vec`
  stored in `[a]` is cloned (value copy) so its identity is lost; objects pass
  because `SmeltObject` is already `Rc`+id.
- `constant(obj)` must return the *same* object and reflect later mutation; a value
  clone breaks both.
- `mapWithFeedback` expects N references to one mutable accumulator.

**Approach.** Give typed arrays reference semantics + identity like objects:
represent `T[]` as an `Rc<RefCell<Vec<T>>>`-backed handle carrying a stable id
(mirror `SmeltObject`/`SmeltArray`). `===`/`toBe`/`same_js_key` then compare the
id; mutation is shared. This is the large change — it touches **every list
operation** (construction, index, push/pop, iterate, len, spread, sort, projection)
in the codegen, plus the MIR ownership opt passes (move-on-last-use assumes value
`Vec`s). Erased arrays already have `SmeltArray { id }`; the work is making the
*typed* path carry the same identity instead of bare `Vec`.

**Risk:** Highest — pervasive across list codegen; perf implications (`Rc`/`RefCell`
indirection on every typed array). Stage behind the compile-corpus + full report;
land incrementally per-operation only if each step keeps the suite green.

**Alternative (smaller, partial):** keep `Vec` but thread an id alongside it only
where reference identity is observed (`toBe`/`===`/`Object.is` on arrays). Brittle;
likely not worth it vs the full handle.

---

## Cluster B — Promise as a first-class inspectable value

**Tests (1, + the deferred isPromise family):** `isStrictEqual::test_built_ins_promises`.

**Root cause.** Promises lower to `Pin<Box<dyn Future>>`. When used as a *value*
(stored, compared, `instanceof`-checked) they erase to `SmeltUnknown::Null`, so all
promises compare equal. Futures aren't `Clone`, so `SmeltUnknown` can't hold one; a
marker-object erasure fixes identity/`instanceof` but breaks `await` (funnel) — see
memory `promise-marker-erasure-is-net-zero`.

**Approach.** A `SmeltUnknown::Promise` backed by `Rc<RefCell<PromiseState>>` that is
simultaneously **awaitable** (poll the shared state) and **inspectable-with-identity**
(`Rc` ptr / id for `===`/`instanceof`). Requires reworking the async lowering so
`Promise.resolve`/`new Promise`/`async fn` produce this shared handle rather than a
bare future, and `await` polls it. Medium-large; touches the async runtime (many
passing async tests at risk).

**Risk:** Medium-high (async is pervasive and currently working).

---

## Cluster C — Indexed data-last callbacks

**Tests (3):** `filter::data_last_filter_indexed`, `forEach::datalast_521`,
`reduce::data_first_indexed_1550`.

**Root cause (hypothesis — investigate first).** All three exercise the callback's
**index** argument in a `pipe`/data-last (or data-first indexed) form. Likely a
shared issue in how the index is threaded through the data-last adapter / lazy
batch (off-by-one, or the index not advancing, or the callback receiving the wrong
arg shape). `reduce` and `forEach` fail `toBe` (identity of returned value/accum);
`filter` fails `toStrictEqual` (wrong elements kept).

**First step:** dump the generated `filter`/`reduce` data-last adapter and compare
the index passed to the callback against the eager path. Fix the adapter; likely
one shared lowering site (`purry`/lazy index plumbing).

**Risk:** Low–medium once the shared cause is found; may be a single bounded fix
rather than a refactor (verify before classifying as "big").

---

## Cluster D — `sortBy` multi-criteria (prop + direction)

**Tests (2):** `sortBy::…using_pipe_and_desc`,
`sortBy::…by_weight_asc_then_color_desc`.

**Root cause (hypothesis).** `sortBy([prop("weight"), "asc"], [prop("color"), "desc"])`
passes **tuples of (accessor, direction)** as variadic sort rules. The lowering must
build a composite comparator that applies each `prop` accessor and direction in
order. Likely the `[accessor, "asc"|"desc"]` tuple rule or the multi-rule chaining
isn't lowered (single-rule sort works; multi-rule/desc doesn't).

**First step:** inspect generated `sortBy` for the rule-tuple handling; confirm
whether `prop(...)` accessors and the `"desc"` direction reach the comparator.

**Risk:** Medium — comparator construction from variadic (accessor, dir) tuples.

---

## Cluster E — Object merge (`mergeAll`, `mergeDeep`)

**Tests (2):** `mergeAll::merge_objects`, `mergeDeep::…weird_object_types_functions`.

**Root cause (hypothesis).** `mergeAll([{...},{...}])` folds objects into one; the
result `toStrictEqual` mismatches — likely key-ordering, last-write-wins semantics,
or the spread/assign accumulation over erased objects. `mergeDeep` additionally
recurses and handles functions/weird types. Both are erased-object accumulation.

**First step:** compare generated merged object vs expected (keys/values/order);
determine whether it's a packing bug (e.g. earlier "mergeAll guard fix was inert"
note) or structural-eq ordering.

**Risk:** Medium.

---

## Cluster F — `mapWithFeedback` mutable-accumulator aliasing

**Test (1):** `mapWithFeedback::…same_accumulator_on_every_iteration…array_length_references…`.

**Root cause.** The test mutates a single accumulator and expects the output array to
hold N references to that *same* mutable object. Needs shared mutable reference
identity — same underlying gap as Cluster A (objects already `Rc`-shared, so this may
already work for objects; if the accumulator is an **array**, it depends on Cluster A).

**First step:** determine whether the accumulator is an object (should work) or an
array (blocked on Cluster A). May fold into Cluster A.

---

## Suggested order

1. **C** (indexed callbacks) — possibly bounded, verify-first; cheapest potential win (3).
2. **D** (sortBy) and **E** (merge) — medium, self-contained per function (4).
3. **A** (array reference identity) — the big representation change; unblocks 5–6 incl. maybe F.
4. **B** (Promise value) — async-runtime rework; smallest count, highest blast radius.

Gate every step on `rust-test-report --full` + `cargo test` + `clippy`; regenerate
`third_party/remeda` first. Never leave `main` red.
