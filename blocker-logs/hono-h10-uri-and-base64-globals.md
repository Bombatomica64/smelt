# Hono families H7 + H10 — the URI transcoding globals (and what remains)

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.

The plan listed H7 (`exported const expression references unresolved const`)
and H10 (`unresolved identifier`/`unresolved class`) separately. Probing showed
they are largely **one family**: three of the four ECMA-262 URI globals were not
modeled at all, so every spelling of them failed — called, passed as a value,
and aliased by an exported const. Fixing the family cleared both messages.

## 1. The sites, and how the family splits

| site | source | family |
| --- | --- | --- |
| `src/utils/url.ts:104` | `tryDecode(str, decodeURI)` | URI, value form |
| `src/utils/url.ts:337` | `export const decodeURIComponent_ = decodeURIComponent` | URI, exported alias (this is H7) |
| `src/utils/cookie.ts:266,278` | `value = encodeURIComponent(value)` | URI, call form |
| `src/utils/cookie.ts:48,57` | `btoa(...)` / `atob(...)` | base64 — see §7 |
| `src/hono-base.ts:541` | `addEventListener('fetch', …)` | ServiceWorker global — see §8 |
| `src/client/client.ts` | `unresolved identifier proxyCallback`, `unresolved class FormData` | `src/client/**`, `new Proxy` (see `hono-scope.md`) |

Reduced repro, 10 lines, reproducing all three URI shapes:

```ts
export const b = (s: string): string => encodeURIComponent(s)   // unresolved identifier
export const c = (s: string): string => decodeURI(s)            // unresolved identifier
type Decoder = (str: string) => string
const apply = (s: string, f: Decoder): string => f(s)
export const e = (s: string): string => apply(s, decodeURI)     // unresolved identifier
export const decodeURIComponent_ = decodeURIComponent           // unresolved const
```

`encodeURI` — the fourth — worked in all three positions. That asymmetry is the
whole finding.

## 2. Wrong output

Lowering rejects. Notably the names ARE recognized: `encodeURIComponent`,
`decodeURIComponent`, `encodeURI` and `decodeURI` are all in
`smelt_stdlib::globals::is_javascript_global_builtin`
(`crates/smelt-stdlib/src/globals.rs:70`), so the profile *claims* them and the
`"X" in globalThis` probe folds to `true` for all four — while only one of them
could be lowered. Recognition and implementation had drifted apart.

## 3. Responsible functions

* `ModuleBuilder::uri_encode_call`,
  `crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs:1630` —
  `if callee.name != "encodeURI" { return Ok(None) }`, a single hard-coded name.
* `ModuleBuilder::builtin_function_value_expression`,
  `crates/smelt-frontend-ts/src/lowering/expr/references.rs:617` — a `"encodeURI"`
  arm in a match over global names, with no siblings.
* `ExprKind::UriEncode` / `Rvalue::UriEncode` — one node for one of four
  operations.
* `ModuleBuilder::collect_module_const_decls`,
  `crates/smelt-frontend-ts/src/lowering/module_init.rs:2560` — a ladder that
  routes non-literal exported-const initializers to
  `push_expression_const_item` (array literals, calls, `new`, imported values,
  module-local references, member accesses) and then falls through to the
  *literal* folder. A bare identifier naming a global matched none of the arms,
  so `export const decodeURIComponent_ = decodeURIComponent` demanded a
  foldable literal — and a function is not a literal.

## 4. Design

ECMA-262 §19.2.6 defines all four from two algorithms (`Encode`, `Decode`)
parameterized by one character set, and the four differ **only** in that set:
the `*Component` pair treats the URI reserved separators `; / ? : @ & = + $ , #`
as ordinary data, the non-component pair leaves them alone so a full URI's
structure survives. That is one operation with a mode, so:

* `smelt_hir::UriTranscodeOp` (`Encode | EncodeComponent | Decode |
  DecodeComponent`) with an `is_fallible()` predicate, and
  `ExprKind::UriEncode` -> `ExprKind::UriTranscode { op, operand }` (same for
  the MIR rvalue). The op appears in the HIR and MIR goldens as
  `uri_encode` / `uri_encode_component` / `uri_decode` / `uri_decode_component`,
  because a wrong `op` compiles and produces a *plausible* string — exactly the
  mistake worth catching in a cheap test.
