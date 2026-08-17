# es-toolkit: clone / isEqualWith campaign

Measured against es-toolkit at the ref pinned in `.github/compat/libraries.json`
(`e008a281`), with the fixture manifest `.github/compat/es-toolkit/Smelt.toml`.
Starting point was Smelt `64dc39d` (post PR #190).

Scope of the session: the four largest failing groups — `isEqualWith` (23),
`cloneDeep` (17), `clone` (15), `cloneDeepWith` (6) — 61 failures.

## Result

| | Passed | Failed | Probe blockers |
| --- | ---: | ---: | ---: |
| Before | 823 | 236 | 0 |
| After | 850 | 209 | 0 |

**+27 passing, 0 newly failing.** Per target group:

| Group | Before | After |
| --- | ---: | ---: |
| `cloneDeepWith` | 6 | **0** |
| `isEqualWith` | 23 | 16 |
| `cloneDeep` | 17 | 12 |
| `clone` | 15 | 10 |
| `toMerged` (not targeted) | 10 | 8 |

`SmeltUnknown` ratchet: avoidable erasure 35670 → 35621. The baseline was
re-snapshotted twice, each with its delta explained in the commit that moved it.

## Fixes landed

Each is a general rule with a regression test; none is keyed on a library
function name.

### 1. `Object.create(proto)` returned its argument instead of a fresh object

`Object.getPrototypeOf` on an erased value yields an opaque `"__smelt_proto:*"`
**string** sentinel, so the standard clone idiom
`Object.assign(Object.create(Object.getPrototypeOf(obj)), obj)` assigned fields
onto a string: every copied key was dropped, and `Object.keys` then enumerated
the sentinel's twenty character indices `"0".."19"`. Handed a concrete prototype
object instead, the result *aliased* that prototype, so the following
`Object.assign` mutated the prototype.

New `ObjectFromPrototype` HIR/MIR node → `smelt_object_from_prototype`, which
always allocates. Two follow-ons the fresh-object model needs:
`__smelt_proto:`-prefixed entries are filtered out of own-key enumeration (they
are inherited, not own), and `smelt_get_object_field` falls back to the
`__smelt_proto:<field>` slot so inherited members are readable with own fields
shadowing them. Those slots were written by the `Object.create({ ... })` literal
lowering and never read before.

**+10 passing.**

### 2. `Object(value)` did not box, and `.valueOf()` always answered `null`

`Object(1)` called as a function was unrecognized, so it produced a null-ish
value tagged `[object Null]`; only `new Number(1)` built the
`{ __smelt_number: true, value: 1 }` wrapper. Separately,
`Object.prototype.valueOf` exists on every value but is an own property of none,
so the erased own-field read always missed and fell through to the null callback
— `Object(1).valueOf()`, `new Number(1).valueOf()` and `(1).valueOf()` all
answered `null`.

`isEqualWith` compares a boxed against an unboxed primitive by matching
`Object.prototype.toString` tags and then `Object.is(a.valueOf(), b.valueOf())`,
so both halves are load-bearing.

New `BoxPrimitive` node → `smelt_box_value`; `valueOf` on an erased receiver →
`smelt_value_of_method`, which prefers a user-defined own `valueOf` and
otherwise unwraps via `smelt_unbox_primitive` (wrapper → primitive, Date → epoch
ms, primitive → itself, other object → itself).

Strings are deliberately left unboxed, matching `new String(x)`, which already
lowers to the plain string. See "not attempted" below.

**+8 passing.**

### 3. A value-less return yielded `null`, not `undefined`

Falling off the end of a JS function — or a bare `return;` — evaluates to
`undefined`. The return terminator lowered to `Constant::None`, which the
erasure seam renders `SmeltUnknown::Null`.

`cloneDeepWith` guards its customizer with `if (cloned !== undefined) return
cloned;`, so a fall-through customizer like
`(v) => { if (typeof v === 'number') return v * 2; }` answered `null` on its
first call — against the whole object — and collapsed the entire clone to
`null`. This is what took `cloneDeepWith` to zero failures.

`Constant::None` is kept for the two return types that need it (`None`/void
emits Rust `()`, `Optional<T>` needs Rust `None`); only the erased type switches.
Mirrors the rule already applied to an uninitialized `let` binding.

**+7 passing.**

### 4. An erased `Map` had no prototype methods

An erased Map is `{ __smelt_map: [[k, v], ...] }` and only `.size` was
synthesized — the erased-Set block right below already had
`keys`/`values`/`entries`/`has`/`forEach`. Every other read answered `undefined`,
and *calling* that `undefined` collapsed to a null callback rather than failing,
so the miss was silent.

`isEqualWith` walks a Map with `for (const [key, value] of a.entries())` after
checking `a.size !== b.size`. With `entries()` empty the loop body never ran, so
two same-size Maps holding completely different entries compared **equal**.

Added `keys`/`values`/`entries`/`get`/`has`/`forEach`. Mutators are left out, as
in the Set block: mutation happens on the typed `SmeltJsMap`.

**+2 passing.**

## Remaining 38 failures, by root cause

Ranked by how much a single general fix would buy. Each cause was confirmed by a
focused probe compiled into the corpus, not inferred from the test name.

### A. `SmeltArray` has value semantics — ~8 tests, and the one hard blocker

`SmeltObject` stores `values: Rc<RefCell<HashMap<..>>>`; `SmeltArray` stores a
plain `Vec`, so `Clone` copies it. A comment at `crates/smelt-codegen-rust/src/lib.rs`
(`smelt_fresh_identity`) already *claims* both share their `Rc` — the array half
was never implemented.

Confirmed: `const nested = { a: [1,2,3] }; nested.a[2] = 4;` leaves
`nested.a[2] === 3`. `smelt_get_object_field` hands back a clone of the
`SmeltUnknown::Array`, so `set_index` writes to a copy.

This is also why the circular cases **hang** rather than fail:
`isEqualWith`'s cycle guard does `stack.set(a, b)` keyed on the array, and
`SMELT_LIST_IDENTITIES` keys the erased id on `Vec::as_ptr` — `a.push(a)`
reallocates, the id changes, the guard misses, and the recursion never
terminates. Two `isEqualWith` circular tests and the transitive-equivalence test
are in this bucket, plus `cloneDeep should deep clone nested objects` and the
sparse-array cases.

Fixing it means `values: Rc<RefCell<Vec<SmeltUnknown>>>`, which removes
`Deref<Target = [SmeltUnknown]>` and therefore every slice method emitted code
reaches through it (`len`, `iter`, indexing, `first`/`last`, `to_vec`).
Deliberately not started here: it is a multi-session change needing revalidation
across remeda (1789 tests), radash and es-toolkit, and it is not worth landing
half-done. It also unblocks family 10 of `estoolkit-runtime-current.md`
(`pull`/`pullAt`/`remove`, 12 tests).

### B. Erased function identity — 2 tests

`isEqualWith(a, a, noop)` answers **false**. Each erasure of a callable builds a
fresh adapter (`{ let smelt_callback = a.clone(); Rc::new(move |args| ..) }`), and
`js_strict_eq` compares functions with `Rc::ptr_eq`, so two erasures of the same
binding are never `===`.

The fix has a clear precedent: memoize the erased adapter on the source
callable's allocation address, exactly as `SMELT_LIST_IDENTITIES` does for
arrays, and hand back the cached `Rc`. The uncertainty is the emit site —
`rest_vector_unknown_adapter_text`'s owned branch keys on "operand is a place
that is not a function parameter", and the Rust type there is not always an `Rc`
(a plain `fn` item would not take `&Rc<F>`). Enumerate those shapes before
wiring the memo. Covers `isEqualWith should compare functions` and
`clone should return functions as is`.

### C. Host/exotic representations — ~12 tests

`ArrayBuffer`, `SharedArrayBuffer`, `Buffer`, `Blob`, `File`, `DataView`, typed
array views, `arguments`. One rep at a time, not a bounded codegen patch — same
conclusion as the earlier campaign. `cloneDeep should clone arraybuffer objects`
still panics `unknown is not array`.

### D. `cloneDeep` of a `RegExp` — 2 tests

`cloneDeep(/abc/gi)` loses source and flags, while shallow `clone` keeps them.
Every ingredient works in isolation: reading `.source`/`.flags` off an erased
RegExp gives `"abc"`/`"gi"`, and `new RegExp(src, flags)` builds correctly from
both literal and erased-string arguments. So the defect is the **guard**, not
the parts — the `valueToClone instanceof RegExp` branch in `cloneDeepWithImpl`
is not being taken, and the value falls through to the generic
`Object.create(getPrototypeOf(x))` path (whose `Object.keys` is empty for a
RegExp marker). Start at `instance_of_text` for RegExp on an erased receiver.

### E. Symbol-keyed properties — 2 tests

`cloneDeep({ [Symbol()]: 1 })` drops the symbol key. `copyProperties` collects
`[...Object.keys(source), ...getSymbols(source)]`; symbol keys live as
`__smelt_symbol:<..>` entries filtered out of `Object.keys`, and
`Object.getOwnPropertySymbols` does not appear to be modeled, so nothing puts
them back. Covers `cloneDeep should clone objects` and
`isEqualWith should compare symbol properties`.

### F. Boxed `String` wrappers — 1 test

`cloneDeep should clone String objects` needs `new String(x)` to be a real
wrapper: the spec asserts `cloned !== strObj` and `toBeInstanceOf(String)`.
`new String(x)` currently lowers to the plain string, so the clone is `===` the
original. Boxing strings means re-exposing the whole `String.prototype` surface
— `length`, character indexing, every string method — on a marker object.
A deliberate non-goal for now; `smelt_box_value` documents the choice and keeps
`Object(str)` consistent with it.

## Adjacent gap found, not fixed

A bare `(v) => null` infers `Type::None`, which HIR still conflates with `void`,
so the callback adapter **drops the returned value** and substitutes `undefined`
(`move |arg0| { (cb)(arg0); SmeltUnknown::Undefined }`). Any customizer protocol
that distinguishes `null` from `undefined` misreads such a callback. This is the
`null`/`void` conflation in `specs/distinct-undefined.md`, and it is the same
lever family 5 (`merge`/`toMerged`/`mergeWith`, ~24 tests) is waiting on. The
regression test in
`crates/smelt-codegen-rust/tests/boxed_primitive_runtime.rs` annotates it and
pins the explicitly-annotated spelling instead.

## Reproducing

```sh
ref="$(python3 -c "import json; libs=json.load(open('.github/compat/libraries.json'))['libraries']; print(next(l['ref'] for l in libs if l['name']=='es-toolkit'))")"
git clone --no-tags --filter=blob:none https://github.com/toss/es-toolkit.git target/compat-repos/es-toolkit
git -C target/compat-repos/es-toolkit checkout "$ref"
cp -R .github/compat/es-toolkit/. target/compat-repos/es-toolkit/
cargo build --bin smelt
target/debug/smelt --manifest-path target/compat-repos/es-toolkit/Smelt.toml probe --format json --output probe.json
RUSTFLAGS=-Awarnings cargo test --manifest-path target/compat-repos/es-toolkit/dist-smelt/Cargo.toml -- --test-threads=4
```

The fastest way to diagnose one of the causes above is to drop a throwaway
`src/predicate/zzprobe.spec.ts` into the checkout (it matches the manifest's
`predicate/**/*.spec.ts` test prefix), `console.log` the values in question, and
run `cargo test ... zzprobe -- --nocapture`. That is how every root cause here
was pinned down; the generated assertion messages carry no actual/expected, so
reading them is not enough. Bound the run with `timeout` — the circular-reference
cases in cause A do not terminate.
