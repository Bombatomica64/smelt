# UTF-16 JavaScript strings in generated Rust

Status: implemented in steps 0-7 (step 8, performance, was not needed); see "Implementation notes" and "Acceptance results"
at the end for where the implementation departs from this design. Base: `origin/rw2-wrong-answers` at `f089c1eb`. That is
the first branch carrying the string-index seam (`c5d98dce`). Every `file:line` below refers
to that tree.

Decision already taken by the user: a JavaScript string in generated Rust is stored as
UTF-16 code units, as V8 stores it. This document says how. It does not reopen whether.

Python `str` is **not** moved to UTF-16. Python indexes by code point, and a hand-written
Python port keeps `String`. Python is still touched for one reason: mixed TypeScript and
Python crates exist. A Python entry can import a TypeScript function
(`crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs:14`, `:259`). Today both
languages lower `string`/`str` to the same `Type::String` (`crates/smelt-hir/src/type_normalize.rs:268`),
and `crates/smelt-codegen-rust/src/emitter/types.rs:291` renders it as `String`. Once the
two languages store strings differently, the type has to tell them apart (§3, step 0).

## 0. The problem, measured

The `smelt_utf16_*` helpers (`crates/smelt-runtime/src/runtime/string_units.rs:18-140`) make
every TypeScript index count UTF-16 code units over a UTF-8 `String`. The answers are right,
but each index seeks from the start of the string: `smelt_utf16_byte_of` at `:57` costs
O(position), with an ASCII fast path at `:59`. So a loop that indexes across a non-ASCII
string is quadratic. Numbers from `git log -1 c5d98dce` (release builds, ms over 20 rounds):

| phase                                 | UTF-8 + UTF-16 rule (today) | Node |
|---------------------------------------|-----------------------------|------|
| charCodeAt scan, 4000 ASCII chars     | 2                           | 0    |
| charCodeAt scan, 4000 non-ASCII chars | 42                          | 0    |
| indexOf+slice words, 88 KB ASCII      | 655                         | 5    |
| indexOf+slice words, 88 KB non-ASCII  | 16476                       | 3    |
| s[i] for i < 2000, ASCII              | 1                           | 1    |
| s[i] for i < 2000, non-ASCII          | 11                          | 0    |

The second defect is that a lone surrogate cannot exist. `"😀".slice(0, 1)` is `"😀"` in
Smelt (`string_units.rs:10-16`) and `"\ud83d"` in Node. A literal containing a lone surrogate
is replaced by U+FFFD in the frontend (`crates/smelt-frontend-ts/src/lowering/js_text.rs:26-37`).

A third defect, found while writing this document: **ordering**. Rust's `str` compares by
code point. JavaScript compares by code unit. Node gives
`["￿","😀","a","é"].sort()` → `a, é, 😀, ￿` and `"😀" < "￿"` → `true`. A
`Vec<String>::sort()` (emitted at `crates/smelt-codegen-rust/src/emitter/list_ordering.rs:46`)
puts U+FFFF before U+1F600. UTF-16 storage fixes this for free: derived `Ord` on `[u16]` is
JavaScript's order. I have not run a generated crate to confirm today's wrong output; the
fixture in §4 will.

Node behaviour this design targets (checked with `node` on this machine):

```
console.log("\ud83d")          -> bytes EF BF BD (U+FFFD); stdout is UTF-8, lossy
console.log(["\ud83d"])        -> [ '\ud83d' ]    (inspect escapes a lone half, lowercase hex)
JSON.stringify("\ud83d")       -> "\ud83d"        (well-formed stringify, ES2019)
"😀".slice(0,1) === "\ud83d"   -> true;  "😀".slice(0,1) + "😀".slice(1) === "😀" -> true
`${"\ud83d"}x`.length          -> 2
encodeURIComponent("\ud83d")   -> throws URIError
new TextEncoder().encode("\ud83d") -> [239, 191, 189]
"\ud83d".toUpperCase() === "\ud83d" -> true
"😀😀".replace(/\ud83d/g, "X") -> "X\ude00X\ude00"  (a non-`u` regexp matches code units)
"😀".split("").length          -> 2
new Map([["\ud83d",1]]).get("😀".slice(0,1)) -> 1
Buffer.from("\ud83d").toString("hex") -> efbfbd
```

## 1. Representation

### Options

| option | lone surrogates | clone | `s += x` in a loop | hash / order | fit |
|---|---|---|---|---|---|
| `widestring::Utf16String` | **rejected** (validated UTF-16) | deep, O(n) | amortised O(1) | code unit | wrong semantics |
| `widestring::U16String` | yes | deep, O(n) | amortised O(1) | code unit | a `Vec<u16>` with lossy conversions; no sharing |
| `boa_string::JsString` (0.22, Boa engine) | yes | O(1), refcounted | **O(n) per append**: immutable, concat allocates | code unit | right semantics, wrong cost for the emitted `+` |
| own `JsString` over `Rc<[u16]>` | yes | O(1) | O(n) per append | code unit | same append problem |
| **own `JsString` over `Rc<Vec<u16>>`, appending in place when uniquely owned** | yes | O(1) | amortised O(1) | code unit | recommended |
| hybrid Latin-1 bytes / UTF-16 (V8 one-byte, Boa) | yes | O(1) | depends | code unit | halves ASCII memory; every operation branches on width |

### Why append decides it