* `ModuleBuilder::uri_transcode_global(name)` maps the four names to the op, and
  both the call path and the value path consult it. The value path's arm is the
  four names listed together, so `values.map(encodeURIComponent)` and
  `tryDecode(str, decodeURI)` build a closure over the *right* variant instead
  of an erased callable.
* Runtime: `smelt_encode_uri_component`, and one shared decoder
  `smelt_decode_uri_octets(value, preserve)` behind `smelt_decode_uri` and
  `smelt_decode_uri_component` — the spec's own parameterization, so the two
  decoders cannot drift. All four live in `crates/smelt-runtime/src/uri.rs`
  beside the existing encoder and are unit-tested there.
* Exported const aliasing a global: a new arm keyed on
  `is_javascript_global_builtin` routes it to `push_expression_const_item`,
  the same treatment the member-access arm already gives
  `export const slice = Array.prototype.slice`. General expression lowering
  then produces the global's value.

Decoding correctness details that matter (all covered by unit tests):

* Decoding runs over **bytes**, because one character can be several
  consecutive escapes (`%C3%A9` is one `é`).
* A leading byte announces its run length; a continuation byte in leading
  position, a truncated run, and octets that are not valid UTF-8 together are
  all `URIError` inputs and return `None`.
* `decodeURI` keeps a *preserved* escape in its escaped text verbatim — it does
  not decode-then-re-encode — so `decodeURI('a%2Fb')` is `'a%2Fb'`, not
  `'a/b'`.

## 5. The throwing-rvalue hole (known, shared, NOT introduced here)

Both decoders throw a `URIError` on malformed input. The emitted Rust renders
that as

```rust
smelt_decode_uri(value.as_str()).expect("URIError: URI malformed")
```

— a **panic, not a catchable throw**. This is the same shape as the existing
`JSON.parse` emission (`serde_json::from_str(..).expect("JSON parse failed")`)
and it has the same single root cause, already written up in
`blocker-logs/estk-final45-misc.md` §1: a fallible stdlib **rvalue** has no
throwing edge in MIR, because only `Terminator::Call` and `Terminator::Await`
carry `unwind: Option<ExceptionHandler>` (`crates/smelt-mir/src/types.rs`), so a
throwing rvalue cannot reach an active `try`.

This matters for Hono specifically: `utils/url.ts`'s `tryDecode` exists to catch
exactly this `URIError` and fall back to decoding what it can, and
`tryDecodeURI('Hello%20World/%A4%A2')` is a real test case. Under the panic the
fallback is unreachable.

**Why it was not fixed here.** Closing it properly means giving a fallible
stdlib operation a throwing edge — either a new expression-level throw in HIR
(there is none; `HirStmt::Throw` is statement-only and an expression lowering
site has no way to append a statement at the right position), or lowering these
operations as calls to synthesized throwing function items. Either is its own
feature with its own design, and it is shared with `JSON.parse` and any future
fallible rvalue, so it should be closed **once** for all of them rather than
worked around in the URI path. The alternative I rejected — a lenient decoder
that returns undecodable text verbatim — would make Hono's tests pass by
accident while silently breaking any source that depends on the throw.

The emitter carries this reasoning as a comment at the emit site
(`uri_transcode_text`), and the runtime tier's module doc says the catch
behaviour is deliberately not asserted. The character-set boundary itself IS
asserted, in `smelt-runtime`'s `uri::tests::malformed_input_is_rejected`.

**Proposed follow-up (not done):** give `Rvalue` a fallible-operation form whose
MIR lowering emits a `Terminator::Call`-shaped edge with `unwind`, and move
`JSON.parse`, `decodeURI`, `decodeURIComponent` and `atob` onto it.

## 6. Generality

