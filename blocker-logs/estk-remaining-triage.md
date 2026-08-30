# es-toolkit generated suite — remaining-failure triage

Baseline at the start of this pass: **909 passed / 150 failed**. Now: **963 passed / 96 failed**
(`main` at `37674f1` alone is 954/105).

Reproduce with:

```bash
cargo run --bin smelt -- rust-test-report \
  --build-manifest third_party/es-toolkit/Smelt.toml \
  --cargo-manifest third_party/es-toolkit/dist-smelt/Cargo.toml \
  --full --suppress-warnings --output blocker-logs/estk-current.md
```

(The probe checkout is not committed. Clone `toss/es-toolkit` at the pinned
ref into `third_party/es-toolkit` and copy `.github/compat/es-toolkit/Smelt.toml`
next to it.)

## Diagnostics prerequisite

Generated assertions used to throw a bare `expect(...).toBe(...) failed`, which
made 150 failures effectively unattributable. They now carry the assertion's
source text and `path:line:column`, so every row below names a real spec line.

## Families

Counts are failing tests at the 150 baseline.

| n | Family | Root cause | Status |
| ---: | --- | --- | --- |
| 5 | `pick` / `omit` | A same-named variadic sibling (compat's `omit(object, ...paths)`) lent its rest slot to the fixed-arity `omit(obj, keys: K[])`, packing the keys array into a list of lists. | **fixed** — call arity now vetoes a name-keyed rest |
| 5 | `mean` / `median` / `meanBy` / `medianBy` | `toEqual`/`toStrictEqual` emitted a plain `!=` on `f64`, so `NaN != NaN`. Only `toBe` used Object.is. | **fixed** |
| ~18 | array element reads | `arr[i]` lowers to an infallible read: a negative normalized index panics (`usize::try_from(...).expect("negative index out of bounds")`) and an out-of-range read substitutes `Default::default()` instead of `undefined`. Also `last()` returns `Some(default)` where its type says `Option<T>`. `at`, `last`, `head`, `maxBy`, `minBy`, `zip`, `zipWith`, `unzip`, `uniq`, `fill`, `compact`, `remove`, `dropWhile`, `dropRightWhile`. | **partly fixed** |
| 6 | `escape` / `unescape` / `escapeRegExp` | Indexing a module-level const record with a *dynamic* key const-folds to "absent": `htmlUnescapes[match] \|\| "'"` became `let _smelt_tmp_1: bool = false; if false { .. } else { "'" }`. The const collection is also lowered as a module-init local that is immediately dropped. | **fixed** (5 of 6; `escapeRegExp` is a different root) |
| ~10 | object property order | `SmeltObject::new`/`with_id` take a `HashMap` and **sort** the keys to build `order`, so every record-to-erased-object conversion loses JavaScript insertion order. `findKey`, `sortKeys`, `invert`, `toCamelCaseKeys`, `toSnakeCaseKeys`, `Object.keys` ordering. JS also orders integer-like keys first, ascending. | **fixed** (`findKey`; the other rows had separate roots, listed below) |
| ~7 | `this` binding | A plain function called as `object.method()` must see `object` as `this`. `SmeltErasedFunction` already carries an `object: Option<..>` receiver slot that call sites leave `None`. `ary`, `unary`, `spread`, `flow.call`, `memoize`, `throttle`. | queued |
| ~14 | `partial` / `partialRight` / `flow` / `flowRight` | Placeholders, `fn.length` on a partially applied function, `new par()` instanceof the target, curried arity. Probably several roots. | queued |
| ~34 | promise / timer | `allKeyed`, `attempt`, `attemptAsync`, `delay` abort, `withTimeout`, `retry` delays, `limitAsync`, `semaphore`, `reduceAsync`, `debounce`, `throttle`, plus 4 concurrency tests (`filterAsync`/`flatMapAsync`/`forEachAsync`/`mapAsync` all assert `maxRunning === 10`, i.e. real concurrent scheduling, not sequential awaits). | queued |
| ~15 | host predicates | `isBrowser`, `isNode`, `isBuffer`, `isSymbol`, `isFunction`, `isFile`, `isError` on a subclass, `isPlainObject`, `isJSONValue`, `isJSON` (panics on invalid JSON instead of returning false), `isLength`, `isNull`/`isUndefined` type-predicate filters. Several are environment-presence questions rather than lowering defects. | queued |
| 6 | `isEqualWith` | THREE roots, not one -- see the correction below. 2 rows were the erasure boundary copying an array's buffer (circular refs); 3 are `globalThis[name]`/`Buffer` folding to `Undefined` (the host-globals family); 1 is a named property store onto an erased array replacing the array with an object. The `primitives` row was fixed earlier by the mixed-nullish array-literal fix. | **2 of 6 fixed** (shared-buffer erasure); 4 reassigned to other families |
| 8 | `clone` / `cloneDeep` | **This row and the two `clone`/`cloneDeep` sections below are superseded — see `blocker-logs/estk-clone-cluster.md`.** Measured, the cluster is EIGHT independent roots, not one; the dynamic-prototype path already works and gated none of them; three of the eight are not cloning defects at all. | **3 of 8 fixed**; the other five are named with their real roots in that log |

### `flow` / `flowRight` / `partial` / `partialRight`: a property assigned onto a function is dropped

Six of the remaining failures across these four modules are the rows named
"curried function" or "placeholders", and they share one root with `curry`.

`curry.ts` attaches a symbol to a FUNCTION value three times --
`curry.placeholder = curryPlaceholder` at module scope (onto a function
declaration) and `wrapper.placeholder = curryPlaceholder` twice (onto a local
function expression) -- then reads it back as `item === curry.placeholder`.

The assignments are dropped. Generated `curry_1967` (the module init) mints the
symbol and immediately discards it:

```rust
pub(crate) fn curry_1967() -> () {
    let curry_placeholder: SmeltUnknown = SmeltUnknown::Symbol("Symbol(curry.placeholder)@10319".to_owned());
    return;
}
```

so the read resolves to `null` and the filter predicate compares against the
wrong value entirely:

```rust
// source: partialArgs.filter(item => item === curry.placeholder)
let _smelt_tmp_4: bool = closure_arg_0.clone().js_strict_eq(&SmeltUnknown::Null.clone());
```

`holders` therefore counts `null`s -- always zero for real arguments -- so
`length` never accounts for a placeholder and the curried call applies its
arguments in the wrong positions.

The representation already exists: erased callable objects carry a
`__smelt_call` slot alongside ordinary fields, which is how `.call` on a
callable object resolves. The missing piece is the assignment path (a static
property store onto a function declaration or function expression) and the
matching read, so the stored field is found instead of answering `null`.

Worth doing before the `toBe` work below: it is a single root behind six
failures in four modules, and probably more once `curry` itself behaves.

### SUPERSEDED: `clone` / `cloneDeep` is THREE roots, not one

> **Superseded by `blocker-logs/estk-clone-cluster.md`.** The three-root split below
> was measured before the `toBe`-identity work landed and is now wrong in its own
> turn: group A is fixed for `Map` (only the `String` object row remains, and its root
> is that `new String(x)` lowers to a primitive, not the `toBe` emitter), group B is
> four unrelated roots rather than "cloning fidelity", and group C is
> `Object.defineProperties` being unmodeled. Kept for the history.


The entry below claimed the root is `toBe` rather than `clone`. That is at
best a third of the story, and I only found out by running the tests and
reading the real assertion messages instead of reasoning from the source.
The ten failures split three ways:

**A. `toBe` emits structural equality for TYPED reference types (3 tests)**
-- `clone should clone maps`, `cloneDeep should clone maps`,
`cloneDeep should clone string objects`. The frontend is already correct:
`test_to_be_identity_type` classifies `List`/`Dict`/`Set`/`Tuple`/`Class`/
`Function` as identity types and routes `toBe` to `BinOp::StrictEq`. The
EMITTER then renders that as Rust's derived `PartialEq`:

```rust
// source: expect(clonedMap).not.toBe(map)
_smelt_tmp_5 = cloned_map != map;
```

so a clone with equal contents compares equal and `not.toBe` fails. The
erased path is already right (`js_strict_eq` is documented as reference
identity for objects); only the typed path is wrong. Identity is available
and unused -- `SmeltRecord` and `SmeltList` both carry an `id`. This is a
correctness bug well beyond `clone`: it affects every `toBe` on a typed
object or array.

**B. `clone` loses host-object content (6 tests)** -- `clone regular
expressions`, `clone error`, `clone custom error`, `clone custom classes`,
`cloneDeep clone instance`, `cloneDeep clone regexp arrays`. These fail on
`toEqual`, not `toBe`, so the clone is a distinct object whose CONTENT does
not survive: `expect(clonedRegex).toEqual(regex)` and
`expect(clonedError).toEqual(error)` both report failure. Cloning fidelity
for RegExp / Error / class instances is its own job, unrelated to A.

**C. One remaining row** -- `cloneDeep should clone read only properties`
fails `expect(b['#b']).toBe(undefined)`, which is neither of the above.

So A and B are independent and can land separately; A is the general one.

### Superseded original note: the root is `toBe`, not `clone`

`clone` itself is correct for the Map case -- a runtime probe on the generated
crate shows `clone(map)` returning a FRESH identity (object id 1 -> 6) with its
`[object Map]` tag intact, built through the reflected
`Object.getPrototypeOf(obj).constructor` path, which already works.

The failure is in the matcher. `toBe` (reference identity) and `toEqual`
(structural) compile to the SAME operation for a `Map` local:

```rust
_smelt_tmp_4 = cloned_map.clone() != map.clone();   // toEqual  -- structural, correct
_smelt_tmp_5 = cloned_map != map;                   // not.toBe -- same op, should be identity
```

so `expect(clonedMap).not.toBe(map)` cannot distinguish a fresh Map from its
original. Two changes are needed together:

1. `test_to_be_identity_type` (`lowering/testing/matchers.rs`) lists `List`,
   `Dict`, `Set`, `Tuple`, `Class` and `Function` but NOT `JsMap`, so a `Map`
   never reaches the strict-identity path. `Type::JsMap` exists only to preserve
   the source `Map` spelling through interning, and it is a reference exactly
   like the `Dict` it shares machinery with -- this is an oversight.
2. Even with (1), codegen emits structural `!=`: two CONCRETE reference operands
   fall past `binary_ops.rs`'s erased/nullish arms to the generic emitter. An
   id comparison exists (`left.id == right.id`) but only for `Optional`
   operands whose inner type is a string-keyed `Dict`. `SmeltJsMap` carries an
   `id`, so the fix is a concrete strict-equality arm for reference types.

Change (1) alone is a no-op (measured: 950/109 either way) and was reverted
rather than left in unverified; both halves must land together, with a full
three-corpus sweep since it changes `toBe` for every reference type.
| 3 | `memoize` | Custom/immutable cache implementations panic with `missing field`. The unary/resolver/`this`-context rows are fixed: `fn.call(this, arg)` was passing the `this` operand as a positional argument, so the callee's first parameter bound to it. Same root fixed `overArgs` and `result` outright and reduced `flow`/`partial`. | queued |

Latent, unexercised by any corpus: the closure-body `.call` path
(`callback_call_method_to_body_expr`) has the same `this`-as-argument defect.
Reverting a speculative fix there changed no corpus result, so it is left alone
rather than shipped unverified.


## Roots isolated but not yet fixed

Each of these was proven by reproduction while fixing something adjacent, so
they are specified work rather than guesses.

- **`Array(n)` holes** — was the real root of `fill`/`zip`/`zipWith`/`unzip`/
  `last`-large, not element reads. **Fixed**; `fill` and `last` now pass,
  `zip`/`zipWith`/`unzip` still fail on the next item.
- **A concrete-list read into an erased slot loses fallibility.** An element
  read keeps its `Option` when the target is `Option<..>` but not when the
  target is `SmeltUnknown`: `b[i]` on a `string[]` misses to `String::new()`
  and erases as `''` where JavaScript wants `undefined`. This is what still
  fails `zip`/`zipWith`/`unzip`.
- **`Array.prototype.sort` with an absent comparator** does not fall back to
  the default ToString comparator; the wrapped optional callback returns
  `Undefined` → NaN → `Equal`, so nothing sorts. Also
  `String.prototype.localeCompare` returns nothing usable (NaN).
- **`Array.isArray(value)` on an `unknown` operand const-folds to `false`**,
  so the array branch of `toCamelCaseKeys`/`toSnakeCaseKeys` never runs.
- **An arrow-const predicate loses `typeof` narrowing.**
  `(value: number|string) => typeof value === 'string'` lowers its body to the
  constant `true`; the same expression in a named `function` lowers correctly.
  Root of `omitBy`/`pickBy`.
- **`Object.keys` on a `SmeltJsMap` does not filter inherited keys**, so
  `Object.create({a:1})`'s `__smelt_proto` entry leaks in as an own key. Root
  of `invert`.
- **`Record<string, V>` lowers to a plain `HashMap`** whenever the program
  contains no erased runtime (`dict_uses_smelt_record` requires
  `needs_unknown_type`), losing key order entirely for such programs.
- **`escapeRegExp`** needs JS→Rust character-class translation
  (`/[\^$.*+?()[\]{}|]/` does not compile under `fancy-regex`) and `$&`
  replacement-pattern expansion in the prelude's `replace_string`.
- **Callback `table[key] ||` models key presence, not JS value-truthiness**, so
  a falsy stored value takes the wrong branch.

## Two false passes removed

Both were passing for the wrong reason and now fail honestly:

- `sortKeys` "sorts alphabetically by default" — passed only because every
  erased object came out alphabetical.
- `initial` "all elements except the last for a large array" — both
  `Array(1000)...` and `Array(999)...` were empty, so the assertion compared
  `[]` to `[]`.

## RESOLVED: `throw new Error(...)` discards the Error object

**This root is fixed on `main` as of `cb5cf1b`** — it was still open when the
note below was written. `src/chunk.ts:26` now emits

```rust
smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record(SmeltRecord::from([
    ("__smelt_error".to_owned(), SmeltUnknown::String("Error".to_owned())),
    ("message".to_owned(), SmeltUnknown::String("Size must be an integer greater than zero.".to_owned())),
]))))
```

so thrown values do carry the error object. The 3400-odd remaining
`smelt_throw(SmeltUnknown::String` sites are the TEST HARNESS's own assertion
failures, not library `throw`s — the original `grep -c` did not separate the
two, which is what made the count look total. Anything still failing in the
promise/util/error areas needs re-triaging against a fresh run rather than
being attributed here.

The original note follows.

### Superseded: `throw new Error(...)` discards the Error object (largest single remaining root)

Found by reading generated code, not yet fixed. Every `throw` in the generated
crate collapses its operand to a bare message string:

```rust
// src/array/chunk.ts:26 — throw new Error('Size must be an integer greater than zero.');
return Err(smelt_throw(SmeltUnknown::String("Size must be an integer greater than zero.".to_owned())));
```

`grep -c 'smelt_throw(SmeltUnknown::Object'` over the whole generated crate
returns **zero matches** — no thrown value anywhere carries an error object,
even though Smelt models one elsewhere (`{ __smelt_error: "Error", message: ..,
cause: .. }`, which the `new Error(msg, { cause })` work already produces and
reads back). So the loss is in the `throw` path specifically, not in Error
construction.

Everything downstream of a thrown error is therefore wrong:
`error instanceof Error` is false, `error.message` is `undefined`, and
`.rejects.toThrow('msg')` cannot match. This is not confined to promises — it
is every `throw` in the library — but the promise specs are where it shows up
most.

Failing tests that plausibly depend on it (~10-14): all six
`expect(...).rejects.toThrow(...)` rows, `attemptAsync` ×2 (`expect(error
instanceof Error && error.message)`), `attempt` `toEqual([new Error('test'),
null])`, `limitAsync` "propagates callback errors", and the three `retry` rows
that surface as an uncaught `SmeltThrown` carrying `{__smelt_error, message}`.

This is the best-value next family: one root, crisply located, and it unblocks
assertions across the promise, util and error areas at once.

## Three more roots, split out of the `zip`/`unzip`/`at` cluster

The erased-element-read fix moved only `zipWith` of the six failures it was
aimed at. Investigating the rest split them into three genuinely distinct
roots — none of them a read-coercion bug:

- **`zip` — the two `undefined`-to-string directions disagree.** `zip_160`
  already emits `unwrap_or(SmeltUnknown::Undefined)`, so the read is right. The
  spec lowers both sides to `(f64, String)` tuples; extracting the actual value
  maps `SmeltUnknown::Undefined => "undefined"`, while the expected literal
  `[3, undefined]` lowers to `(3.0, String::new())`. JS says
  `String(undefined) === "undefined"`, so the constant coercion and the erased
  extraction must agree — or the tuple should never have been typed
  `(f64, String)`.
- **`unzip` — a nested lvalue store never reaches its target.**
  `result[i][j] = zipped[j][i]` lowers as `_tmp = result.get(i).cloned()`
  followed by a write into `_tmp`. `SmeltList::clone` deep-copies, so the store
  is lost. Confirmed minimally: `grid[0][0] = 'a'` then reading it back gives
  `undefined`. This is a place-projection bug and likely affects any nested
  index assignment.
- **`at` — a typed list cannot hold a hole.** `at<T>` writes `arr[index]` into
  `result: SmeltList<T>`, so a miss stores `Default::default()`. There is no
  `undefined` to put in a `Vec<T>`. This is the same storage question `Array(n)`
  raised, and it now has two callers wanting an answer.

### `allKeyed`: erased `SmeltUnknown::Promise` values in a record (NARROWED, NOT YET VERIFIED)

Four of `allKeyed`'s seven tests fail, and the pass/fail split is a clean
discriminator -- every FAILING test builds its object with `Promise.resolve`
or `Promise.reject`; every PASSING test uses either no promise at all
(plain values, empty object) or the `new Promise(resolve => setTimeout(...))`
constructor.

Note which test passes: "should resolve promises concurrently, not
sequentially", which times two 50ms sleeps and asserts `elapsed < 90`. So
concurrency is NOT the defect. The `Promise.all` list path does await its
futures in a `for` loop rather than joining them, which looks sequential at
a glance, but `from_future_primed` runs each async body's synchronous prefix
at creation and that timing test passes -- do not chase this as a
concurrency bug (I did, briefly; it is not one).

The failing shape is value-flattening. `Promise.resolve(1)` in an object
literal lowers to `SmeltUnknown::Promise(...)` stored in the record:

```rust
let _smelt_tmp_4: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([
    ("a".to_owned(), { let smelt_future = _smelt_tmp_1; SmeltUnknown::Promise(SmeltPromise::from_future(...
```

`all_keyed` then receives an erased object whose values are themselves
promises, takes the `item_is_erased` branch of `AsyncOp::All` in
`crates/smelt-codegen-rust/src/emitter/call.rs`, and is supposed to unwrap
each with `smelt_await_flatten`. The tests that need that unwrap are exactly
the ones failing, so the defect is somewhere in that chain -- the
`SmeltUnknown::Promise` -> `SmeltFuture<SmeltUnknown>` conversion, the single
flatten level, or the `result[keys[i]] = values[i]` write-back.

NOT yet confirmed by running: narrowing this further needs the es-toolkit
generated suite, and the exact assertion diff should be read before changing
any emitter code.

### CORRECTION: `isEqualWith` is not the wrapper, and it is three roots

The note below is **WRONG**, and its "decisive fact" is a fallacy. Verified by
running the suite and reading the generated Rust:

`src/predicate/isEqualWith.spec.ts` imports `./isEqualWith` — the PLAIN one. It
never touches `src/compat/predicate/isEqualWith.ts`, so no failure in it can be
"the compat wrapper". And "`isEqual` has ZERO failures" proves nothing about the
engine on these shapes: `isEqual.spec.ts` has no circular-reference, array-view,
error-object or non-index-property case at all. It tests array BUFFERS only. An
untested shape passing is not evidence.

The six split three ways:

**A. Circular arrays (2 tests) — FIXED.** The erasure boundary
`From<SmeltList<SmeltUnknown>> for SmeltArray` copied the buffer while keeping
the `id`, so an array put into an `unknown[]` slot was a stale snapshot wearing
the live array's identity. `array1[0] = array1` therefore stored a copy of
`array1` as it was one statement earlier. The cycle guard in `areObjectsEqual`
(`Object.is(a, b)`, `stack.set(a, b)`) is correct and the `SmeltJsMap` cycle
stack is correct — the erased element simply could never BE the array. Now
`with_storage(list.id(), list.storage())`; see
`blocker-logs/smeltlist-shared-buffer.md`. es-toolkit 961/98 -> 963/96.

**B. `globalThis[name]` folds to `Undefined` (2 tests, plus most of the ~15
host-predicate family).** `should compare array views` and `should compare error
objects` both build their fixtures with `globalThis[type]` and `new CtorA('a')`.
`globalThis` lowers to `SmeltRecord::from([])` — an EMPTY record — so the
generated code is literally

```rust
let _smelt_tmp_10: SmeltUnknown = SmeltUnknown::Undefined;
let ctor_a: SmeltUnknown = _smelt_tmp_10.clone();
```

and every `new CtorA(..)` answers `SmeltUnknown::Null`. All four members of each
pair are then `null`, so every comparison is `true` and `actual` is
`[[true, true, true], ..]` against `[[true, false, false], ..]`. Note also that
`typeof globalThis.Buffer !== 'undefined'` const-folds to `true` on that empty
record (`isBuffer_1.rs:14`, `let smelt_logical: bool = true;`), which is wrong in
the opposite direction. This is the host-globals job, not an `isEqualWith` bug.

**C. `Buffer` is not modelled (1 test).** `should compare buffers` passes its
first two assertions and fails only
`isEqualWith(buffer, new Uint8Array([1]), noop)` — which must be `false`. Both
values carry the `[object Uint8Array]` tag, so the discriminator is
`isBuffer(a) !== isBuffer(b)`; `is_buffer_462` always returns `false` (same empty
`globalThis`), so the two look identical. Same family as B.

**D. A named property store onto an erased array DESTROYS the array (1 test).**
`should treat arrays with identical values but different non-index properties as
equal`. `array1.every = null` on a `SmeltUnknown::Array` emits

```rust
match &mut array1 {
    SmeltUnknown::Object(map) => { map.insert("every".to_owned(), SmeltUnknown::Null); },
    other => { *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([("every".to_owned(), SmeltUnknown::Null)]))); }
}
```

(`emitter/control_flow.rs:528`/`:532`, and the same fallback in
`smelt_index_assign`'s non-index arm). The array `[1, 2, 3]` becomes the object
`{ every: null }`, and the sibling becomes `{ concat: null }`, so the comparison
sees two one-key objects with different keys. `SmeltArray` has no property bag,
so a correct fix means giving it one — a JS array IS an object with index
handling — which touches `new`/`with_id`/`with_storage`/`Clone`/`PartialEq`/
`Hash`, `Object.keys`, `hasOwn`, spread and JSON. Specified, not attempted.

Second defect visible in the same emission: the chained assignment
`array1.every = array1.filter = ... = null` emits ONLY the first store; the other
eight targets are dropped. Harmless here (the comparison ignores non-index
properties) but general.

The superseded note follows.

#### Superseded: the six remaining failures are the wrapper, not the engine (NARROWED, NOT YET VERIFIED)

All six remaining `isEqualWith` failures are named "when customizer returns
undefined" -- the fallback path -- and they cover array views, buffers, error
objects, circular arrays, transitive circular arrays, and arrays with
differing non-index properties.

The decisive fact: `isEqual` has ZERO failures. `src/predicate/isEqual.ts` is
literally `isEqualWith(a, b, noop)`, so the shared engine
(`is_equal_with_529`, from `src/predicate/isEqualWith.ts`) already handles
every one of those six shapes correctly. Whatever is broken is in the compat
wrapper `src/compat/predicate/isEqualWith.ts`, not the comparison itself.

Two things I ruled OUT by reading the generated Rust, so nobody re-checks
them:

- The customizer's "no opinion" is NOT collapsed to `false`. The engine takes
  `Option<bool>`, and the wrapper closure's fall-off-end correctly yields
  `Ok::<Option<bool>, _>(None::<bool>)`.
- The wrapper does reach the same engine; both paths call
  `is_equal_with_529`.

So the difference is confined to what the wrapper's closure does that `noop`
does not: call the user customizer, then test `a instanceof Map` and
`a instanceof Set` (closing over the OUTER `a`/`b`, which is the real source
semantics), then recurse through `Array.from` + `after(2, ...)`. A plausible
shape is those `instanceof` probes misbehaving on host objects (Buffer,
Error, TypedArray) and diverting into the `Array.from` path -- but that would
not by itself explain the two circular-array rows, so it is not the whole
story.

NOT verified by running. The next step is the actual assertion diff for one
host-object row and one circular row; do not change the engine, which the
`isEqual` result shows is already correct.

### `sort()` with an absent comparator is a NO-OP (CONFIRMED BY CONSTRUCTION)

`Array.prototype.sort` with no comparator, or with an `undefined` one, must
use the JS default: coerce each element to a string and compare
lexicographically. Smelt instead makes every pair compare EQUAL, so the list
comes back unchanged.

The whole chain is visible in the generated output and the prelude, in
`third_party/es-toolkit/dist-smelt/src/sortKeys.rs` for
`Object.keys(object).sort(compareKeys)`:

```rust
// absent comparator collapses to Undefined ...
compare_keys.map(|f| SmeltUnknown::Number((f)(a, b) as f64)).unwrap_or(SmeltUnknown::Undefined)
// ... which smelt_into_f64 turns into 0.0 (the `_ => 0.0` arm) ...
sort_by(|left, right| {
    let ordering = (smelt_comparator)(left, right).smelt_into_f64();
    if ordering < 0.0 { Less } else if ordering > 0.0 { Greater } else { Equal }
})
```

`0.0` is neither `< 0.0` nor `> 0.0`, so every pair is `Equal`, and Rust's
`sort_by` is stable -- the input order survives untouched.

This is GENERAL, not a sortKeys quirk: it hits every `.sort()` with no
argument or an undefined one, anywhere in any corpus. `sortKeys` just makes
it obvious because all three of its tests fail, including "should sort
object keys alphabetically by default".

The fix is a real default comparator (string coercion + lexicographic
compare) selected when no callable comparator is supplied -- not a tweak to
the `0.0` arm of `smelt_into_f64`, which is correct for other callers.

Ruled out while finding this: `SmeltRecord` DOES model insertion order (it
carries an `order: Vec<K>` beside its `HashMap`), so the rebuilt object is
not the problem.

One loose end: `sortKeys`'s third failure passes a REAL compare function,
which should take the `Some` branch and work. That one is not explained by
this defect and needs its own look after this is fixed.

#### Follow-up: the third `sortKeys` failure is `localeCompare`, a separate root

The loose end noted above is closed. `should sort keys with a custom compare
function` passes a REAL comparator, so it takes the `Some` branch and is not
affected by the no-comparator defect. It fails for its own reason: the
comparator body is `(a, b) => b.localeCompare(a)`, and
`String.prototype.localeCompare` is not a modeled builtin. The member read
resolves to `SmeltUnknown::Null`, which is then CALLED:

```rust
let smelt_source_value = SmeltUnknown::Null.clone();          // b.localeCompare
let smelt_function = match smelt_source_value { SmeltUnknown::Function(f) => Some(f), ... };
let _smelt_tmp_3: SmeltUnknown = (_smelt_tmp_2)(closure_arg_0.clone());
```

So `sortKeys` needs BOTH fixes to go green: the default comparator, and
`localeCompare`. For the ASCII cases the corpus exercises, `localeCompare` is
an ordinary lexicographic comparison returning a negative / zero / positive
number, so modeling it is small — and it is a plain missing builtin, not a
design problem.

Note the shared shape with the function-property defect recorded above: an
UNMODELED MEMBER SILENTLY BECOMES A VALUE (`Null` here, a bogus `.fieldN`
there) instead of being diagnosed, and the wrong value then flows into a
call or a comparison. The in-flight function-statics work adds a diagnostic
for the function-receiver case; the same treatment for unmodeled string
methods would have surfaced this immediately rather than at runtime.


## `Function.prototype.apply` had no lowering arm (FIXED)

Found by reading generated code for `flow`, then reduced to a two-line repro.
`fn.apply(thisArg, argsArray)` fell through the static-member dispatch to the
field-read path, resolved `apply` as an ABSENT member, and lowered to a bare
`SmeltUnknown::Null` — silently, with no diagnostic. `flow`'s generated body:

```rust
let _smelt_tmp_4: f64 = funcs.len() as f64;
let _smelt_tmp_5: bool = _smelt_tmp_4.clone() != 0.0;
if _smelt_tmp_5.clone() {
    _smelt_tmp_6 = SmeltUnknown::Null;   // <-- funcs[0].apply(this, args)
    result = _smelt_tmp_6.clone();
```

`.call` in the loop directly below it lowered correctly, which is what made the
failure look like a composition bug rather than a missing member.

This is the third independent instance of the same meta-defect recorded in this
log: **an unmodeled member silently becomes a value** instead of being
diagnosed (the others were function props and `localeCompare`). Worth a
systematic sweep rather than another one-off.

Fixed by an `apply` arm mirroring the existing `call` arm, spreading the
trailing array through `ClosureCallSpread`. Moved 3 tests (105 -> 102):
`flow`/`flowRight` "should supply each function with the return value of the
previous" and `throttle` "should call the function with correct arguments".
Also removed a latent miscompile: `.apply` on a callable object previously
emitted a direct `__smelt_call` struct-field read that computed the wrong value
at runtime, and did not compile at all when the interface lowered to a newtype.

### CORRECTION: the cluster was never about placeholders

The placeholder machinery was already working. `curry.placeholder` lowers
correctly to `SmeltUnknown::Symbol("Symbol(curry.placeholder)@10319")` and the
`filter(item => item === curry.placeholder)` predicate compares against it —
verified by reading the regenerated `dist-smelt/src/curry.rs`. The earlier claim
in this log that the assignments were dropped and the read resolved to `null`
was **stale**: that was fixed before this pass.

The real second root was **an overloaded callable interface dropping its call
arguments**. The generated struct carries ONE `__smelt_call` slot, typed from
the FIRST call signature. `CurriedFunction2` declares four signatures of
differing arity, so every call site adapted to the zero-argument one:

```rust
_smelt_tmp_8 = { let smelt_callback = curried.__smelt_call…;
  Rc::new(move |arg0, arg1| { let v = (smelt_callback)(); /* args 2 and 3 discarded */ … }) };
```

Fixed by collapsing a differing-arity overload set to one erased variadic slot —
which overload runs is decided by the runtime argument list, a genuine dynamic
boundary — exactly as `ty/annotations.rs` already collapses a differing-arity
UNION. Uniform-arity interfaces keep their concrete slot.

### Still open in the `flow`/`partial` cluster (5 tests, three further roots)

- **Overload selection ignores argument TYPES** — blocks `partial`/`partialRight`
  "should work with curried functions" and `flow`/`flowRight` "curried functions
  with placeholders". `signature_accepts_arg_count` picks the first overload
  matching the ARITY, so `curried(2, 3)` selects the PLACEHOLDER overload
  `(t1: __, t2: T2)` instead of `(t1: T1, t2: T2): R`; TypeScript rejects the
  first because `2` is not the placeholder's unique symbol. The assertion then
  const-folds to `_smelt_tmp_10 = !(false);`. Needs argument types threaded into
  `function_member_type_for_arg_count` / `interface_call_signature_type` plus an
  assignability test.
- **Erased -> fixed-arity -> erased round trip loses arity** — blocks
  `partialRight` "supports placeholders". The TS overload declares a 2-parameter
  result, so the erased variadic is adapted down to a 2-arg `Rc<dyn Fn>` and
  immediately re-erased; `par('a','b','d')` loses its third argument. The
  narrowing adapter should not be inserted when both ends are the dynamic
  boundary.
- **`fn.length`** (2) and **`new par() instanceof Foo`** (2) remain as recorded.

### Separately confirmed: a `__smelt_call` slot read as a struct FIELD is called without an adapter

Pre-existing and unrelated to the `apply` work (verified by stashing: identical
output before and after). `let t = c.__smelt_call.clone(); (t)(2.0, 3.0)` calls a
`SmeltErasedFunction` with call syntax — E0618. No corpus reaches it.

### Separately confirmed: callable-object construction via a function-static assign

Reduced while building an `.apply` repro, and PRE-EXISTING on `main` (verified
with the change stashed). This source:

```ts
interface Callable { (x: number, y: number): number; tag(): string; }
const fn = ((x: number, y: number) => x + y) as Callable;
fn.tag = () => 'tag';
return fn;
```

lowers `Callable` to a NEWTYPE (`available field is: 0`) while the call site
still emits `c.__smelt_call`, so it fails to compile with E0609 — even for a
plain direct call, before `.apply` is involved. Building the same value with
`Object.assign` instead lowers correctly and passes. So the defect is in the
`as`-cast + function-static-assign construction path, not in the callable
interface itself.

## `__proto__` is a READ defect, not a store defect (FIXED)

The recorded diagnosis — "a `__proto__` write on a plain object creates an
ordinary own property instead of swapping the prototype, so merge copies it" —
is **wrong**, in the half that named the write. The spec's source is
`Object.create(null)`, and a null-prototype value does not inherit the
`__proto__` accessor from `Object.prototype`, so in real JavaScript
`source.__proto__ = {...}` there *does* store an ordinary own property. Node
agrees: `Object.keys(source)` is `["__proto__", "a"]`. The generated store was
already correct, and the first assertion (`expect(result).toEqual({ a: 2 })`)
already passed — es-toolkit's `isUnsafeProperty` guard was observable all along.

The only failing assertion was the READ, `expect(result.__proto__).toBe(
Object.prototype)`. `x.__proto__` lowered as an ordinary field read
(`smelt_get_object_field(&map, "__proto__")`), so on `result`, which has no own
`__proto__` slot, it answered `Undefined`. `__proto__` is an accessor inherited
from `Object.prototype` whose getter is `Object.getPrototypeOf(Object(this))`.

Fix: the read shares HIR with `Object.getPrototypeOf` — `PrototypeSentinel`
gained `own_slot_shadows`, set only for the `__proto__` spelling, which emits
`smelt_proto_accessor` (own slot first, then the sentinel) instead of
`smelt_prototype_sentinel`. `Object.getPrototypeOf` stays blind to own
properties as specified. Writes keep their existing store lowering, which is
what makes the null-prototype own slot exist. Guarded by
`crates/smelt-codegen-rust/tests/proto_accessor_runtime.rs`.

## `debounce` / `throttle` edge options — ONE root, not two (FIXED)

`throttle ... trailing edge only` and `debounce ... leading edge` share a single
root, and neither involves the virtual clock: both failed on their FIRST,
synchronous assertion, before any `delay`. They are mirror images of the same
dropped option — with `edges` ignored, throttle's default fires the leading edge
it was told to suppress, and debounce's default never fires the leading edge it
was told to use.

Generated evidence, `debounce_spec.rs`:

```rust
DebounceOptions { signal: smelt_record_map.get("signal")..., edges: None }
```

`edges: None` — hard-coded, while the record plainly holds an `"edges"` entry.
The record-to-interface projection asks whether the record's value type can
render as the optional field's INNER type; for a literal in which every property
is optional the value type is already `Option<T>`, which fails that question, so
the field was written as `None`. `can_render_non_function_dict_value_as` also
returned early on an optional target, so the identical-`Option<T>` case never
reached its own `source == target` check. Both are fixed in
`emitter/core.rs::record_projection`; guarded by
`crates/smelt-codegen-rust/tests/optional_record_field_runtime.rs`, which fails
on the pre-fix compiler (verified by reverting only that emitter change).

## `debounce should not add multiple abort event listeners` — NOT a shared root

Third of the three "timer/mock" rows, and unrelated to the two above. Generated
`debounce_spec.rs`:

```rust
let add_event_listener_spy: SmeltUnknown = SmeltUnknown::Null;
```

`vi.spyOn(signal, 'addEventListener')` lowers to an inert placeholder — by
design: `lowering/stdlib.rs::vitest_spy_on_call` discards its target and returns
`vitest_mock_handle_expr`, recording nothing except for the special
`Date.getTimezoneOffset` case. The spec then reads
`addEventListenerSpy.mock.calls`, which is `Null`.

Making it pass is a feature, not a fix, and a two-part one: (1) a real `vi.spyOn`
that replaces a live method with a recording wrapper and restores it on
`mockRestore`, and (2) a host `AbortSignal` whose `addEventListener` is an
interceptable member at all — today registration is the runtime manipulating the
`__smelt_abort_listeners` array on the marker record, so there is no property for
a spy to wrap. Part 2 touches the host-representation seam, which regates all
three corpora. Not attempted; reported as scope, not rejected.