The emitter turns `text += x` into `tmp = text + &x; text = tmp;`. Both operands go through
`string_like_operand_expr`, and the `+` is built at
`crates/smelt-codegen-rust/src/emitter/binary_ops.rs:1360`
(`format!("{lhs_text} + &{rhs_text}")`). You can see it in
`examples/typescript/end-to-end/123_string_from_codes_and_search/expected.rs:612-613`, Hono's
`encodeBase64` loop. `text` is moved into the `+`. With `String` that is an in-place
`push_str`, so the loop is linear.

An immutable shared string (`boa_string`, `Rc<[u16]>`) must copy on every `+`, and that loop
becomes quadratic. That is the same class of regression this change is meant to remove. V8
avoids it with ropes (ConsString). A rope is more machinery than we need, because the emitted
code already hands `+` an owned left operand: `Rc::make_mut` on an `Rc<Vec<u16>>` appends in
place when the count is 1, and copies only when the string is really shared. That is the
standard-library idiom for copy-on-write, and it gives:

- clone O(1). Today it is O(n): generated code clones strings freely.
- append amortised O(1) when the left side is uniquely owned, which a moved local is.
- one extra pointer hop on reads (`Rc` box → `Vec` header → buffer). A thin-header
  allocation can remove it later behind the private field, without changing generated code.

### Why not Latin-1 now

V8's one-byte strings save half the memory for ASCII text. The cost is a width branch in every
operation, regexp running over two input types, and two code paths to test. The recommended
type keeps its storage private: generated code only calls `JsString` methods and `&JsStr`
(= `&[u16]` through `Deref`). So a Latin-1 variant can be added inside the type later, if a
memory benchmark asks for it, with no emitter change. Measure first: the corpora are
small-string heavy, and the per-string header (16-byte `Rc` counts + 24-byte `Vec`) dominates
the doubling for strings under ~40 characters.

### Recommendation

An own runtime item, `JsString`, in a new `crates/smelt-runtime/src/runtime/js_string.rs`:

```rust
/// A JavaScript string: immutable UTF-16 code units, shared by reference count.
/// Lone surrogates are representable; ordering and equality are by code unit,
/// which is ECMAScript's order.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsString(Rc<Vec<u16>>);
pub type JsStr = [u16];                       // the borrowed view, like `&str`

impl Deref for JsString { type Target = [u16]; }   // `&s` is `&JsStr`
impl Borrow<[u16]> for JsString {}                 // map lookups by `&JsStr`
impl From<&str> / From<String> / From<char> for JsString   // encode_utf16, O(n)
impl JsString {
    fn from_units(units: &[u16]) -> Self;     // literals with lone surrogates
    fn from_ascii(text: &str) -> Self;        // number formatting: widen bytes
    fn len(&self) -> usize;                   // JS `length`, O(1)
    fn unit_at(&self, i: usize) -> Option<u16>;          // charCodeAt, O(1)
    fn char_at(&self, i: usize) -> Option<JsString>;     // s[i], one unit, O(1)
    fn code_point_at(&self, i: usize) -> Option<u32>;    // pair or lone unit
    fn slice(&self, start: usize, end: usize) -> JsString; // copies end-start units
    fn index_of(&self, needle: &JsStr, from: usize) -> Option<usize>;
    fn last_index_of(&self, needle: &JsStr, from: usize) -> Option<usize>;
    fn push_str(&mut self, other: &JsStr);    // Rc::make_mut + extend
    fn is_well_formed(&self) -> bool;
    fn to_string_lossy(&self) -> String;      // lone half -> U+FFFD (char::decode_utf16), O(n)
    fn chars_lossless(&self) -> impl Iterator<Item = Result<char, u16>>; // for case/normalize
}
impl Add<&JsStr> for JsString { /* push_str on self: in place when unique */ }
impl Display for JsString { /* lossy, ASCII fast path; what console.log prints */ }
impl Debug for JsString { /* quoted, lone halves as \uXXXX: what assert_eq! shows */ }
impl PartialEq<str> / PartialEq<&str> for JsString {}  // tests and runtime keys
pub trait ToJsString { fn to_js_string(&self) -> JsString; } // f64, i64, bool, JsString, SmeltUnknown
```

Transcoding uses only the standard library (`str::encode_utf16`, `char::decode_utf16`). The
type is about 300 lines over `Rc<Vec<u16>>`. That is less custom machinery than the 140 lines
of SWAR seek code it replaces, and every method is the obvious one. CLAUDE.md's preference for
well-known crates is honoured where a crate does real work:

- **`regress`** (the ECMAScript regex engine Boa uses), with its `utf16` feature, replaces
  `fancy-regex` + `regex` (`crates/smelt-stdlib/src/deps.rs:107`). See §2 regexp.
- **`memchr`** (optional, step 8): `memchr::memmem` over the byte view of a `&[u16]`
  (accepting only even offsets) for long-needle `indexOf`.

`boa_string` is the reference for naming and semantics (`JsString`/`JsStr`, `CodePoint` =
scalar or lone surrogate). It is not a dependency, because its strings are immutable, which
makes the emitted `+` quadratic. It would also add six dependencies to every crate
(fast-float2, itoa, pastey, rustc-hash, ryu-js, static_assertions; per docs.rs 0.22.0).

How a hand-written port reads: `let greeting: JsString = JsString::from("hello");`,
`name.len()`, `text.slice(a, b)`, `out = out + &piece`, `println!("{greeting}")`. That is what
a team porting a JavaScript library with strict UTF-16 semantics would write.