The rule is stated over the ECMA-262 §19.2.6 family and its character sets, and
the exported-const arm over `is_javascript_global_builtin`. Nothing keys off
Hono, a file, or a specific alias name. `export const enc = parseInt` gets the
same treatment as `= decodeURIComponent`.

## 7. `btoa` / `atob` — mine, NOT landed, and not on the critical path

4 occurrences (`utils/cookie.ts:48,57`, plus `utils/encode.ts:21,26` which the
manifest probe does not reach). They are base64 over *binary strings*
(latin-1): `btoa` throws `InvalidCharacterError` for a code unit above 0xFF and
`atob` for a non-base64 character, so they need the same throwing mechanism §5
describes. Neither is in `is_javascript_global_builtin`, so they are honestly
reported rather than half-modeled.

They are also **not on the critical path**: both sites sit inside
`getCryptoKey` / `makeSignature` / `verifySignature`, whose other lines need
`crypto.subtle` and `TextEncoder` — both owned by the standards stream. Fixing
`btoa`/`atob` alone would not make `utils/cookie.ts` lower. Recorded here as
remaining work with its dependency, not silently dropped.

A base64 codec already exists in the generated byte-buffer prelude
(`smelt_host_buffer_base64_value` in
`crates/smelt-codegen-rust/src/byte_buffer_prelude.rs`), so the codec itself is
not the work; the binary-string semantics and the throw are.

## 8. `addEventListener` — needs a decision, not a fix

`src/hono-base.ts:541`, inside the deprecated `Hono#fire()` Service Worker
entry point — and the source itself marks it `// @ts-ignore`, because
`addEventListener` is not in scope for TypeScript either. It is a
`ServiceWorkerGlobalScope` global that the non-DOM Node profile deliberately
lacks.

Three options, none of which I took unilaterally:

1. **Model it as a no-op registration.** Wrong: the profile says the global is
   absent, and a silent no-op `fire()` would appear to work.
2. **Lower a call to a profile-absent global as a thrown `ReferenceError`.**
   This is the precise JS answer — `Hono#fire()` in Node really does throw
   `ReferenceError: addEventListener is not defined` — and it is a general rule
   ("a call to a global the target profile declares absent throws"). It needs
   the same throwing mechanism as §5, so it should land with that work. This is
   my recommendation.
3. **Exclude the file.** Not possible: `hono-base.ts` is the framework core.

## 9. Tests

* `crates/smelt-runtime/src/uri.rs` — four unit tests: the two encoders differ
  by exactly the reserved separators; the two decoders differ the same way; a
  multi-byte escape run decodes to one character; and the malformed-input set
  (`%`, `%A`, `%zz`, `a%2`, a continuation byte in leading position, a truncated
  run, invalid UTF-8 octets) is rejected while `''` is not.
* `crates/smelt-frontend-ts/src/tests/part04_tests.rs` —
  `lowers_all_four_uri_transcoding_globals_called_and_as_values`: all four
  called, one passed as a value, and the exported alias, with an assertion that
  a `UriTranscode` node exists for each of the four ops (a wrong `op` would
  otherwise pass).
* `crates/smelt-codegen-rust/tests/uri_transcode_runtime.rs` (new tier, in the
  `host` shard of `.github/workflows/runtime-tiers.yml`) — four fixtures that
  RUN: encoder pair, decoder pair, each variant round-tripping through its own
  partner, and every value form (passed to a higher-order function, aliased by
  a module const, that alias called directly, and `map`ped over a list). Each
  fixture is chosen so the two variants of a pair *disagree*, which is what
  makes it a test of the character set rather than of "it returned a string".
* The pre-existing `encode_uri_call_and_value_forms_lower_to_uri_encode` in
  `category_tests.rs` was updated to the new node and now pins the `op`.

## 10. Result

URI family: 5 occurrences -> 0 (`decodeURI`, `encodeURIComponent` ×2, the
exported alias, and the `string search` follow-on in `url.ts` that the alias
was masking). `src/utils/url.ts` drops from 5 blockers to 1, and that one
(`request.url.indexOf(':')`) is standards-stream demand — `Request.url` must be
typed `string`.
