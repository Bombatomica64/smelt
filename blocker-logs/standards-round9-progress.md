# Standards round 9: text codecs and the Blob/File upgrade

Date: 2026-09-07. Plan: `blocker-logs/standards-tier-plan.md` §4. Previous
state: `blocker-logs/standards-tier-progress.md`. Demand:
`blocker-logs/hono-fetch-demand.md` §4.

## 1. What landed

| plan item | state |
| --- | --- |
| `TextEncoder` / `TextDecoder` | **landed**, concrete Rust, runtime tier + fixture (section 2) |
| concrete byte view (`SmeltUint8Array`) | **landed** as the value `encode` answers (section 2) |
| `Blob` / `File` upgrade | **landed**, byte-backed concrete Rust, runtime tier + fixture (section 3) |
| `FormData` | not landed; surface and the multipart decision are settled below (section 5) |
| `crypto` (`randomUUID`, `getRandomValues`, `subtle.digest`) | not landed; scoped below (section 6) |
| `AbortController` / `AbortSignal` | not landed; what already exists is inventoried below (section 7) |
| `Optional<T>` string coercion fidelity | not landed, and it is BLOCKED — see section 8 |
| floating-async synchronous prefix | not landed (section 9) |

Three pre-existing defects were found on the way and recorded rather than
folded into a feature item:
`blocker-logs/standards-instanceof-narrowing.md`,
`blocker-logs/standards-module-const-host-value.md`,
`blocker-logs/standards-erased-cast-size-collision.md`. The second one is the
most urgent thing in this file — see section 4.

## 2. `TextEncoder` / `TextDecoder`, and the byte view

Three concrete generated types: `SmeltTextEncoder`, `SmeltTextDecoder`, and
`SmeltUint8Array` (a shared `Vec<u8>` with a JS reference identity). Members
are the members' exact source types — `encode` a byte view, `decode` a
`String`, `length`/`byteLength` `f64`, `encoding` a `String`.

### The byte view has a SYNTHETIC class name, and why

`TextEncoder.encode` returns a `Uint8Array` in the source types, but the
spelling `Uint8Array` still means the byte-backed HOST RECORD that the eleven
typed-array views share (`smelt_stdlib::host_object`). Rebinding the name would
change the meaning of existing programs, and the reason is concrete rather than
cautious:

* a view's runtime identity carries an element type, a byte offset, and a
  shared `ArrayBuffer` that reflective construction
  (`new Object.getPrototypeOf(x).constructor(..)`) reads back — es-toolkit's
  `clone` uses the reflected form where `cloneDeepWith` uses the direct one,
  and its specs compare the two results against each other;
* es-toolkit annotates `Uint8Array` inside erased typed-array UNIONS
  (`src/predicate/isTypedArray.ts`, `src/object/toCamelCaseKeys.ts`), so making
  the name a concrete class would turn those union arms concrete while the
  values flowing through them stay records.

So the concrete view is reached only through
`smelt_stdlib::BYTE_ARRAY_CLASS_NAME` (`__SmeltUint8Array`), and it erases —
through its own `IntoSmeltUnknown` — to exactly the byte-backed record. The
runtime tier proves the consequence: on an erased concrete view,
`erased instanceof Uint8Array` is true and
`Object.prototype.toString.call(erased)` is `[object Uint8Array]`. A concrete
view assigned into a `Uint8Array`-annotated slot crosses the ordinary boundary.

**Demand recorded:** converting the eleven typed-array views from byte-backed
host records to concrete Rust views. It is the change that would let the
spelling `Uint8Array` mean the concrete type, and it needs the whole family at
once (element typing, byte offsets, the shared `ArrayBuffer`, reflective
construction) — not one view at a time, which would be exactly the kind of
special case CLAUDE.md refuses.

### Decisions

* `decode` is `String::from_utf8_lossy`, which IS the spec's non-`fatal`
  behaviour (U+FFFD for ill-formed input). `fatal`/`ignoreBOM` change the
  answer, so `new TextDecoder(label, options)` is a named blocker rather than a
  silently different decoder; so is any label outside the encoding standard's
  UTF-8 rows.
* UTF-8 needs no crate. Rust's `str` is UTF-8, so both directions are in
  `core`; `encoding_rs` would only start paying for itself at the first
  non-UTF-8 label, which the frontend refuses outright.
* `length` and `byteLength` are separate methods even though they agree for a
  one-byte element type, because they are separate spec members.

## 3. `Blob` / `File`

