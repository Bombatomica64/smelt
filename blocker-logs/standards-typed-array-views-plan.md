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

## Why it is not one round

Increment 3 alone rewrites every one of the 25 frontend registry consultations
and the 16 codegen helper call sites, and each of 3, 4 and 5 needs its own full
gate cycle (es-toolkit transpile + compile + 1059 tests + ratchet, remeda 1789,
radash 84, the corpus invariant, and the Hono crate). Landing 1–2 without 3
changes nothing observable, which is exactly what makes them safe to land
first; landing 3 without 4–5 leaves the corpus fixtures and the routed-around
arms inconsistent for one round but green.

The item's standing instruction is "complete or not at all". Under that rule
this round produced the plan rather than a third of the feature. What I need
from the coordinator is which of these two:

* **approve the increments** as five commits across rounds 22–24, with 1–2
  landing immediately (they are inert by construction), or
* **keep the all-or-nothing rule** and give the feature a whole round with
  nothing else in it, in which case increments 1–5 land as one commit and the
  first gate run happens at the end.

Either way the surface above is the contract, and Hono's stop
(`crypto.subtle.digest` on an erased view) is increment 4.
