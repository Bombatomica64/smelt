# es-toolkit: typed arrays get real view identity

Follow-on to `estk-host-representations.md`, which deliberately deferred this.
Measured against es-toolkit at the ref pinned in `.github/compat/libraries.json`
(`e008a281`) with the fixture manifest `.github/compat/es-toolkit/Smelt.toml`,
starting from Smelt `7bdd67c` (the `claude/estoolkit-utilities-wxhb0t` tip, which
equals `main`).

## Result

| Corpus | Before | After |
| --- | --- | --- |
| es-toolkit | 871 passed / 188 failed | **873 passed / 186 failed** |
| remeda | 1789 passed / 0 failed | **1789 passed / 0 failed** |
| radash | fails to compile, 3 errors | fails to compile, **same 3 errors** |

Newly passing (3):

| Test | Why it was failing |
| --- | --- |
| `flattenObject handles TypedArrays correctly` | a typed array was a plain list, so `flattenObject` descended into it |
| `isEqual should return false for different array buffers` | `new Uint8Array([1, 2, 3]).buffer` was `undefined` |
| `isTypedArray returns true for typed arrays` | `ArrayBuffer.isView` was false for every numeric view |

Newly failing (1) — and it is an *accidental* pass, see "the one regression"
below:

| Test |
| --- |
| `isEqualWith should compare buffers when customizer returns undefined` |

The target test, `isEqualWith should compare array views when customizer returns
undefined`, **still fails**. It needs three further features that are not about
typed-array representation at all; see "what the target test still needs".

## The representation, and why it is the idiomatic one

Smelt modeled every numeric typed array as a plain `Vec<f64>`. Probe-confirmed
consequences, all wrong:

| Expression | Answered | Should answer |
| --- | --- | --- |
| `Object.prototype.toString.call(new Float32Array(new ArrayBuffer(8)))` | `[object Array]` | `[object Float32Array]` |
| `new Float32Array(new ArrayBuffer(8)).length` | `8` (the byte count) | `2` |
| `new Float64Array(new ArrayBuffer(8)).length` | `8` | `1` |
| `ArrayBuffer.isView(new Uint8Array(buf))` | `false` | `true` |
| `new Uint8Array([1, 2, 3]).buffer` | `undefined` | an `ArrayBuffer` |
| `new Float32Array([1.1])[0]` | `1.1` | `1.100000023841858` |
| `new Int8Array([-1]).buffer` vs `new Uint8Array([255]).buffer` | not comparable | byte-identical |

The eleven views are now **byte-backed host objects that carry their own element
type**. Each joins `smelt_stdlib::host_object::HOST_OBJECTS` with its own identity
marker, its own `Object.prototype.toString` tag, `ByteBufferRole::View`, and a new
`TypedArrayElement` (`Int8`, `Uint8`, `Uint8Clamped`, `Int16`, `Uint16`, `Int32`,
`Uint32`, `Float32`, `Float64`, `BigInt64`, `BigUint64`). Node's `Buffer` gets
`Uint8`, because it subclasses `Uint8Array`.

**Why this is the idiomatic representation, not a marker dodge.** A typed array
*is* bytes plus an element type — that is the whole of what it is, in JavaScript
and in Rust. The element type is load-bearing here, not decoration:

* it is the stride that turns a byte count into an element count, so a
  `Float64Array` over eight bytes has one element where a `Uint8Array` has eight;
* it decides signedness, so the same byte `0xff` reads as `255` through `uint8`
  and `-1` through `int8`;
* it decides precision, so `new Float32Array([1.1])[0]` is
  `1.100000023841858` — a value no list of `f64` source literals holds;
* it decides the write rule, so `Uint8Array` wraps modulo 256 where
  `Uint8ClampedArray` saturates;
* and it decides what `new Float32Array(source)` *means*: over an `ArrayBuffer` it
  re-views the bytes, over another view or an array it converts the elements. A
  shapeless byte copy cannot tell those two apart.

