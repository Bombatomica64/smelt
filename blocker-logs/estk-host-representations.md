# es-toolkit: host / exotic representations

Cause **C** of `estk-clone-and-equality.md` — the byte-buffer host objects,
host-constructor identity, and the `arguments` object. Measured against es-toolkit
at the ref pinned in `.github/compat/libraries.json` (`e008a281`) with the fixture
manifest `.github/compat/es-toolkit/Smelt.toml`, starting from Smelt `86c8d3b`
(the `claude/estoolkit-utilities-wxhb0t` tip).

## Result

| | Passed | Failed |
| --- | ---: | ---: |
| Before | 850 | 209 |
| After | 861 | 198 |

**+11 passing, 0 newly failing.** Eleven of the twelve targeted tests now pass:

| Test | Now |
| --- | --- |
| `clone should clone ArrayBuffer` | pass |
| `clone should clone SharedArrayBuffer` | pass |
| `clone should clone Data views` | pass |
| `clone should clone File` | pass |
| `cloneDeep should clone ArrayBuffer objects` | pass |
| `cloneDeep should clone buffers` | pass |
| `cloneDeep should clone Blob objects` | pass |
| `cloneDeep should clone File objects` | pass |
| `isEqualWith should compare buffers` | pass |
| `isEqualWith should compare arguments objects` | pass |
| `isEqualWith should treat arguments objects like Object objects` | pass |
| `isEqualWith should compare array views` | **still fails** — see below |

`SmeltUnknown` ratchet: avoidable erasure 35621 → **35341** (−280); runtime
prelude 2686 → 2985 (+299, the new helpers); legitimate boundary 40965 → 42686.
The es-toolkit baseline is re-snapshotted in the same commit. The examples-corpus
hard invariant holds: avoidable erasure stays 0.

## What was actually missing

The byte-buffer host objects were **identity-only**: a record carrying
`{ __smelt_arraybuffer: true, byteLength: n }` and nothing else. Every clone or
deep-equality path over binary data walks straight into what that lacks.

Confirmed by probe, before the fix:

| Operation | Answered |
| --- | --- |
| `arrayBuffer.slice(0)` | **the same object** (`cloned === buffer`) |
| `new SharedArrayBuffer(8).byteLength` | `undefined` |
| `new DataView(ab).byteLength` / `.byteOffset` / `.buffer` | `undefined` |
| `Buffer.from([1,2,3])[0]` | `null` |
| `buffer.subarray()` | `null` |
| `new Uint8Array(arrayBuffer)` | **panic** `unknown is not array` |
| `ArrayBuffer.isView(Buffer.from([1]))` | `false` |
| `Object.prototype.toString.call(Buffer.from([1]))` | `[object Buffer]` |
| `Blob === Blob` | `false` |
| `blob.constructor` | `undefined` |
| `Object.keys(arguments)` | `["length"]` |

## Fixes landed

Each is a general rule with a regression test; none is keyed on a library
function name.

### 1. Byte storage for the binary-data host objects

`smelt_stdlib::host_object` gained a `ByteBufferRole` (`Storage` for
`ArrayBuffer`/`SharedArrayBuffer`, `View` for `Buffer`/`DataView`) and a
`to_string_tag` override. Everything downstream reads the registry rather than
restating which markers have bytes: the runtime helpers, the frontend
construction dispatch, and `ArrayBuffer.isView`.

A byte-backed record is
`{ "<marker>": true, "bytes": [...], "byteLength": N, "length": N }`, and one new
prelude module (`byte_buffer_prelude.rs`) is the only place that knows that
layout. It backs:

* `.slice()` / `.subarray()` on an erased receiver — a **fresh** record of the
  same host identity over the sliced bytes. The erased-slice lowering used to
  forward any non-array receiver unchanged, which is why `clone(buf)` (whose whole
  body is `return obj.slice(0)`) returned its own argument.
* indexed element reads *and writes* (`buffer[1]`, `result[i] = byte`), so a byte
  buffer's indexed slots are its bytes rather than record properties.
* array-like element extraction, so `new Uint8Array(arrayBuffer)` views the
  buffer's bytes instead of panicking.
* `ArrayBuffer.isView`, now the `instanceof` disjunction over the registry's
  `View` entries. Answering only for `DataView` left
  `isTypedArray(Buffer.from([1]))` false, so the buffer's clone path was never
  taken.

