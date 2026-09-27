# Hono phase 3, round 5 — presence tests and identity

Same checkout and overlay as round 4 (`target/compat-repos/hono`). Fresh `target/debug/smelt`,
`smelt build --manifest-path <abs>`, `cargo test --no-fail-fast -- --test-threads=4` on `dist-smelt`.

| gate | round 4 end (H26) | round 5 |
| --- | --- | --- |
| tests | 219 passed / 61 failed (280) | **256 passed / 24 failed** (280) |
| previously passing now failing | — | **0** (after normalizing the numeric `_NNN` suffix the test generator appends to same-named cases, which shifts when item numbering changes: `get_book_250/251` → `251/252`, `should_be_book_411/412` → `412/413`) |

## Failure families at the start of the round (61)

| # | family | tests | root cause |
| ---: | --- | ---: | --- |
| F1 | `utils/body.test.ts` — every `parseBody` case | 24 | `isRawRequest = (r: HonoRequest \| Request) => 'headers' in r` emitted `false`: the union `in` check only knew user-class DATA fields, and a host class (`Request`) has no MIR class entry, so every arm answered `false`; `parseFormData` then read `request.raw.headers` off a `Request`. |
| F2 | `router/trie-router/node.test.ts` | 15 | several independent presence/identity defects in `Node#insert`/`#search` (below) |
| F3 | `utils/crypto.test.ts` | 4 | `unreachable!("union guard selected an excluded member")` in `crypto.rs` (union narrowing picks an arm the guard excluded) |
| F4 | `middleware/timeout` | 4 | app-level (compose / timeout race), not investigated |
| F5 | `middleware/powered-by` | 3 | `c.res.headers.set(..)` after `await next()` not visible on the returned response, not investigated |
| F6 | `utils/url.test.ts` | 4 | `getPattern` regex source (`\/` escaping, F2d below), `getQueryParam(s)` fragment handling, `mergePath` rest recursion |
| F7 | `utils` (cloudflare kv / ensureWithinOutDir / joinPath) | 3 | `SyntaxError: EOF while parsing` (JSON of an empty body) and `index out of bounds` in `utils_3.rs:159` |
| F8 | `adapter/nextjs handler`, `serve-static/path`, `pattern-router` dup param | 3 | singletons |

## Fixed (largest family first)

| # | rule | files | proof |
| ---: | --- | --- | --- |
| F1 | `'k' in value` answers from the value's whole instance surface. A host class answers from a new shared stdlib registry of its SPEC surface (`smelt_stdlib::stdlib_class_instance_has_property`: Request/Response/Headers/URLSearchParams/FormData/Blob/File/TextEncoder/TextDecoder, plus `Object.prototype` members); a user class answers for fields, methods, accessors and inherited members; a single concrete class receiver is answered statically too (it used to be erased first, losing methods). The erased-arm recognizer keeps its data-field-only rule. | `smelt-stdlib/src/classes.rs`, `emitter/union.rs`, `emitter/map.rs`, `expr/operators.rs` | e2e 133 (1), stdlib unit test; body 24 → 2 |
| F2a | A local assigned a keyed read and compared with `undefined`/`null` EXPLICITLY is widened to `T \| undefined` even when `T` has falsy inhabitants (`const nextP = parts[++i]; nextP === undefined`). Truthiness-only tests keep the old rule (the default already reads falsy). | `presence_tested_locals.rs`, `testing/matchers.rs` | e2e 133 (2) |
| F2b | An erased (`any`/union) module `const` with a non-literal initializer read from functions is a module SLOT, evaluated once (`const emptyParams = createNullObject()`; `x === emptyParams` identity). So is a `const x = new UserClass()` read from functions. A binding the program calls stays on the erased call path. | `module_init.rs` | unit `erased_and_constructed_module_consts_read_from_functions_are_module_slots`, e2e 132 (`viaGlobal`) |
| F2c | `{ ...rec }` of a typed record into an erased record merges the source instead of silently replacing it by `{}`. | `expr/operators.rs` | unit `typed_record_spread_into_an_erased_record_keeps_its_source` |
| F2d | `rec[k] ?? f` / `arr[i] ?? f` test the read's own `Option` (a missing key/index is `undefined`); the total read used to supply `""` and never reach the fallback. `optional_element_read_text` gained the record arm. | `emitter/optional_access.rs`, `emitter/place.rs` | e2e 133 (3) |
| F2e | `list[i] ||= []` tests the element read as optional (same rule records already had). | `stmt/assignments.rs` | e2e 133 (4) |
| F2f | `.test()` on a receiver typed `RegExp \| X` or erased runs the runtime regex (`SmeltFromUnknown`), instead of compiling the stringified `/src/` as a pattern. | `stdlib.rs` | e2e 133 (5) |
| F2g | `SmeltRegExp::new` stores `source` as `EscapeRegExpPattern` renders it (`new RegExp('a/b').source === 'a\\/b'`). | `smelt-codegen-rust/src/lib.rs` | e2e 133 (5), url `getPattern` test |
| F2d' | (narrowing found by the `projected_receiver_place_runtime` tier) the record arm applies only to `SmeltRecord`-backed (string-keyed) records; other key types render other maps whose `get` borrows. | `emitter/place.rs` | tier passes |
| — | `SmeltRequest: Default`, so a class with a `Request` field can derive `Default` for its storage struct. | `fetch_types_prelude.rs` | e2e 133 |