`SmeltBlob`: immutable `Rc<Vec<u8>>` bytes, a MIME type, and optional `File`
metadata. `size`, `type`, `text()`, `arrayBuffer()`, `bytes()`,
`slice(start?, end?, contentType?)`, and `File`'s `name`/`lastModified`.

### One Rust type for two spellings

The spec's `File` is a `Blob` plus two data properties and no behaviour, and
Rust has no inheritance. A `SmeltFile` wrapping a `SmeltBlob` would make every
`Blob`-typed slot need a widening conversion the type system would have to
carry; an optional-name blob makes `file instanceof Blob` free. It is also what
the erased record already did — `__smelt_file` stamped on top of
`__smelt_blob`, one shape — so the concrete value and its erased form describe
the same object. The two `File`-only members are refused on a plain `Blob` at
lowering, where the spelling is still known.

### No `RefCell`

A blob is immutable in the spec: `slice` answers a new blob and there is no
mutator on the interface. The bytes sit behind a plain `Rc<Vec<u8>>` — shared,
never borrowed mutably. The `Rc` is for cheap cloning of a possibly large
buffer, not for interior mutability. This is the first modeled reference type
in the tier that does NOT need a cell, and saying why is what stops the next
one from copying the `Rc<RefCell<..>>` shape by habit.

### `size` was accidentally right and everything else was wrong

The old record stored its content as a `String` and reported `content.len()`.
That is a byte count, so `size` was right; but a blob's content is arbitrary
bytes, and `arrayBuffer()`/`bytes()` have to answer them exactly. The concrete
type stores `Vec<u8>` and decodes only at `text()`, which is the direction the
spec defines. The runtime tier asserts a `Uint8Array` part containing `0xFF`
survives, which the `String`-backed record could not hold at all.

### `BlobPart` keeps its type where it has one

`BlobPart` is `Blob | BufferSource | string`. Two of the three arms are modeled
concretely, so a parts array typed `List<String>` or `List<Blob>` goes straight
to `from_string_parts` / `from_blob_parts`, and only a genuinely heterogeneous
array — mixed arms, or a `BufferSource`, still the erased byte-backed record
family — is erased and walked at runtime by `smelt_blob_parts_bytes`.

That erased walk is the one real dynamic boundary in the blob surface, and it
shrinks to nothing once the typed-array views become concrete. Keeping the
common shapes concrete is also what keeps the examples invariant at 0 avoidable
erasure: erasing every parts array put **16 avoidable lines** in the new
fixture, and the invariant is what forced the better design.

### The erased record has one definition

`smelt_blob_record` is called both by `impl IntoSmeltUnknown for SmeltBlob` and
by the reflected host constructor, so a directly-constructed blob and a
reflectively-constructed one cannot drift. It is byte-identical to the record
the pre-concrete helper built, which is what leaves es-toolkit's
`cloneDeepWith` arm (`value instanceof Blob`, then
`new Blob([value], { type: value.type })` over an `unknown`) working unchanged.
The runtime tier asserts that idiom directly.

### A `Blob` body

`new Response(blob)` now extracts a real body and carries the blob's `type` as
the body's content type, as the spec's "extract a body" step does. One
`BodyInit` arm closed; `FormData`, `URLSearchParams` and `ReadableStream`
remain named blockers there.

## 4. The most urgent thing found: a module-level `const` does not reach its readers

`blocker-logs/standards-module-const-host-value.md`. A module-level `const`
holding a modeled host value, read from ANY function, lowers to an empty
erased-record default and the generated crate **fails to compile**. It affects
every modeled host class — `Headers`, `URLSearchParams`, `Response`, `Request`,
and now the codecs and `SmeltBlob`.

This should be the next round's first item, ahead of any new type. It is the
commonest shape in the Hono corpus for exactly the types this stream is
building: `hono-fetch-demand.md` records `TextEncoder` used "36× as
`new TextEncoder().encode(…)`, **5× via a bound `encoder`**", and a bound
encoder at module scope is this bug. The inline spelling works; the bound one
stops the build. Landing `FormData` or `crypto` before fixing it just adds more
types that cannot be hoisted.

## 5. `FormData`: the surface and the multipart decision, settled

Demand (`hono-fetch-demand.md` §4): `append` 26, `get` 4, `forEach` 4,
constructed 4 in `src/`, 50 across the corpus. Plus
`Request.formData()`/`Response.formData()`.