## 2. Boundaries

"Cost" is per crossing. n = string length in code units.

| boundary | today (file:line) | after | cost |
|---|---|---|---|
| **string literal** | `RustExpr::string_literal` → `"x".to_owned()` (`crates/smelt-codegen-rust/src/rust.rs:198`, used by `emitter/literals.rs:29`) | `JsString::from("x")`. A literal holding a lone surrogate emits `JsString::from_units(&[0xD83D])` | O(n) allocation + widening, the same order as today's `to_owned` |
| **lone-surrogate literal reaching MIR** | lossy at `crates/smelt-frontend-ts/src/lowering/js_text.rs:26`; `Constant::String(String)` at `crates/smelt-mir/src/types.rs:700` | add `Literal::StringUnits(Box<[u16]>)` / `Constant::StringUnits(Box<[u16]>)`, produced **only** when the `JSStr` is not valid UTF-8 (`value.as_str()` is `None`); every other literal keeps `String` | build time only |
| **`console.log` / printing** | `println!("{format}", …)` at `emitter/call.rs:590` | unchanged text; `Display for JsString` prints lossily, which is exactly Node's UTF-8 stdout | O(n) transcode into the formatter (no `String` built) |
| **console inspect** (`[ 'x' ]`, objects) | `crates/smelt-runtime/src/runtime/console_inspect.rs` (65 sites), quoted strings built char by char | walk units; a lone surrogate prints `\udXXX` (lowercase hex, as Node does) | O(n) |
| **`String(x)`, template literals, `+`** | templates lower to `+` concatenations (`104_…/expected.rs:4248`: `"flag=".to_owned() + &tmp.to_string()`); `+` at `binary_ops.rs:1360`; non-string operands through `.to_string()` (81 emitted `.to_string()` sites) | `JsString + &JsStr`; non-strings through `ToJsString::to_js_string()` | append amortised O(1) when the left operand is unique |
| **number → string** | `smelt_number_to_string` → `String` (`crates/smelt-runtime/src/runtime/number_format.rs:53`, `:90`) | keep the `String` algorithm, wrap in `JsString::from_ascii` (digits are ASCII) | O(digits) widen |
| **string → number** | `crates/smelt-runtime/src/runtime/string_to_number.rs` over `&str` | parse straight from units (ASCII grammar). A non-ASCII unit is whitespace or NaN, so no transcode is needed | O(n) |
| **`JSON.stringify`** | `serde_json::to_string` at `emitter/map.rs:1124`, `emitter/bigint.rs:366`; `impl Serialize for SmeltUnknown` at `runtime/json_serde.rs:6` (`serialize_str` at `:21`) | `impl Serialize for JsString` = `serialize_str(&self.to_string_lossy())`. Exact for well-formed text. **Gap:** serde's data model cannot carry a lone surrogate, so `JSON.stringify("\ud83d")` gives `"�"`-as-char instead of the escape `"\ud83d"`. Closed in step 7 with a JSON writer over `JsStr` | O(n) transcode |
| **`JSON.parse`** | `serde_json::from_str::<SmeltUnknown>` at `runtime/thrown.rs:200`; typed decoding in `emitter/json_decode.rs` | decode from `to_string_lossy()` input; `impl Deserialize for JsString` via `visit_str` → `From<&str>` | O(n) each way. Lone `\udXXX` escapes in the input: today's behaviour carries over (I did not verify whether serde_json rejects them or replaces them); step 7 |
| **regexp** | `fancy_regex` over UTF-8 with a JS→Rust pattern translator (`crates/smelt-stdlib/src/js_regex.rs`, 326 lines; cache at `runtime/regexp.rs:28`); match bytes mapped back through `smelt_utf16_index_of_byte` (`runtime/regexp_match.rs:65-66`, `runtime/regexp.rs:240`, `:251`) | **`regress`** with `utf16`: compile the JavaScript pattern as written (no translation); match `&[u16]` with the UCS-2 entry for non-`u` regexps (code units, like `/\ud83d/` above) and the UTF-16 entry for `u`/`v` regexps. Indices are code units, so `index`, `lastIndex`, captures and `replace` slicing need no mapping. Boa calls these `find_from_ucs2` / `find_from_utf16`; **check the exact names on docs.rs with all features enabled**, since the default docs page does not list them | O(match). Today it is O(match) plus an O(prefix) index map per match |
| **regexp, interim if `regress` slips** | — | transcode haystack to UTF-8 once per `exec`/`replace` call, keeping a unit-offset table | O(n) per call. A global `exec` loop becomes O(n·matches): do not ship the flip with this unless step 5 is scheduled |
| **`toUpperCase`/`toLowerCase`** | `.to_uppercase()` on `&str` (`emitter/strings.rs:11-17`) | ASCII fast path in units. Otherwise split into well-formed runs (lone halves pass through unchanged, as in Node) and call `str::to_uppercase` on each run, so special casing and final-sigma rules stay the standard library's | O(n) + run transcode |
| **`normalize`** | `unicode_normalization` over `.as_str()` (`emitter/strings.rs:40-53`) | same, over well-formed runs; lone halves pass through | O(n) |
| **`localeCompare`** | `smelt_locale_compare(&str, &str)` (`runtime/strings.rs:37`) | decode both through `chars_lossless`; a lone half compares as its unit value | O(n) |
| **`Intl.*`** | number/date formatting output | output strings → `JsString::from` | O(output) |
| **`encodeURI(Component)` / `decodeURI`** | `runtime/uri.rs:16`, `:46`, `:79`, `:150` over `&str` | walk units. A lone surrogate **throws `URIError`** (Node does); today it cannot occur | O(n) |
| **`btoa` / `atob`** | Latin-1 handling over code points (`runtime/thrown.rs:275-291`) | natural over units: `btoa` rejects a unit > 0xFF | O(n) |
| **`String.fromCharCode` / `codePointAt` / `split("")`** | seam at `emitter/string_units.rs`, `runtime/string_units.rs:94-138` | O(1)/O(n) unit operations; halves are real values | O(1) per index |
| **Map/Set keys, `Record<string,V>`** | `SmeltRecord<String, V>` (`emitter/types.rs:325-327`); `SmeltPrimSet<String>` (`runtime/prim_set.rs:37`); `impl SmeltJsKeyEq for String` (`runtime/key_eq.rs:24`); strict-eq impl (`:44`); `SmeltPropertyKey` (`runtime/record.rs:95`) | the same containers at `JsString`; derived `Hash`/`Eq` over units; `js_key_hash` = hash of units; `Borrow<[u16]>` lets lookups pass `&JsStr` | hash O(n) over 2n bytes, roughly 2× today's byte hashing. Key clone O(1) (today O(n)) |
| **`SmeltUnknown::String`** | payload `Rc<str>` (`runtime/unknown.rs:24`); erasing copies (`SmeltUnknown::String(value.into())`, `rust.rs:422-428`); `length` via `smelt_utf16_len` at `unknown.rs:116` | payload `JsString`. Erase is a move/`Rc` clone; extract is a clone | **O(1)** instead of O(n) |
| **erased objects (`SmeltObject`)** | keys `String` (`runtime/object.rs:8`) | step 6: keys become `JsString` (`Object.keys`, `for…in`, computed `o[k]` all hand out or take JS strings). Until then convert at those three boundaries | O(key length) per crossing until step 6 |
| **struct fields named by source properties** | Rust identifiers | unaffected | — |
| **`process.env`** | `::std::env::var(name).ok()` (`emitter/rvalue.rs:188`) | `var_os` → lossy UTF-8 → `JsString` (Node decodes environment bytes lossily too) | O(n) |
| **`fs`** | `std::fs::read_to_string` / `fs::write` (`emitter/strings_io.rs:453`, `:468`) | read: `JsString::from(String)`. Write: `to_string_lossy()` (Node writes a lone half as EF BF BD). The `:468` arm returns `contents.len()`; make it the byte count that was written, as now | O(n) |
| **`TextEncoder`/`TextDecoder`** | `encode(&str)` → `as_bytes` (`runtime/text_codec.rs:71`); `decode` → `from_utf8_lossy` (`:121`) | encode: lossy UTF-8 of units (Node: `[239,191,189]`); decode: `from_utf8_lossy` then `JsString::from` | O(n) |
| **`URL`, `fetch`, `Request`/`Response`, headers, `FormData`, HTTP server, crypto** | `url::Url::parse(&str)` (`runtime/url.rs:69`); `runtime/fetch_types.rs` (193 string sites), `form_data.rs` (65), `http_server.rs` (57), `crypto.rs` (60) | **host internals keep `String`**: their data model is UTF-8 bytes on the wire. Convert at every JS-visible getter/setter/argument (`JsString::from` out, `to_string_lossy` in). The migration then touches accessors, not internals | O(n) per crossing; these strings are short (URLs, header values) except bodies |
| **Python interop** (mixed crates) | one `Type::String` for both | `Type::PyStr` (Python) renders `String`; `Type::String` (TS) renders `JsString`; the coercion seam (`emitter/coercion.rs:124` `value_at_type`, used for call arguments at `emitter/call.rs:483` and `emitter/static_call_args.rs:337`) gains a `String ↔ PyStr` arm: `JsString::from(&s)` / `s.to_string_lossy().into_owned()` | O(n) per call across languages. A lone half becomes U+FFFD in Python: a Rust `String` cannot hold it, which is today's Python limitation, not a new one |
| **generated `#[test]`s** | emitted at `emitter/function_item.rs:77` | `assert_eq!` over `JsString` uses `PartialEq<&str>` and `Debug` (lone halves shown as `\uXXXX`) | — |
| **`smelt_utf16_*` seam** | `runtime/string_units.rs` (140 lines), `emitter/string_units.rs` TS arms | TS arms become `JsString` method calls (`.len()`, `.unit_at(i)`, `.slice(a,b)`); the runtime file is deleted. Python arms (`chars()`) stay | O(1) index, O(k) slice |