All of that is derived from one registry table. The generated runtime has exactly
one little-endian decode/encode pair per element type
(`byte_buffer_prelude::decode_expression` / `encode_expression`), selected by the
record's own marker — the emitter and the emitted code never restate a width.

The end state a skilled Rust team would reach is one step further: `Vec<f32>` /
`Vec<u8>` element storage with a borrowed window over shared bytes. That needs
sub-`f64` numeric types in HIR, which Smelt does not have (`Type::Int` and
`Type::Float` only) — see "deliberately deferred".

## What changed

Every change is a general rule driven by the shared registry; none is keyed on a
library function name.

### 1. The registry carries element types

`smelt_stdlib::host_object` gained `TypedArrayElement` (with `byte_width` and a
stable `tag`) and an `element: Option<TypedArrayElement>` field on `HostObject`.
`ArrayBuffer`/`SharedArrayBuffer` stay `None` (they own bytes without interpreting
them) and so does `DataView`, whose element type is chosen per
`getFloat32`/`setInt16`/... call rather than by the view.

### 2. One byte-buffer constructor for every byte-backed kind

`smelt_host_buffer_construct(marker, args)` is the single construction path, and
`new Uint8Array(...)`, `new ArrayBuffer(...)`, `new DataView(...)`,
`Buffer.from`/`alloc`/`concat`, `new Buffer(...)` and the reflected
`new Object.getPrototypeOf(x).constructor(...)` all route through it via
`ExprKind::HostConstruct`. It implements the four JavaScript forms — `new X(n)`
(`n` *elements*), `new X(buffer[, byteOffset[, length]])` (a byte **view** sharing
the buffer record), `new X(otherView)` / `new X([...])` (an element
**conversion**), and `new X()` — with the storage/view split taken from the
registry roles.

`Buffer.from(...)` used to build its record inline as a `DictLit`, which left it
without the `buffer`/`byteOffset` slots the shared constructor gives every view;
es-toolkit's `clone`/`cloneDeep` buffer specs compare a reflectively-built clone
against the directly-built original with *structural* equality, so the two shapes
had to agree. The `DataView` arm of `smelt_reflected_construct` also disappeared:
it existed only because that constructor could not yet window a separate storage
buffer.

### 3. The record layout is element-aware

```text
{ "<marker>": true, "bytes": [b, ...], "byteLength": N,
  "length": N / BYTES_PER_ELEMENT, "byteOffset": off, "buffer": <ArrayBuffer> }
```

`length` is the element count; indexed reads and writes decode/encode at the
element's width and signedness; `slice`/`subarray` bounds are element indices for
a view and byte indices for byte-addressed storage (one code path, once the stride
comes from the registry); and an element write also lands **in place** in the
backing `buffer` record, so `view[0] = 1` is visible through `view.buffer` without
minting a new buffer identity (`view.buffer === buffer` keeps holding).

### 4. Own-key enumeration is the element indices

A typed array's own enumerable properties are exactly its indexed elements:
`Object.keys(new Uint8Array(1))` is `['0']`. `Object.keys`/`values`/`entries` now
answer that for a byte-backed record on both the tagged-`SmeltUnknown` and the
structural-`SmeltRecord` paths (remeda's `isShallowEqual` reads the record path).
Leaking `bytes`/`byteLength`/`length`/`buffer` instead would make a deep-equality
walk compare internal storage and recurse through `buffer` back into the view.

### 5. `instanceof` and `ArrayBuffer.isView` resolve through markers

`x instanceof Uint8Array` used to fold to a constant derived from the static type
(`true` for *any* `number[]`), because the numeric-list model left no identity to
test. It is now the registry marker probe, and it accepts more than one marker
where the platform has a real subclass relation the registry already records in
`to_string_tag` — so a Node `Buffer` satisfies `buf instanceof Uint8Array`.

`ArrayBuffer.isView` is the `instanceof` disjunction over the registry's `View`
entries, which grew from 2 to 13. That disjunction is now built as a **balanced**
`||` tree: `||` is associative over these pure marker probes, but the nesting depth
is what the short-circuit control-flow lowering recurses over, and a twelve-deep
left-nested chain **overflowed the emitter's stack**. (That is a pre-existing
scalability property of short-circuit lowering, not of this change — worth its own
look, since ordinary source with a dozen `||` clauses would hit it too.)

