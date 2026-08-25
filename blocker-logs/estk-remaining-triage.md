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
| 8 | `isEqualWith` | One spec; customizer-returns-undefined fallbacks over typed-array views, buffers, errors, sparse and circular arrays. | queued |
| ~10 | `clone` / `cloneDeep` | Map, RegExp, Error, class instances, `String` objects. Overlaps the dynamic-prototype work already noted in the compat manifest. | queued |
| 6 | `memoize` | Custom/immutable cache implementations panic with `missing field`. | queued |


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