## 3. Where it lands

### Measured size

Counts were taken with grep and a literal-aware script over `f089c1eb`, excluding test
modules (`#[cfg(test)]` tails, `tests/`, `*_tests.rs`). "Emitted" means occurrences inside
Rust string literals in the emitter, i.e. text that becomes generated code.

| what | count |
|---|---|
| `Type::String` matches/constructions, codegen-rust | 293 in 53 files |
| `Type::String`, frontend-ts (unchanged by this design) | 378 in 71 files |
| `Type::String`, frontend-py (step 0 changes these to `Type::PyStr`) | 72 in 15 files |
| `Type::String`, hir / mir / transpiler | 9 / 4 / 5 |
| emitted string-API spellings (`String`, `&str`, `.to_owned()`, `.to_string()`, `.as_str()`, `.chars()`, `smelt_utf16_`) in codegen-rust | 495 in 63 files; top: `emitter/strings.rs` 98, `coercion/extract.rs` 35, `fetch_types.rs` 29, `string_units.rs` 26, `strings_io.rs` 23, `crate_builder.rs` 17, `place.rs` 16, `primitive_cast.rs` 16 |
| … of which emitted `.to_owned()` / `.to_string()` / `SmeltUnknown::String` | 121 / 81 / 79 |
| runtime items (`@smelt:item` markers) | 366 |
| runtime code matches for the same pattern set | 1641 in 56 files; top: `fetch_types.rs` 193, `thrown.rs` 144, `url.rs` 80, `to_string_tag.rs` 72, `console_inspect.rs` 65, `form_data.rs` 65, `crypto.rs` 60, `http_server.rs` 57, `regexp.rs` 54, `bigint.rs` 52, `erased_method.rs` 52 |
| runtime fns taking `&str` / returning `String`-ish | 206 / 153 |
| `SmeltUnknown::String` in runtime code | 274 |
| `smelt_utf16_` call sites outside the seam (runtime) | 15 (`erased_method.rs` 6, `property_access.rs` 4, `regexp.rs` 2, `regexp_match.rs` 2, `unknown.rs` 1) |
| committed TS goldens that spell `String` | 168 of 169 `expected.rs` |