### 6. `crypto.getRandomValues` accepts an erased view

Its argument is a typed array, whose static type is now the erased dynamic one, so
the lowering accepts that alongside a concrete numeric list. (remeda's
`randomBigInt` is the only in-corpus caller and stopped lowering without this.)

## The one regression, and why it is an accidental pass

`isEqualWith should compare buffers` asserts
`isEqualWith(Buffer.from([1]), new Uint8Array([1]), noop) === false`. It passed
before only because `new Uint8Array([1])` reported the **wrong** spec tag
(`[object Array]`), so es-toolkit's tag comparison rejected the pair for the wrong
reason. Both values now correctly report `[object Uint8Array]` (Node's `Buffer`
*is* a `Uint8Array`, and `estk-host-representations.md` set that tag deliberately),
so the pair reaches the typed-array arm, where the only discriminator is
`isBuffer(a) !== isBuffer(b)`.

`isBuffer` is broken by two *pre-existing* defects, both documented and neither
about typed arrays:

1. es-toolkit's `isBuffer` reads `globalThis.Buffer.isBuffer(x)` through the
   module-level `globalThis_` shim, and cross-module const-item inlining flattens
   that `||` chain to an **empty record** at the use site, losing the
   `__smelt_global_object` marker (`estk-const-item-inlining.md`). Generated
   `isBuffer_1.rs` reads a field off `{}`.
2. The property read `.Buffer` lowers to the key `"buffer"` — a source-name
   interning collision, the same class of defect `estk-host-representations.md`
   recorded for a local named `Constructor`.

So `isBuffer(x)` is a constant `false` for every value, which is also why
`isBuffer should return true for buffer instances` fails in the baseline. Fixing
either defect alone is not enough: the global-object record carries no members at
all, so `globalThis.Buffer` would still be `undefined`. Making it work needs the
global object to expose the modeled host constructors as members *and* both
defects fixed — three separate changes, none of them typed-array representation.
Left alone rather than papered over.

## What the target test still needs

`isEqualWith should compare array views` was the named target. Reading the
generated code shows the blockers are not representational:

```js
const CtorA = globalThis[type] || function (n) { this.n = n; };
const bufferA = globalThis[type] ? new ArrayBuffer(8) : 8;
return [new CtorA(bufferA), new CtorA(bufferA), new CtorB(bufferB), new CtorB(bufferC)];
```

* `globalThis[type]` — a **dynamic** string index into the global object — lowers
  to `SmeltUnknown::Undefined`, so the whole probe answers "this view does not
  exist". The interned builtin-namespace values that `smelt_builtin_namespace(name)`
  already mints for every registry host constructor are exactly what it should
  resolve to; the global-object record just has no members.
* `globalThis[type] || function (n) { … }` lowers the local `ctor_a` to **`bool`**,
  so it could not be called even once the left operand resolved.
* `new CtorA(bufferA)` — `new` on an **erased callee** — lowers to
  `SmeltUnknown::Null`. Nothing dispatches to the `__smelt_call` slot a
  builtin-namespace record already carries.

The same three blockers gate `isEqual should compare array views` and, notably,
about nineteen currently-*vacuous* passes: `clone`/`cloneWith should clone <type>
values` (nine each), `merge`, `transform` and `isTypedArray` all begin with
`const Ctor = globalThis[type]; if (!Ctor) return;` and today return early. Those
tests read `actual.buffer === view.buffer`, `byteOffset` and `length` on a cloned
view — which is exactly why this change put real `buffer`/`byteOffset` slots and
in-place write-through on the records first. Fixing `globalThis[dynamic]` without
a solid view representation underneath would have converted nineteen vacuous
passes into failures.

