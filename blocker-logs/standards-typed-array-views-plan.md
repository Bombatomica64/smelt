# Typed-array views as concrete types: inventory and increment plan

The item has been the standards stream's carried work since round 17 ("complete
or not at all"). This is the measured scope, the one compatibility constraint
that decides the shape of the work, and the increments that can each land
gate-green on their own. It is a plan, not a partial implementation: nothing in
it is half-built in the tree.

## Where the family stands today

Two representations exist, and neither is the target.

**The erased face** (`smelt-stdlib/src/host_object.rs`,
`smelt-codegen-rust/src/byte_buffer_prelude.rs`). The eleven `TypedArray`
views, `ArrayBuffer`, `SharedArrayBuffer`, `DataView` and Node's `Buffer` are
marker-bearing records carried in `SmeltUnknown`: 61 registry marker entries,
11 runtime helpers (`ELEMENTS`, `OWN_ELEMENTS`, `SLICE`, `ELEMENT`,
`SET_ELEMENT`, `IS_VIEW`, `CONSTRUCT`, `INDEX_KEYS`, `RECORD_INDEX_KEYS`,
`RECORD_ELEMENTS`, `BYTES_KEY`), 16 codegen/frontend call sites into those
helpers and 25 frontend sites consulting the registry. This face is what
answers `ArrayBuffer.isView`, `[object Uint8Array]`, element-width decoding,
`Object.keys` over a view, and (since round 21) JSON serialization.

**The concrete stub** (`smelt-codegen-rust/src/text_codec_prelude.rs`).
`SmeltUint8Array` — `StdlibClass::ByteArray`, reached only through the reserved
synthetic name `__SmeltUint8Array` — is an `Rc<RefCell<Vec<u8>>>` plus an id
with `length`, `byteLength`, `to_bytes` and the two erasure adapters. It has no
element kind, no byte offset, no shared buffer, no indexing, no `subarray`, no
iteration. It exists because `TextEncoder.encode` had to answer something
concrete.

A source `Uint8Array` annotation resolves to the ERASED face. That is what
makes `new Uint8Array([1,2])` a host record, why `json_stringify_runtime.rs`
holds four cases the examples corpus cannot, and why Hono's whole crate now
stops at `crypto.subtle.digest`: the digest models a concrete byte view, and
the erased views are not accepted as input.

## The constraint that shapes the work

The erased face cannot simply be deleted. es-toolkit's suite depends on
observing it at run time — `isTypedArray` is `ArrayBuffer.isView(x) && !(x
instanceof DataView)`, `cloneDeep` reconstructs a view through
`Object.getPrototypeOf(x).constructor`, `keys`/`values` enumerate index keys,
deep equality compares elements, and the `[object X]` tag distinguishes the
eleven. Its typed-array surface is 293 source references (78 `ArrayBuffer`, 56
`Uint8Array`, 27 `DataView`, the rest spread across the nine remaining views),
and the member surface a concrete family must answer is `.length`, `.slice`,
`.set`, `.byteLength`, `.buffer`, `.byteOffset`, `.fill`, `.subarray` plus
indexed read/write and iteration.

So the target is not "replace the erased face" but "add a concrete face and
make the erased one its BOUNDARY form", the same shape the codecs, `Headers`,
`URLSearchParams` and `FormData` already have: a concrete value that erases to
the existing marker record byte-for-byte and recovers from it. Anything less
and the 1055/4 es-toolkit result moves.

## Increments

Each one is independently landable and gate-green, and the order is forced by
that constraint.

1. **The concrete family and its prelude.** One runtime type parameterized by
   element kind — `SmeltTypedArray { id, buffer: Rc<RefCell<Vec<u8>>>, kind,
   byte_offset, length }` — plus `SmeltArrayBuffer { id, bytes }` and the
   `DataView` shape. Members: `length`/`byteLength`/`byteOffset`/`buffer`,
   indexed read/write at the element's width and signedness, `subarray`/`slice`
   (offset views over the SAME buffer, which is what makes the shared-buffer
   semantics observable), `set`, `fill`, iteration, `PartialEq` by elements,
   `Debug`, and the `[object <Kind>]` tag. No frontend change yet: the family
   is unreachable from source, so the gates cannot move.
2. **The boundary adapters.** `IntoSmeltUnknown` produces exactly today's
   marker record (marker, `bytes`, `byteLength`, `length`, and the storage
   record a view carries), `SmeltFromUnknown` recovers a view from any
   byte-backed record, and the origin registry keeps identity across the round
   trip the way `Headers` does. Gate: es-toolkit unchanged, because nothing
   reaches the concrete face yet but every erased answer is now produced by the
   adapter under test.
3. **The frontend switch.** `Uint8Array` and its siblings resolve to the
   concrete classes; constructor lowering for every spelling (`new
   Uint8Array(n)`, `(array)`, `(buffer)`, `(buffer, offset, length)`, `(view)`);
   member and method dispatch; indexing; `instanceof`; `ArrayBuffer.isView`;
   `Object.keys`/`values`/`entries` and JSON through the concrete face. This is
   the increment that moves numbers, and it is where es-toolkit's 1055/4 and
   the examples invariant have to be re-measured line by line.
4. **Retire the routed-around arms.** `StdlibClass::ByteArray` folds into the
   family (`TextEncoder.encode` answers a `Uint8Array`, `TextDecoder.decode`
   takes any view), `crypto.getRandomValues` fills a concrete view in place,
   and `crypto.subtle.digest` accepts a view or a buffer — which is Hono's
   current stop.