Also this round (task A): builtin call handlers are keyed on member NAMES; a receiver whose static
type declares that member as a method now owns the call (`dispatch_builtin_call` declines the whole
registry; `list_callback_call`/`list_reduce_call` also check after lowering), and a function literal
passed to a class method is contextually typed by the declared callback parameter, with the
`&dyn Fn` borrowed-callback ABI at the call. e2e 132, unit
`user_class_methods_named_like_array_methods_are_not_array_builtins`.

## Open, next queue (24)

| family | tests | evidence |
| --- | ---: | --- |
| crypto union guard `unreachable!` | 4 | `crypto.rs:128` / `:81` |
| timeout middleware | 4 | `middleware/timeout/index.test.ts` status 504/408/500 |
| powered-by | 3 | header set after `await next()` lost |
| url | 3 | `getQueryParam(s)` with `#fragment`, `mergePath` with rest args |
| utils | 3 | empty-body JSON parse; `utils_3.rs:159` index out of bounds |
| trie-router multi-match `res[0][1]['id']` should be `undefined` | 2 | `handlerSet.params[key] = undefined` into `Record<string,string>` stores `""`: the record's value type cannot hold absence |
| body | 2 | `__proto__` own property via `getOwnPropertyDescriptor`; `file[]` with `all: true` |
| singletons | 3 | nextjs handler (JSON EOF), serve-static path join, pattern-router duplicate param |

Known limitations touched but not fixed: a user type guard (`isRaw(request)`) does not narrow the
union (a member read in the guarded arm is erased); a same-module `const f = () => ..` read from a
module-slot initializer is a default-returning stub (`module_global_function_expression`); `in` over
a receiver that is neither statically knowable nor a class is unchanged; builtin interception on a
`new UserClass().m()` receiver is only declined by the list-callback/reduce handlers.

## Gates on this head

* examples invariant: avoidable **0 → 0**; golden suite 20/20 (79 Rust goldens gain only the
  `SmeltRegExp::escape_pattern_source` prelude lines and, where fetch types are emitted, the
  one-line `impl Default for SmeltRequest`; HIR/MIR goldens shift only by fewer duplicate receiver
  lowerings and symbol renumbering).
* es-toolkit ratchet: avoidable **27109 → 27103** (−6); baseline re-snapshotted in this change;
  `cargo check` and `cargo test --no-run` 0 errors.