Many runtime sites keep compiling unchanged once `JsString: From<&str> + Display + PartialEq<str>`
(for example `SmeltUnknown::String("TypeError".into())`). The host-internals rule above
removes most of `fetch_types`/`url`/`form_data`/`http_server`/`crypto` from scope. A realistic
edit count is about 500 emitter spellings and 400–600 runtime lines. That is an estimate; the
compiler will give the exact list at step 4.

### Emitted types

- `Type::String` (TypeScript) renders `JsString` at the one lowering entry
  (`emitter/types.rs:291`). That covers signatures, struct fields, `SmeltList<JsString>`,
  `Option<JsString>`, union arms and generic arguments, with no other site changed.
- Borrowed parameters stay whatever the emitter picks today; `&str` positions become
  `&JsStr` (`&[u16]`).
- `SmeltRecord<String, V>` (`types.rs:327`) → `SmeltRecord<JsString, V>`.
- `rust.rs:561` (the reserved-name list) gains `JsString`.
- `Type::PyStr` renders `String`. Every Python path is byte-identical.

### Migration order (each step is shippable: tests green, stdout unchanged)

0. **Split the Python string type.** Add `Type::PyStr` to `smelt_hir::Type`. Make the Python
   frontend intern it (72 sites). Add `Type::is_text()`. Then audit the 293 codegen
   `Type::String` sites: string-generic logic matches `is_text()`, JS-only logic keeps
   `Type::String`. Both render `String`; the coercion seam's `String ↔ PyStr` arm is the
   identity. **Output byte-identical**; the examples oracle proves it.
1. **Add `JsString`** (`runtime/js_string.rs`) and `ToJsString`, with unit tests in
   `crates/smelt-runtime/src/runtime_tests.rs`, not yet demanded. Add demand wiring in
   `runtime_demand.rs`. No output change.
2. **Spell through the `JsString` API while it is still `String`.** Generated crates get
   `type JsString = String; type JsStr = str;` plus a `JsStringOps` extension trait on
   `String`/`str` (`len_units`, `unit_at`, `slice_units`, `to_js_string`, …) that forwards to
   today's `smelt_utf16_*` helpers. Move the emitter module by module (strings,
   strings_io, coercion, place, list, map, …) to `JsString::from(lit)`, `.to_js_string()` and
   the `JsStringOps` names. Behaviour is unchanged, goldens change. Many small PRs, each
   re-blessing goldens with the mechanical diff check in §5.
3. **Runtime signatures spell `JsString`/`&JsStr`** for JS-visible strings (same aliases).
   Host internals keep `String`; accessors convert through `JsString::from`/`to_string_lossy`,
   which are the identity under the alias. Per-file PRs.
4. **The flip.** Replace the aliases with the real `JsString` struct and `JsStr = [u16]`.
   `JsStringOps` becomes inherent methods and `runtime/string_units.rs` is deleted.
   `SmeltUnknown::String` takes `JsString`. Map/Set/Record keys move to `JsString`. The
   remaining compile errors are exactly the unconverted boundaries. This is the one large PR;
   steps 2–3 keep it mostly mechanical. **Requires step 5 to have landed** (or the interim
   regexp transcode, with a dated follow-up).
5. **Regexp on `regress`.** This can land *before* the flip: `regress` over `&str` with byte
   indices mapped through the current helpers. It drops `js_regex.rs` translation and
   `fancy-regex`/`regex` (`deps.rs:107`) and gets exact JS syntax. At the flip, switch to the
   `utf16`/`ucs2` entry points and delete the index mapping.
6. **Lone-surrogate literals and erased-object keys.** Add `Constant::StringUnits` from
   `js_text.rs` when the `JSStr` is ill-formed. `SmeltObject` keys become `JsString`.
7. **Exact host edges:** a JSON writer over `JsStr` (escape lone halves), JSON.parse of lone
   escapes, `encodeURIComponent` `URIError`, `TextEncoder` lossy encoding.