5. **Fold the runtime tiers back into the corpus.** With views concrete,
   `json_stringify_runtime.rs`'s byte cases and the enumeration cases become
   ordinary fixtures at zero avoidable erasure, and the `avoidable` counts in
   es-toolkit and remeda should FALL; both baselines get re-snapshotted with
   the delta reported.

## Status

* **1 and 2 landed** (round 23): the concrete family and its boundary adapters,
  inert by construction because nothing reached the concrete face from source.
* **3 and 4 landed together** (round 24), as one commit because increment 3
  cannot land alone: `crypto.subtle.digest` is the first consumer of a concrete
  view, so the moment `new Uint8Array(..)` stopped being erased the digest's
  input check had to move with it. What that commit did beyond the list above:
  * the eleven view spellings and `ArrayBuffer` resolve to two modeled classes
    (`StdlibClass::TypedArray` / `ArrayBuffer`), keyed off the shared registry
    rather than a list of names, so `Type::Class { name }` keeps the source
    spelling while the eleven share one Rust type;
  * every constructor spelling dispatches on the ARGUMENT TYPE at the call site
    (a length, an element list, shared storage, another view), with one
    `from_erased` prelude boundary for an argument whose type carries no shape;
  * members, methods, indexed read/write, iteration/spread/`Array.from`,
    `instanceof`, `ArrayBuffer.isView`, `String(view)`, the `[object X]` tag,
    `Object.keys`/`values`/`entries` and `Reflect.ownKeys` all answer from the
    concrete value;
  * `is_erased_class_type`'s hand-maintained list of non-erasing stdlib classes
    became one registry question (`StdlibClass::has_concrete_runtime_type`) —
    the list had silently omitted `ArrayBuffer` and the three `node:http`
    classes, which is an E0308 in generated Rust rather than a warning;
  * the erased record is built by ONE builder shared with the erased face
    (`smelt_host_buffer_view_record_with_id`), and the family retains its live
    value in the host-origin registry so a write through an erased alias reaches
    the storage the concrete value still holds;
  * `crypto.subtle.digest` takes a view or an `ArrayBuffer` and answers the
    spec's `ArrayBuffer`; `Blob.arrayBuffer()` likewise, where `bytes()` stays a
    view; `crypto.getRandomValues` fills a concrete view's own byte window.
  `SharedArrayBuffer` and `DataView` deliberately keep the erased record.
* **5 landed** (round 25). What moved and what did not, and why:
  * `json_stringify_runtime.rs`'s byte-VIEW and byte-STORAGE rows are now
    corpus fixtures (`74_typed_array_views`), which is a stronger test than a
    tier one because it also pins the emitted Rust. Byte storage needed one
    addition to get there: `Object.keys`/`values`/`entries`/`Reflect.ownKeys`
    on a concrete `ArrayBuffer` answer the EMPTY list from the class rather
    than through a record — storage addresses its bytes through accessors, so
    its emptiness is a property of its class and needs no round trip.
  * The reserved synthetic class name is retired. `TextEncoder.encode`,
    `TextDecoder.decode`, `Blob.bytes()` and `crypto` answer a `Uint8Array`
    under that name, so a program can annotate what it gets
    (`const bytes: Uint8Array = encoder.encode(text)`). The synthetic spelling
    existed only while the source spelling still meant the erased record; once
    increment 3 made them the same Rust type it only hid what `encode` answers.
  * What STAYS in the tiers, and will until `DataView` and `SharedArrayBuffer`
    are concrete: an erased view's `instanceof`/`isView`/`String()`/
    `JSON.stringify`, the recovery of a concrete view from an erased buffer, a
    `DataView`'s own-property answers, an `unknown` or generic stringify
    operand, and a union with an erased arm. Every one of those is erased BY
    CONSTRUCTION, so no amount of concreteness in the family moves them; the
    corpus invariant is `avoidable == 0` and they cannot be zero.
  * The baselines did NOT move: the cases that changed home were tier-only, so
    no generated corpus output changed. es-toolkit stays at 32247 (+0), remeda
    at 24884 (+0), the examples invariant at 0, and es-toolkit's suite at
    1055 / 4.

## What is left of the family

`SharedArrayBuffer` and `DataView` are the two byte hosts still modeled as the
erased record. `DataView` is the interesting one: its element width is a
property of each CALL (`getUint16(0)`, `setFloat64(8, x)`) rather than of the
value, so it is the one member of the family whose kind cannot be a field.
Making it concrete means a per-call width parameter on the same shared storage,
which is a smaller job than increments 3+4 were but is its own item.

Measured at 3+4: es-toolkit 1055 / 4 (unchanged, same four failures) with the
ratchet FALLING 32438 → 32247; remeda 1789 / 0 with its advisory report falling
25017 → 24884; radash 84 / 0; the examples invariant still 0.

## Why it is not one round

Increment 3 alone rewrites every one of the 25 frontend registry consultations
and the 16 codegen helper call sites, and each of 3, 4 and 5 needs its own full
gate cycle (es-toolkit transpile + compile + 1059 tests + ratchet, remeda 1789,
radash 84, the corpus invariant, and the Hono crate). Landing 1–2 without 3
changes nothing observable, which is exactly what makes them safe to land
first; landing 3 without 4–5 leaves the corpus fixtures and the routed-around
arms inconsistent for one round but green.

The coordinator approved the increments across rounds 23–25 with one condition:
every commit passes every gate, and no commit leaves the family reachable from
source but half-modeled. That condition is what forced 3 and 4 into one commit.

Hono's stop (`crypto.subtle.digest` on an erased view) is cleared: the whole
crate's remaining diagnostics are 8 in 6 files, and the only standards-owned one
left is `btoa`/`atob` in `src/utils/cookie.ts`.