## Deliberately deferred

* **`Vec<f32>` / `Vec<u8>` element storage.** The genuinely idiomatic end state
  needs sub-`f64` numeric types in HIR (`Type::Int`/`Type::Float` are all there
  is) and a `Type` variant to preserve the view spelling through interning — the
  `Type::JsMap` pattern, which touches ~95 sites across 33 files. Element *values*
  are already faithful (rounded and wrapped at the real width); only the storage
  is still `f64` bytes.
* **Write-through aliasing between two views over one buffer.** Both views share
  the buffer *record* (so `view.buffer === buffer` holds and a write through a view
  updates that buffer in place), but each keeps its own byte window, so a write
  through one view is not observed through the *other* view. Making the window a
  borrow of the storage record is a change to record identity, not to element
  typing, and nothing in the three measured corpora reads a byte written through an
  aliasing view.
* **Splitting `k in obj` from `Object.hasOwn(obj, k)`.** One emitter serves both,
  and they differ in JavaScript: `in` walks the prototype chain, so
  `'length' in view` must be `true` (remeda's `isEmptyish` reads exactly that)
  while `Object.hasOwn(view, 'length')` is `false`. `Object.keys`/`values`/
  `entries` report the correct own-key set; the property test is deliberately the
  `in` answer. Pre-existing conflation.

## SmeltUnknown delta

es-toolkit ratchet (`blocker-logs/smelt-unknown-baseline-es-toolkit.json`):

| Category | Baseline | Current | Delta |
| --- | ---: | ---: | ---: |
| Runtime prelude | 3007 | 3034 | +27 |
| Legitimate boundary | 40453 | 38378 | −2075 |
| Avoidable erasure | 35682 | 35711 | **+29** |

Total occurrences fall (79142 → 77123). The examples-corpus hard invariant is
untouched: avoidable erasure there stays 0.

Per-shape attribution of the +29 (net of offsetting removals):

| Delta | Shape | Example |
| ---: | --- | --- |
| +54 | `let _smelt_tmp: SmeltUnknown = smelt_reflected_construct("S", vec![SmeltUnknown::Array(…)])` | `cloneDeep_spec.rs:676` |
| +28 | same, as an assignment | `cloneDeep_spec.rs:686` |
| +27 | `let _smelt_tmp_N: SmeltUnknown;` (temporaries for the above) | `AbortError_spec.rs:49` |
| −25 | the old typed-array record literal | `flattenObject_spec.rs:145` |
| −24 | the old typed-array record literal | `cloneDeep_spec.rs:1161` |
| −22 | the old list-literal typed-array argument | `isTypedArray_spec.rs:55` |
| ±0 | `hasOwn`/`Object.keys` shapes that merely gained a byte-buffer arm (equal + and − pairs in `orderBy.rs`, `assign.rs`, `toCamelCaseKeys_spec.rs`, `bindAll.rs`, `get.rs`) | — |

Every added occurrence is a **construction of a modeled host object** —
`smelt_reflected_construct("<view>", …)` and its temporaries — which is the same
shape, at the same call sites, that the existing `new ArrayBuffer(8)` /
`Buffer.from(...)` constructions already contribute to this metric today.

**The committed baseline is deliberately left unchanged, so the CI gate still
flags this.** Reclassifying `smelt_reflected_construct` as a legitimate boundary in
`classify_line` would clear it — and would arguably be *consistent*, since it would
reclassify the existing `ArrayBuffer`/`Buffer`/`DataView` construction lines the
same way — but the policy in `AGENTS.md` allows reclassification only when concrete
types, unions or scoped generics genuinely cannot represent the value, and a typed
array *can* be a concrete Rust type (that is the deferred end state above). Claiming
otherwise to clear a gate would be dishonest, so this needs a maintainer decision
between:

1. reclassifying host-object *construction* as a legitimate boundary (a broad,
   principled change that also lowers the number for the pre-existing byte-buffer
   lines), or
2. re-snapshotting the baseline to accept +29 as the cost of correct view identity.

## Regression tests

* `crates/smelt-stdlib/src/host_object.rs` — the element-type table is asserted:
  eleven views plus `Buffer`, the byte widths, the `Storage`/`View` role split, and
  that every name in `TYPED_ARRAY_CLASS_NAMES` has a registry entry with an
  element type. A drift here silently changes `length`, `isTypedArray`, and every
  decode.
* `crates/smelt-codegen-rust/src/tests/part_7_tests.rs` — string goldens (these run
  in CI) for the per-view spec tags, the element codec (one decode/encode per
  width, signed vs unsigned, `Uint8ClampedArray` saturating, the
  `(marker, tag, BYTES_PER_ELEMENT)` table with the byte-addressed kinds excluded),
  `length` as the element count, the storage-view/element-conversion split, the
  element-index own-key set, and marker-based `instanceof`.
* `crates/smelt-codegen-rust/tests/typed_array_runtime.rs` — four end-to-end runtime
  cases (`#[ignore]`d by convention, run with `-- --ignored`) that execute the
  generated programs. Only running them proves the records decode real elements:
  `distinct_views_over_one_buffer_are_distinguishable` pins the
  `Uint8Array`/`Uint8ClampedArray` pair that no length heuristic can separate, and
  `element_typing_is_real_not_a_marker` pins the four facts a marker alone cannot
  carry (signedness, single precision, modulo wrap, saturation).

Two frontend tests changed shape and say why in a comment:
`instanceof_typed_array_folds_to_boolean` became
`instanceof_typed_array_resolves_through_the_view_marker` (a `number[]` no longer
answers `true`; a concrete non-class operand now raises the same honest blocker
every other host-object `instanceof` raises), and
`typed_array_constructors_lower_to_numeric_lists` became
`..._lower_to_byte_buffer_host_constructs`.

## Reproducing

```sh
ref="$(python3 -c "import json; libs=json.load(open('.github/compat/libraries.json'))['libraries']; print(next(l['ref'] for l in libs if l['name']=='es-toolkit'))")"
git clone --no-tags --filter=blob:none https://github.com/toss/es-toolkit.git target/compat-repos/es-toolkit
git -C target/compat-repos/es-toolkit checkout "$ref"
cp -R .github/compat/es-toolkit/. target/compat-repos/es-toolkit/
cargo build --bin smelt
target/debug/smelt --manifest-path target/compat-repos/es-toolkit/Smelt.toml build
RUSTFLAGS=-Awarnings cargo test --manifest-path target/compat-repos/es-toolkit/dist-smelt/Cargo.toml -- --test-threads=4
```

remeda (the gate that caught the two `isEmptyish` and one `isShallowEqual`
regressions this change went through before landing):

```sh
git clone --no-tags --filter=blob:none https://github.com/remeda/remeda.git target/compat-repos/remeda
git -C target/compat-repos/remeda checkout 3c80f28bb394edbf89f1fc9978571dec8ed20edc
cp -R .github/compat/remeda/. target/compat-repos/remeda/
target/debug/smelt --manifest-path target/compat-repos/remeda/Smelt.toml build
RUSTFLAGS=-Awarnings cargo test --manifest-path target/compat-repos/remeda/dist-smelt/Cargo.toml --no-fail-fast
```

Generated assertion messages carry no actual/expected values, so drop a throwaway
`src/predicate/zzprobe.spec.ts` into the es-toolkit checkout (it matches the
manifest's `predicate/**/*.spec.ts` test prefix), `console.log` the values, and run
`cargo test ... zzprobe -- --nocapture`. Every claim above was pinned that way or by
reading the generated Rust directly.