`subarray` joins `slice` in `collection_slice_call`: same `(begin, end)` index
arguments, and since Smelt models byte buffers by value a copy is the faithful
lowering — and it is the *distinct object* the `isBuffer(v) ? v.subarray() : ...`
clone idiom is after.

### 2. Node `Buffer` reports `[object Uint8Array]`

`Buffer` subclasses `Uint8Array`, so the platform tag is `[object Uint8Array]`.
`isEqualWith` says so in a comment ("Buffers are also treated as
`[object Uint8Array]`s") and routes them through its typed-array arm; a
`[object Buffer]` tag fell off the end of both its `switch` and
`cloneDeepWith`'s. Two equal buffers compared **unequal**, and a buffer was not
cloneable.

### 3. One host constructor for the direct and the reflected path

es-toolkit reaches a host constructor two ways — directly in `cloneDeepWith`
(`new DataView(v.buffer.slice(0), v.byteOffset, v.byteLength)`) and reflectively
in `clone` (`new Object.getPrototypeOf(obj).constructor(...)`) — and its specs
compare the two results against each other. The reflected constructor was
`smelt_fresh_identity(args[0])` for every kind but `Error`, so
`new Constructor(view.buffer.slice(0))` answered a shallow copy of the *buffer*,
not a `DataView`.

New `HostConstruct` HIR/MIR node routes `new ArrayBuffer(...)` /
`new SharedArrayBuffer(...)` / `new DataView(...)` through the *same*
`smelt_reflected_construct` the reflected path calls, so the two are
indistinguishable by construction rather than by two hand-matched lowerings. That
constructor now builds the byte buffers (from a length, another byte buffer, or an
element array), `DataView` (a window over separate storage, retaining
`buffer`/`byteOffset`/`byteLength`), and `Blob`/`File` (through the existing
`smelt_blob_record_from_parts`, which became unconditional since an unconditional
prelude helper cannot call a gated one).

The marker→kind, marker→class, and kind→class tables moved into a focused
`reflection_prelude.rs` derived from the registry.

### 4. Interned host-constructor values, and `.constructor` on a host record

JavaScript exposes one object per global builtin name, so `Blob === Blob` and
`blob.constructor === Blob` both hold. A bare `Blob` reference built a
`__smelt_builtin_namespace` **record literal**, and a record mints its identity on
construction — so every mention was a different object and both comparisons were
false. A host record answered `undefined` for `.constructor` besides.

New `BuiltinNamespace` HIR/MIR node → `smelt_builtin_namespace(name)`, one cached
record per name, carrying a `__smelt_call` slot when the name also names a modeled
host constructor. `smelt_get_object_field` resolves a marker-bearing record's
`.constructor` to that same value, and the reflected prototype's `constructor`
slot is that value too. A plain object still answers `undefined`, which is what
makes two plain objects compare as equal instances.

### 5. The `arguments` object carries its argument values

`arguments` lowered to `{ length: <declared arity> }` — no values at all. So
`Object.keys(arguments)` enumerated `["length"]`, the exact inverse of the real
key set: an `arguments` object's indexed elements are enumerable own properties
and its `length` is *not*.

New `ArgumentsObject` node rebuilds the object from the enclosing function's own
parameters (`body.params` is the positional parameters followed by the rest
parameter's local; the arity stack gives the split), flattening the rest list onto
the end. `length` is stored but filtered from own-key enumeration — the same shape
as the existing `__smelt_regexp` → `source`/`flags` and `__smelt_error` →
`message`/`cause`/`errors` exceptions — and the record tags as
`[object Arguments]`, which `isEqualWith` folds to `objectTag` and
`cloneDeepWith`'s `isCloneableObject` accepts.

A reference from an arrow function *nested* inside the owning function keeps the
length-only record: the nested body has its own parameter list, so there is
nothing local to read, and reporting a closure's parameters as the outer call's
arguments would be worse than the old stand-in.

## Not fixed: `isEqualWith should compare array views`

Root cause, confirmed by probe:

```
new Float32Array(new ArrayBuffer(8))  ->  tag [object Array], length 8
new Float64Array(new ArrayBuffer(8))  ->  tag [object Array], length 8
isEqualWith(f32, f64, noop)           ->  true   (expected false)
```

Smelt models the numeric typed arrays as **plain `Vec<f64>` lists**, so no view
identity survives: the tag is `[object Array]` for every one of them, and the
length is the byte count rather than the element count (`Float64Array` over eight
bytes should be length 1, not 8). The spec walks
`[...typedArrays, 'DataView'].map(...)` and asserts each consecutive pair compares
unequal. Length alone cannot carry it either — `Uint8Array` and
`Uint8ClampedArray` over the same buffer have the *same* element count, so that
pair needs genuinely distinct tags.

**Deliberately not attempted.** Giving the numeric typed arrays real view
identity means changing their representation from `List<Float>` to a
marker-bearing record across the whole corpus: every `.map`/`.filter`/`.length`/
index/`Array.from` use of a typed array in remeda (1789 tests), radash and
es-toolkit changes shape with it. That is the same class of change as cause A's
`SmeltArray` value semantics, and the same conclusion applies — it needs its own
session and revalidation across all three libraries, and is not worth landing
half-done. It would also fix `isTypedArray` and `ArrayBuffer.isView` for the
numeric views, which are wrong today for the same reason.

## Adjacent gaps found, not fixed

A local binding named exactly `Constructor` makes a `.constructor` **field read**
in the same module resolve to the key `"Constructor"` — so
`const Constructor = Object.getPrototypeOf(x).constructor` reads a field that does
not exist and answers `undefined`. Reproduced in a standalone program; renaming the
local to `Ctor` fixes it. es-toolkit's `clone` spells exactly that binding but is
unaffected (its generated read is lowercase), so the trigger is narrower than the
name alone — likely source-name interning unifying the local's symbol with the
field key under some condition. Pre-existing, unrelated to host representation;
the runtime regression test avoids the name and says why.

`isBuffer(x)` is **false** for a real `Buffer`, so `cloneDeep`'s
`isBuffer(v) ? v.subarray()` branch is never taken (the test passes through the
typed-array branch instead, which the `ArrayBuffer.isView` fix opened). The cause
is not host representation: es-toolkit's `isBuffer` reads
`globalThis.Buffer.isBuffer(x)` through the module-level const
`globalThis_ = (typeof globalThis === 'object' && globalThis) || ...`, and
const-item inlining flattens that `||` chain to an **empty record**
`SmeltRecord::from([])` at the use site — the `__smelt_global_object` marker is
lost. Generated `isBuffer_1.rs` then reads field `"buffer"` off `{}`. That is the
const-item-inlining defect (`estk-const-item-inlining.md`), not this campaign.

## Reproducing

```sh
ref="$(python3 -c "import json; libs=json.load(open('.github/compat/libraries.json'))['libraries']; print(next(l['ref'] for l in libs if l['name']=='es-toolkit'))")"
git clone --no-tags --filter=blob:none https://github.com/toss/es-toolkit.git target/compat-repos/es-toolkit
git -C target/compat-repos/es-toolkit checkout "$ref"
cp -R .github/compat/es-toolkit/. target/compat-repos/es-toolkit/
cargo build --bin smelt
target/debug/smelt --manifest-path target/compat-repos/es-toolkit/Smelt.toml probe --format json --output probe.json
RUSTFLAGS=-Awarnings timeout --signal=KILL 40m cargo test --manifest-path target/compat-repos/es-toolkit/dist-smelt/Cargo.toml -- --test-threads=4
```

Drop a throwaway `src/predicate/zzprobe.spec.ts` into the checkout (it matches the
manifest's `predicate/**/*.spec.ts` test prefix), `console.log` the values in
question, rebuild, and run `cargo test ... zzprobe -- --nocapture`. Every root
cause above was pinned that way; the generated assertion messages carry no
actual/expected values, so reading them is not enough.

## Regression tests

* `crates/smelt-stdlib/src/host_object.rs` — the byte-buffer role split and the
  single `to_string_tag` override are asserted, since a regression there silently
  changes `isTypedArray`.
* `crates/smelt-codegen-rust/src/tests/part_7_tests.rs` — string goldens for the
  shared host constructor, the byte-buffer slice/element/write routing,
  `ArrayBuffer.isView`'s view set, the `Buffer` spec tag, namespace interning, and
  the `arguments` object. These run in CI.
* `crates/smelt-codegen-rust/tests/host_representation_runtime.rs` — five
  end-to-end runtime cases (`#[ignore]`d by convention, run with `-- --ignored`)
  that execute the generated programs: only running them proves the records carry
  real bytes, keep distinct identities, and enumerate the right keys.