* radash `384 passed; 3 failed`, remeda `1787 passed; 2 failed` (pins hold).
* runtime tiers: all pass (`projected_receiver_place_runtime` failed on the first run and passes
  after F2d').
* hono: 219/61 → **256/24**, no previously passing test lost.

# Round 6 — absence, assertions and host-shaped values

Same checkout/overlay. Baseline at the start of the round: **256 passed / 24 failed**. End of
round: **265 passed / 15 failed** (280). Previously passing now failing: **0** (names compared
modulo the numeric `_NNN` de-dup suffix; 9 gained, 0 lost).

## Per-family outcome

| family | tests | outcome | root cause / rule |
| --- | ---: | --- | --- |
| crypto | 4 | **3 fixed**, 1 open | (a) `sourceBuffer as ArrayBuffer` over `ArrayBufferView \| ArrayBuffer` projected the union onto its `ArrayBuffer` arm → `unreachable!` for a view. Rule: a TS assertion never changes the runtime arm; `crypto.subtle.digest` looks through a `TypeAssert` whose operand union is all byte-backed and the emitter reads `to_bytes()` of whichever arm is live (`match` over every arm). (b) the hex encode `Array.prototype.map.call(view, (x) => x.toString(16))` answered `[]`: generic read-only `Array.prototype.<m>.call(x, ..)` (registry `ARRAY_NON_MUTATING_METHODS`) now lowers as `Array.from(x).<m>(..)`; and `toString(radix)` inside a callback body dropped the radix (callback-body method lowering), now `NumericToStringRadix`; an erased receiver dispatches at run time. Open: `should create hash for Buffer` compares against `createHash` from `node:crypto`, which is not a modeled host export (our `sha256(new Uint8Array(1))` matches Node, verified in a fixture). |
| trie-router | 2 | **fixed** | `expect(params['id']).toBe(undefined)` on a `Record<string, string>` folded to a constant `false` (the read is typed `string`). Rule: `rec[k] === undefined` / `xs[i] === undefined` / loose `== null` with a non-nullable element is a PRESENCE test through the read's own `Option` (`get(k).is_none()`), the Rust a hand port writes; no widening of the record's value type is needed. |
| utils (joinPath / ensureWithinOutDir) | 2 | **fixed** | `paths[0][0]` on `''` panicked (`expect("index out of bounds")`). Rule: a string index read is total like the list read (`unwrap_or_default()`); miss-sensitive reads go through the optional read. |
| body | 2 | **fixed** | (a) `FormDataEntryValue` was an opaque erased class, so pushing an entry into `(string \| File)[]` chose the `File` arm for a string. Rule: the lib alias is its union `File \| string` (like `ArrayBufferView`/`BodyInit`), source declarations win. (b) `Object.getOwnPropertyDescriptor(obj, '__proto__')?.value` read the member off an empty record. Rule: in Smelt's object model every own record entry is a data property, so the descriptor is `hasOwn(obj,k) ? { value: obj[k], writable/enumerable/configurable: true } : undefined` (receiver and key must be effect-free re-reads). Also: `r?.value` / `r?.done` on an optional receiver was always lowered as an ITERATOR result; the syntactic `?.` now rules that out (TS types iterator results as non-optional). |
| timeout | 4 | rule fixed, tests blocked on (3) | `Promise.race([next(), timeoutPromise])`: a racer's `sleep(1100)` fired every timer due in its own window, including the `setTimeout(reject, 1000)` of the other racer, and the earlier-listed racer won. Rule: under a race driver a positive sleep arms a no-op wake-up timer at its deadline and suspends until virtual time reaches it (a sleep IS a timer). The tests still fail on the error/text response built by `createResponseInstance` (bug 3). |
| url | 3 | 1 sub-assertion fixed, tests blocked on (3) | `getQueryParams(url, 'absent')` answered `''`: the erased twin of the optional element read gained the record arm (a missing key erases to `undefined`). Remaining failures: `decodeURIComponent_ = decodeURIComponent` and the recursive `mergePath` const are default-returning stubs (bug 3). |
| powered-by | 3 | blocked on (3) | `set res` rebuilds the response through `createResponseInstance`, which is the default stub. |
| nextjs handler | 1 | blocked on (3) | `c.json` → `createResponseInstance` stub → empty body → JSON EOF. |
| serve-static path | 1 | open | the oracle is `join` from `node:path/posix`, which Smelt does not model (`defaultJoin` itself matches Node on all 22 cases in a fixture). Needs a modeled `node:path` host module. |
| pattern-router dup param | 1 | open | `new RegExp('^/(?<id>..)/(?<id>..)')` must throw `SyntaxError`; a non-literal `RegExp` construction is an infallible `external_new` with no unwind edge. Design: a fallible `BuiltinFn::RegExpCompile` (like `JsonParse`/`UriDecode`) for non-literal patterns, compiled through `SmeltRegExp::try_compiled`. Not done this round. |
| cloudflare KV | 1 | open | `Object.assign(global, { __STATIC_CONTENT_MANIFEST })` + ambient `declare const` read: needs a global-object property store. |

Also fixed (found while reducing): a program that only constructs a `RegExp` and reads
`source`/`flags` emitted `SmeltRegExp` without its prelude (the type table now demands it); a
ternary in ARGUMENT position (`console.log(first ? first.join(',') : ..)`) lowered its arms
without the test's narrowing — now the same guard/inverse-guard facts as every other ternary.

## Open, next queue (15)

| blocker | tests |
| --- | ---: |
| bug (3) `module_global_function_expression` default stub (`createResponseInstance`, `decodeURIComponent_`, recursive `mergePath`) | 11 (timeout 4, powered-by 3, url 3, nextjs 1) |
| `node:crypto` `createHash` oracle | 1 |
| `node:path/posix` `join` oracle | 1 |
| throwing `new RegExp(dynamic)` | 1 |
| global-object property store (`Object.assign(global, ..)`) | 1 |

Known limitations seen but not fixed: a recursive typed record (`type Tree = { [k: string]:
string \| Tree }`) walked by `nested = nested[k] as Tree; nested[k] = v` writes into a copy
(value semantics of a union-held record); `JSON.stringify(undefined)` prints `null`; an async
function expression without a return annotation returned from an untyped arrow fails to compile
(E0271); a user `function main` collides with the generated `fn main`; mutable module `let`
globals are not flow-narrowed inside functions; an `Array.prototype.map.call` callback's return
type is inferred `unknown` (erased `List<SmeltUnknown>`), although its body is a string.

## Gates on this change (local)

* `cargo build --bin smelt`; `cargo test --lib -p smelt-codegen-rust -p smelt-frontend-ts -p smelt-stdlib -p smelt-hir -p smelt-mir`: 1109 + 1112 + 9 + 55 + 59 pass (14 new unit tests in `keyed_presence_and_receiver_tests.rs`; one snapshot, `string_index_and_for_of_emission`, re-snapshotted for `unwrap_or_default()`).
* New e2e `134_absence_and_host_boundaries` (0 avoidable SmeltUnknown). Golden suite 20/20. The Rust goldens change in three ways: 13 async examples gain the race-sleep branch in `smelt_sleep_ms`; the argument-position ternary narrowing gives `126`/`128`/`38`/`39`-style arms their present type (`Option<String>` → `String`); string index reads use `unwrap_or_default()`.
* examples invariant: avoidable **0 → 0**.
* clippy `--lib`: no warnings on added lines.
* hono: 256/24 → **265/15**, 0 lost.
* es-toolkit and the other corpora were not run locally (CI). The descriptor lowering builds an erased `Record<string, unknown>` (`PropertyDescriptor.value` is `any`), and `Array.prototype.<m>.call` now takes the typed `Array.from` path, so es-toolkit's SmeltUnknown count may move in either direction.