8. **Performance follow-ups,** each measured: `memchr::memmem` for long needles; a cached
   `JsString` per literal in hot loops (`thread_local`, `Rc` clone); evaluate Latin-1 storage
   against a memory benchmark.

## 4. Acceptance criteria

**Benchmark.** Commit the workload, which today exists only in the `c5d98dce` message. Add
`benchmarks/strings/utf16_index.ts`: the `scan`, `words` and `chars` phases over an 88 KB
ASCII text and the same text with every word carrying a non-ASCII letter and one astral
character. It prints a checksum per phase (so a wrong answer fails), and runs through the
existing `benchmarks/run.py` harness against Node. Targets, release build, 20 rounds:

| phase | target |
|---|---|
| indexOf+slice words, 88 KB, ASCII and non-ASCII | ≤ 30 ms each (Node 3–5), non-ASCII ≤ 1.5× ASCII |
| charCodeAt scan, 4000 chars, either text | ≤ 1 ms |
| s[i] for i < 2000, either text | ≤ 1 ms |
| Hono `encodeBase64`-shaped `+=` loop over 1 MB | linear: 2× input → ≤ 2.2× time |

Checksums equal Node's in every phase.

**Parity.** Every `examples/typescript/end-to-end/*/expected.stdout` (Node-validated) still
matches. The 5 `ensure_node_agrees` tests pass. The 12 Python examples and the
cross-language tests (`hir_cli_cross_language_tests.rs`) pass.

**Ratchets.** `blocker-logs/smelt-unknown-baseline.json` stays at 0 avoidable, and
es-toolkit's avoidable count does not rise. `JsString` is concrete, and erasing a string
already counts as erased today. Same for `typedness-baseline*.json`. `classify_line`
(`crates/smelt-transpiler/src/unknown_report/classify.rs:184`) needs no change unless it
matches the `Rc<str>` payload; I found no such match.

**Fixtures to add** (`examples/typescript/end-to-end/`, next free numbers after `191_…`).
Each prints code units in hex wherever stdout's lossy UTF-8 would hide a difference:

- `192_utf16_lone_surrogate`: `console.log("\ud83d")`, `console.log("😀".slice(0,1))`, `[s]`
  and `{k: s}` inspect, `"😀".slice(0,1) === "\ud83d"`, `slice(0,1)+slice(1) === s`, `length`,
  `charCodeAt(0).toString(16)`, template literal of a half, `String.fromCharCode(0xd83d) === s`,
  `isWellFormed`/`toWellFormed`.
- `193_utf16_code_unit_order`: `["￿","😀","a","é"].sort()`, `"😀" < "￿"`,
  `localeCompare` of halves.
- `194_utf16_regexp_indices`: `exec` index after an emoji, a `g` `exec` loop's `lastIndex`,
  `/\ud83d/g` replace on `"😀😀"`, `/./u` vs `/./` over an emoji, `search`, `split(/(?:)/u)`,
  `match` captures after astral text.
- `195_utf16_keys`: `Map`/`Set`/`Record<string,…>` keyed by a half from `slice` and the
  literal `"\ud83d"`; `Object.keys` order and identity.
- `196_utf16_host_edges`: `TextEncoder` of a half, `encodeURIComponent` throwing `URIError`,
  `toUpperCase` passthrough, `JSON.stringify` of a half (once step 7 lands; until then this
  fixture is excluded with a note, not given a wrong expected output), `btoa` over code units
  > 0xFF throwing.
- `197_utf16_concat_loop`: the `+=` builder (correctness; the speed is covered by the
  benchmark).
- A cross-language test in `hir_cli_cross_language_tests.rs`: Python passes non-ASCII and
  astral `str` to a TypeScript function that slices it and returns it; Python prints it.

**Mutation checks** (CLAUDE.md "Verification contracts"). Temporarily make
`JsString::slice` round a cut pair to the left, derive `Ord` by code point, and drop the
`ucs2`/`utf16` distinction in regexp. Confirm that 192, 193 and 194 respectively fail. Record
the result in the PR.

## 5. Risks and verification at scale

- **The byte oracle changes nearly every golden.** 168 of 169 TS `expected.rs`, plus the
  prelude text. Byte-identity cannot be the check. Instead:
  1. *stdout is the oracle.* `expected.stdout` files are Node's output and must not change
     (except new fixtures).
  2. *Mechanical-diff gate for golden re-blessing.* A script (`scripts/golden_rename_diff.py`)
     applies the known rewrites to the old golden (`"x".to_owned()` → `JsString::from("x")`,
     `: String` → `: JsString` in TS bodies, `smelt_utf16_len(&x)` → `x.len()`, …) and diffs
     the result against the new golden. Reviewers read only the residue. Steps 2–3 keep each
     PR's residue small.
  3. *Library suites.* `smelt rust-test-report` over es-toolkit, remeda, radash, date-fns and
     the hono_crud benchmark. The pass count per suite must not fall, and the failing set's
     diff is reviewed name by name.
  4. *Differential string fuzzer.* A seeded generator writes a TypeScript program applying
     random operation sequences (`slice`, `substring`, `indexOf`, `charCodeAt`,
     `codePointAt`, `split`, `replace` with string and regexp, `toUpperCase`, `+`, `sort`,
     `Map` keys, `JSON.stringify`, `padStart`, `at`) to strings drawn from ASCII, Latin-1,
     BMP, astral and lone halves. It prints code units in hex and compares Node with the
     generated crate. Commit with fixed seeds as an integration test; longer runs on demand.
