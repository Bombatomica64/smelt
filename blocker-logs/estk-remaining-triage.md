# es-toolkit generated suite — remaining-failure triage

Baseline at the start of this pass: **909 passed / 150 failed**. Now: **928 passed / 131 failed** (matches
`blocker-logs/es-toolkit-ci-baseline.json`'s pinned ref `e008a2818cd8`).

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
| 6 | `isEqualWith` | One spec; customizer-returns-undefined fallbacks over typed-array views, buffers, errors, and circular arrays. The `primitives` row is fixed: its `Object(...)`-boxed pairs already compared correctly (the tag probe knows the wrapper markers), and the real defect was the pair table's `[null, undefined, false]` rows lowering identically to `[null, null, true]` -- see the mixed-nullish array-literal fix. | queued |
| ~10 | `clone` / `cloneDeep` | Map, RegExp, Error, class instances, `String` objects. Overlaps the dynamic-prototype work already noted in the compat manifest. | queued, root identified |

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

### `clone` / `cloneDeep`: the root is `toBe`, not `clone`

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

## `throw new Error(...)` discards the Error object (largest single remaining root)

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

### `isEqualWith`: the six remaining failures are the wrapper, not the engine (NARROWED, NOT YET VERIFIED)

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