**Shape.** The same ordered pair list `SmeltHeaders`/`SmeltUrlSearchParams`
already use, because a `FormData` IS that: an insertion-ordered list of
name/value entries with case-SENSITIVE names and duplicate names allowed. The
one difference that matters is the value type: an entry value is
`string | Blob` (a `File` when the part had a filename), which is now a real
two-arm union of modeled types — `String` and `SmeltBlob` — rather than
anything erased. That is the whole reason to land it after `Blob` and not
before.

    struct SmeltFormData { id: usize, entries: Rc<RefCell<Vec<(String, SmeltFormDataValue)>>> }
    enum SmeltFormDataValue { Text(String), File(SmeltBlob) }

`get` answers `Optional<union>`, `getAll` a `List<union>`, `has` a `bool`,
`set`/`append`/`delete` `None`, `entries` a `List<(String, union)>`. `append`
with a third `filename` argument replaces the blob's name, which is the spec's
own step and free with the optional-name blob.

**Multipart parsing: hand-write it, do not take a crate.** The plan left this
open ("`multer`-free: use the `multipart` crate or hand-parse; justify the
pick"). The pick is a hand parser, and the reason is that every crate in this
space is built for the wrong shape:

* `multer` is `async` over a `Stream<Item = Result<Bytes, E>>` and pulls in
  `futures`, `bytes`, `httparse`, `encoding_rs`, `spin`. `Request.formData()`
  parses a body Smelt has ALREADY buffered (`SmeltBody` is bytes or a chunk
  list), so the whole streaming apparatus is dead weight, and its error type
  would have to be mapped into the generated crate's one error channel anyway.
* `multipart` (0.18) is `std`-blocking and built around `hyper` 0.10 /
  `iron` server types.
* `formdata`/`multipart-rs` are unmaintained.

What is actually needed is RFC 7578 over a `&[u8]` with a known boundary: split
on `--boundary`, parse each part's headers (only `Content-Disposition` — for
`name` and the optional `filename` — and `Content-Type`), take the bytes
between the blank line and the next boundary, and stop at `--boundary--`. That
is ~60 lines in the prelude, has no dependency, and is testable against Node
byte-for-byte. `application/x-www-form-urlencoded` bodies reuse
`SmeltUrlSearchParams::from_query`, which already exists — one line.

Write the boundary extraction as its own documented helper: it comes from the
`Content-Type` header's `boundary` parameter, and getting it wrong is the one
failure mode that produces a silently EMPTY `FormData` rather than an error.

## 6. `crypto`: what is in scope and the crate picks

Demand: `randomUUID` 3, `getRandomValues` 1, `subtle.digest` 4 in scope;
`subtle.importKey` 19, `generateKey` 13, `exportKey` 11, `sign`/`verify` 2 each
are in `src/utils/jwt/**` and `src/middleware/{jwt,jwk}`, which the campaign
excludes this round and which must stay a NAMED BLOCKER so those files stay
excluded honestly.

* `randomUUID` -> `uuid` with the `v4` feature, pay-for-use. The alternative
  (formatting 16 `getrandom` bytes by hand) is 20 lines and no dependency, and
  is worth preferring if `uuid` is the only user of its own dependency tree —
  check `Cargo.lock` before deciding, and say which in a comment at the
  `Cargo.toml` emit site.
* `getRandomValues(view)` -> `getrandom`. Note it MUTATES its argument in place
  and returns it, so it needs a byte view whose bytes can be written. The
  concrete `SmeltUint8Array` from section 2 holds its bytes behind
  `Rc<RefCell<Vec<u8>>>` — deliberately, unlike `SmeltBlob` — so this works;
  a `Uint8Array` HOST RECORD argument does not, and that asymmetry is the
  reason to route `getRandomValues` through the concrete view and make a record
  argument a named blocker rather than silently filling a copy.
  There is already a `crypto_get_random_values_call` handler in the call
  dispatch chain: check what it currently does before adding a second path.
* `subtle.digest(alg, data)` -> `sha1` for SHA-1, `sha2` for SHA-256/384/512,
  both pay-for-use and both familiar. It is `async`, so the result is a
  `Future<byte view>`; the algorithm argument is a literal in every corpus use,
  so select the hasher at emit time and make a non-literal algorithm a named
  blocker — a runtime dispatch over four hashers would carry all four crates
  into every crate that hashes anything.

## 7. `AbortController` / `AbortSignal`: what already exists

Do NOT start from scratch. Already in tree:

* `abort_controller_constructor_expression` in `lowering/new_expr.rs`, reached
  from `new_expr.rs` for the name `AbortController`;