- **Compile time and code size.** `JsString` is about 300 lines, demanded by nearly every TS
  crate (it replaces the 140-line seam). `regress` replaces `regex` + `fancy-regex`. `regex`
  is the heavier crate, so regexp-using crates should compile faster; measure on es-toolkit's
  generated crate (cold `corgi build`, before and after). Emitted text length is roughly flat
  (`JsString::from("x")` vs `"x".to_owned()`). Accept ≤ 5% cold-build regression.
- **Runtime cost at host edges.** Hono and fetch-heavy code crosses `String ↔ JsString` on
  every URL, header and body access: O(n) each. Gate: hono_crud requests/s within 5% of
  today. If a body path regresses, give `Response.text()` a direct UTF-8 → UTF-16 decode
  (no intermediate `String`).
- **Memory.** Doubling for ASCII-heavy data. Measure peak RSS of the es-toolkit test run;
  Latin-1 storage (step 8) is the lever, behind the private field.
- **`regress` performance.** It is a backtracking engine, as V8's irregexp is; `regex` is a
  DFA. A pattern that is linear under `regex` could backtrack. Mitigation: the library suites
  plus a regexp micro-benchmark over the corpus's literal patterns. Report any pattern more
  than 3× slower.
- **Hashing cost.** Map-heavy code hashes 2n bytes. If it shows up, switch the key hasher
  (the containers already own their index type, `runtime/js_map.rs:56`) rather than the
  representation.
- **Python regressions from step 0.** The `is_text()` audit can misclassify a site. The
  12 Python examples and the cross-language tests are the net; step 0 must be byte-identical,
  which catches every misclassified site that changes output.

## What I did not read

- `crates/smelt-frontend-ts` beyond `lowering/js_text.rs`. I counted its `Type::String` sites
  but did not audit them (the design does not change them).
- The bodies of `fetch_types.rs`, `http_server.rs`, `form_data.rs`, `crypto.rs` and
  `console_inspect.rs`. Their counts are grep counts; the "host internals keep `String`" rule
  is a design choice, not a verified property of each site.
- `json_decode.rs` typed decoding in detail, and serde_json's handling of lone `\u` escapes.
- `regress` feature-gated method names (`find_from_ucs2`/`find_from_utf16` as Boa uses them).
  The docs.rs default page does not list them.
- Each of the 293 codegen `Type::String` sites one by one: the step-0 audit is unsized beyond
  that count.
- Nothing was compiled or run except `node` behaviour checks.

## Implementation notes

Where the implementation departs from the design above, and why.

- **`JsStr` is a newtype, not `type JsStr = [u16]`.** `JsStr` is a
  `repr(transparent)` unsized struct over `[u16]`, as `Path` is over `OsStr`.
  An alias would let `[u16]`'s inherent `len`/`contains`/`split`/`repeat`
  shadow the JavaScript methods an extension trait adds (`s.contains(&t)`
  would resolve to `<[u16]>::contains(&u16)`), and `Display` cannot be
  implemented for a foreign slice type.
- **Search arguments are `impl SmeltJsPattern`.** `index_of`, `starts_with`,
  `split`, `replace_*` take a JavaScript string or a Rust `&str` (encoded on
  the stack when short), so the emitter and the runtime can pass a literal
  without building a `JsString`.
- **Step 3 is merged into step 4.** The runtime crate compiles the text of
  every item with all gates on, so its items cannot be written against an
  alias that only generated crates see. Step 2 (emitter spellings under
  `type JsString = String`) shipped on its own; the runtime moved at the flip.
- **Erased-object keys moved at the flip (part of step 6).** A
  `Record<string, unknown>` (`SmeltRecord<JsString, SmeltUnknown>`) shares its
  field store with the erased `SmeltObject` it is viewed as; the two cannot
  have different key types. `SmeltObject` keys are therefore `JsString` from
  step 4 on. Its lookups still accept the runtime's own `&str` keys through
  `SmeltObjectKey`, which encodes a short ASCII key on the stack.
- **Host edges.** Host internals keep UTF-8 `String`. Arguments cross through
  the `SmeltHostText` trait (implemented for `str`, `String`, `JsStr`,
  `JsString`), so a host function takes either; results cross in the emitter
  (`host_text_at_type` for host rvalues, the fallible-op adapter, the host data
  property reads). `SmeltHostText` is emitted in every crate (its JavaScript
  impls live in the `JsString` item), because host items such as the JSON
  parser and `parseInt` are shared with Python.
- **Regexp at the flip uses a lossy interim** (step 4): the haystack is
  transcoded to UTF-8 per call and indices are mapped back with the old
  `smelt_utf16_*` helpers, so `string_units.rs` survives until step 5.