* `__smelt_abortcontroller` / `__smelt_abortsignal` markers in the host-object
  registry, with `[object AbortController]` / `[object AbortSignal]` tags;
* a runtime tier `crates/smelt-codegen-rust/tests/abort_signal_runtime.rs`.

So the round-9 shape of this item is an UPGRADE of a marker record to a
concrete type, exactly like `Blob` was — read the existing tier first, and keep
every case in it green. The plan's open question ("`CancellationToken` or a
small `Rc<Cell>`; pick the simplest that works in the single-threaded `Rc`
runtime") answers itself once the listener list is considered: `signal.aborted`
is an `Rc<Cell<bool>>`, `signal.reason` an `Rc<RefCell<Option<SmeltUnknown>>>`
(the reason is any JavaScript value — a genuine boundary), and
`addEventListener('abort', ..)` is the SAME insertion-ordered listener list
`SmeltEventEmitter` already has. Compose the emitter rather than growing a
second listener store, the way `SmeltIncomingMessage` does.
`AbortSignal.timeout(ms)` needs a timer, which the runtime already has.

## 8. The `Optional<T>` string coercion is BLOCKED on `Type::Null`

The round's item 6 asked to fix "interpolating an `Optional<T>` into a template
prints `""` where JS prints `"null"`/`"undefined"`". Reproduced, four affected
coercion sites, and it cannot be fixed as stated. Repro:

```ts
function pick(flag: boolean): string | null { return flag ? "value" : null; }
const absent = pick(false);
console.log(`[${absent}]`);      // Node: [null]        Smelt: []
console.log("concat: " + absent); // Node: concat: null  Smelt: concat:
console.log(String(absent));      // Node: null          Smelt: (empty)
const n: number | null = null;
console.log(`n=${n}`);            // Node: n=null        Smelt: n=
```

All four emit `unwrap_or_default()`, and the last one is even constant-folded to
`String::new()`.

The blocker is that JS prints `"null"` for `null` and `"undefined"` for
`undefined`, and Smelt cannot tell them apart: `string | null` and
`string | undefined` both lower to `Optional(String)`. Choosing one spelling
would be silently wrong half the time, which is not better than being silently
wrong all of the time — it is worse, because it would look fixed.

This wants `Type::Null`, which is already scoped as one of the three deferred
es-toolkit families (see the round-8 plan note "the three deferred families
(`Type::Null`, Optional index reads, tagged-realm constant evaluation)"). The
coercion fix is then four sites: the template-interpolation operand, the string
`+` operand, `String(x)`, and `console.log` of an optional (which is already
handled separately and correctly via `console_value_text`'s `absent.text()` —
worth reading, because it is the one site that already carries the absent
SPELLING and shows what the other three need).

## 9. Not attempted this round

* the floating-async synchronous prefix (`from_future_primed` exists; the drain
  runs the whole call rather than only its suspension);
* `for await` / the async-iterator protocol, and `ReadableStream` beyond a
  declared surface — explicitly out of scope for round 9. Demand for them from
  Hono is `getReader` 1 / `pipeTo` 1 in `src/`, with the real users
  (`src/helper/streaming/**`, `src/utils/stream.ts`) already excluded, so they
  stay low priority next to section 4.

## 10. Gate numbers

* examples end-to-end: 10/10 (`hir_cli_cross_language_tests`), two new fixtures
  `45_text_codec` and `46_blob_file`, both byte-identical to Node 22.
* `smelt-frontend-ts`: 1070 passed / 0 failed.
* `smelt-codegen-rust` lib: 1001 passed / 0 failed.
* `smelt` bin: 50 passed / 0 failed.
* runtime tiers: `text_codec_runtime` 5/5, `blob_runtime` 6/6,
  `fetch_types_runtime` 8/8 (all `--ignored`; the two new tiers are registered
  in the `standards` shard of `runtime-tiers.yml`).
* examples SmeltUnknown invariant: avoidable erasure 0, unchanged; prelude
  re-snapshotted (+2059 over the round, the three new runtime types).
* es-toolkit ratchet: avoidable 32488 -> **32416** (-72), legitimate
  38989 -> 38961 (-28), prelude 2592 -> 2623 (+31). Re-snapshotted.
* remeda: 1787 passed / 2 failed — `groupByProp` symbol grouping. **Verified
  pre-existing**: identical result from a `smelt` built at the stream head
  (`80663c62`) on the same checkout, so the brief's "1789 / 0" is stale rather
  than a regression from this round.