- **Step 5 (regexp on `regress`).** `regress` 0.10 with the `utf16` feature.
  The API used: `Regex::with_flags(&str, &str)` for a `u`/`v` pattern,
  `Regex::from_unicode(units, flags)` for any other pattern (its code units
  passed as the "code points", so an astral literal is its two halves, as
  ECMA-262 reads a non-Unicode pattern), and `find_from_utf16` /
  `find_from_ucs2` over the subject's code units; a `Match` reports code unit
  ranges, so no index is ever converted. The engine compiles `i`, `m`, `s`,
  `u`, `v`; `SmeltRegExp` drives `g`, `y` and `d` itself. `split` implements
  ECMA-262 `@@split` (captures included, an empty match at a piece's start
  skipped), which the old engine did not.
  Python `re` keeps `fancy-regex`: its syntax is not ECMAScript's, so
  `re.sub` lowers to its own helper (`smelt_py_regex_replace`) and the shared
  regex ops resolve their crate by the body's language (`Need::RegexOps`).
  The TypeScript frontend no longer translates patterns: a `RegExp` keeps its
  JavaScript source, and a regex literal at a direct-op site (`.test`,
  `.replace`, a replacer callback) is lowered to a `RegExp` value. Where a
  literal must become a pattern *string*, `i`/`m`/`s` fold into an ES2025
  modifier group `(?ims:...)`, which `regress` parses; `u`/`v` have no
  in-pattern spelling and those two sites (a regex literal passed where a
  `string` is expected, a regex literal folded into a `const` string) still
  drop them, as before. `string_units.rs` and `Need::StringUnits` are gone.
- **Step 6 (lone surrogates).** A string literal value holding a lone
  surrogate lowers to `Literal::StringUnits` / `Constant::StringUnits` (its
  exact UTF-16 code units) and is emitted as `JsString::from_units(&[..])`;
  every other literal stays `Literal::String`. Template quasis and static
  property names still cross into Rust text lossily (U+FFFD), as before: a
  lone half there is rare, and a property name is a Rust field spelling.
  The erased-object key part of this step moved to step 4 (see above).
  `String.prototype.isWellFormed`/`toWellFormed` exist on `JsStr` but are not
  yet modelled as frontend string methods.
- **Step 7 (host edges) and the rest of step 6.**
  - Template literal parts and string-literal object keys keep a lone
    surrogate's code units too (`template_element_literal`, the dict-literal
    key path). A folded module object constant records such a key's units
    (`ObjectConstEntry::key_units`), so `key in table` and the recreated object
    read the exact key. A *typed* struct field named by such a key is a Rust
    identifier and keeps the lossy spelling.
  - `String.prototype.isWellFormed`/`toWellFormed` are modelled
    (`StringIsWellFormed`, `StringToWellFormed`).
  - `JSON.stringify` writes through the runtime's own JSON writer
    (`smelt_json_stringify`), which applies the same ECMA-262 rules as
    `Serialize for SmeltUnknown` and quotes every string, value or key, over
    its code units (`QuoteJSONString`), so a lone half is its lowercase
    `\uXXXX`. serde's data model cannot carry a lone surrogate, and a
    `serde_json` `RawValue` would need a feature a project's own
    `[rust.dependencies]` entry may leave out.
  - `JSON.parse`: `serde_json` rejects a lone-surrogate `\u` escape. When it
    fails on text holding a surrogate escape, a small ECMAScript-exact reader
    (`SmeltJsonReader`) parses it, keeping each escape's code unit; other
    invalid JSON throws `serde_json`'s `SyntaxError` as before.
  - `encodeURI`/`encodeURIComponent` are fallible ops, like the decoders: a lone
    surrogate (`SmeltHostText::has_lone_surrogate`) throws a catchable
    `URIError: URI malformed`.
  - `TextEncoder.encode` writes U+FFFD for a lone half, as Node does, through
    the host-text path; no change was needed.

## Acceptance results

`benchmarks/strings/utf16_index.ts` through `benchmarks/strings/run.py`
(release build, 20 rounds per phase, milliseconds for all 20 rounds; the
checksums of every phase equal Node's). "Before" is main at `3a6ce75c`, the
UTF-8 `String` representation; "after" is the step 7 tree.

| phase | Node | before | after |
| --- | ---: | ---: | ---: |
| scan ascii (4000 x `charCodeAt`) | 2 | 3 | 1 |
| scan wide | 1 | 57 | 1 |
| words ascii (88 KB `indexOf` + `slice`) | 7 | 561 | 15 |
| words wide | 4 | 10445 | 11 |
| chars ascii (`s[i]`) | 1 | 3 | 0 |
| chars wide | 1 | 25 | 1 |
| build n=1048576 (`+=`) | 58 | 36 | 38 |
| build n=2097152 | 65 | 75 | 75 |
| build n=4194304 | 121 | 153 | 149 |

- 88 KB `indexOf` + `slice`: 15 ms ASCII and 11 ms non-ASCII for all 20
  rounds (target <= 30 ms), non-ASCII 0.73x ASCII (target <= 1.5x).
- 4000-unit `charCodeAt`: 1 ms for 20 rounds (target <= 1 ms).
- `+=` builder: linear, 38 / 75 / 149 ms at 1M / 2M / 4M pieces.
- Peak RSS of the benchmark: Node 278900 KB; generated program 8488 KB
  before, 14448 KB after (a JavaScript string is 2 bytes per code unit).
- Defect check: with `JsString::push_str`/`push_unit` copying the buffer on
  every append instead of appending in place when unshared, the 1M build takes
  193810 ms instead of 38 ms and the 2M build does not finish within the
  remaining four minutes, so the linearity check catches a broken
  append-in-place. The unit test `a_unique_builder_appends_in_place` fails on
  the same defect.
- Lone halves (`"\ud83d"`, `"😀".slice(0, 1)`) print, compare and measure
  `.length` as in Node, and `["😀", "\uffff"]` sorts as in Node (code unit
  order): node-parity fixture `lone_surrogate_strings`.

Step 8 (performance) was not needed: every target is met.
