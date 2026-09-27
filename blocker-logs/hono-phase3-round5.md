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

# Round 7 — the "module-global function expression default stub" family, re-triaged

Same checkout/overlay. Baseline at the start of the round: **266 passed / 14 failed** (280).
End of round: **268 passed / 12 failed**. Previously passing now failing: **0** (names
compared modulo the numeric `_NNN` suffix; 2 gained, 0 lost).

Round 6 filed 10 tests under one family — module-level arrow consts read from class
methods lowering to `module_global_function_expression`'s default-returning stub. Checking
each test against its panic and the generated code showed **three** different root causes;
the stub was real and is fixed, but it only owned `mergePath`.

## Per-test root cause

| tests | root cause | outcome |
| --- | --- | --- |
| url `mergepath` | `export const mergePath: (...paths: string[]) => string = (base, sub, ...rest) => { .. mergePath(sub, ...rest) .. }`: the arrow's item was pushed only AFTER its body was lowered, so the self-call found no item and bound the default-returning stub (`""`). | **fixed** (rule A) |
| url `getqueryparam` | not the stub. `decodeURIComponent_` already read through its module slot. The real defect was in the Rust control-flow structurizer: in `_getQueryParam` the optimized `if (..) { ..; if (!encoded) return undefined }` arm falls through to the slow path, but a `Switch` inside a forward join region was handed to `emit_terminator`, which lost the region's stop and rendered the fall-through as the function's default return (`return SmeltUnknown::Null`) — `getQueryParam(url, 'Hono is')` answered `undefined`. | **fixed** (rule C) |
| powered-by ×3, timeout ×4, nextjs ×1 | not the stub. `createResponseInstance` IS now an item called from the `Context` accessors, but none of these tests reach it with a real app: `Hono extends HonoBase<E, S, BasePath>` and `HonoBase` is **generic**, so `super(options)` is dropped (`class_is_reproducible_base` refuses generic bases). `Hono::new` never runs the base constructor: `use`/`get`/`request`/`fetch` stay `Default::default()` closures, `app.request(..)` returns an empty `200` response without dispatching (so status assertions pass and header/body ones fail; `c.json` → empty body → JSON `EOF`). | **open** — needs the architectural change below |

## Rules

* **A. A module arrow const called from any item body is an item, and a self-recursive
  one is predeclared.** `forward_arrow_const_names` scanned function declarations, slot
  initializers, exported consts and other arrows as referrers, but not CLASS declarations
  (plain, exported, `export default`), so `const add = ..; class C { m() { return add(1) } }`
  kept `add` as a module-body closure and the method called the stub (`0`). Classes and
  default exports are now referrers, and an arrow that names itself in its own body is its
  own referrer. `arrow_function_const_declaration_inner` predeclares a self-recursive
  arrow's item (final params, rest, required count, declared return type) before lowering
  its body and fills the same slot afterwards — the same shape as a function
  declaration's predeclaration. Only an arrow with a return type known up front is
  predeclared (TS itself rejects the unannotated form under `noImplicitAny`, TS7023).
* **B. A modeled host class intersected with object shapes is that class.**
  Lifting the Hono `errorHandler` arrow (now read from a class field) exposed
  `Response & TypedResponse<T, U, 'text'>` lowering to `Dict<String, Unknown>`: a real
  `SmeltResponse` retyped as an empty record, which did not even compile when returned as
  `Response | Promise<Response>` (E0308). A value of a stdlib-modeled class carries exactly
  its modeled members, so the other members of such an intersection are type-level brands
  no value of that representation can hold; the class is the precise type (no new
  `SmeltUnknown`). Exactly one modeled class + records/classes only; everything else keeps
  the existing record/erased merge.
* **C. A branch inside a forward join region is emitted up to its own inner join.** In
  `emit_block_until_goto_inner`, a `Switch` in a `RegionExit::Join` region now finds the
  branch's immediate post-dominator within the region (`join_region_branch_join`: the
  nearest block every non-diverging path of each arm reaches before `stop`; an arm that
  diverges before `stop` places no constraint; `stop` itself is always a candidate), emits
  `if c { then..J } else { else..J }` (an arm that diverges is emitted whole), then
  continues from `J` still bounded by `stop`. Arms that cycle back to the switch, or a
  branch whose arms both diverge, keep the previous emission.

## Open, next queue (12)

| blocker | tests |
| --- | ---: |
| **derived-constructor semantics** (below) | 8 (timeout 4, powered-by 3, nextjs 1) |
| `node:crypto` `createHash` oracle | 1 |
| `node:path/posix` `join` oracle | 1 |
| throwing `new RegExp(dynamic)` | 1 |
| global-object property store (`Object.assign(global, ..)`) | 1 |

**Derived-constructor semantics** is an architectural item, not a lowering rule, and was
deliberately not attempted this round. Two defects, both needed for the Hono app tests:

1. `super(..)` to a GENERIC base is dropped (`class_is_reproducible_base`): the base's
   constructor body never runs. Minimal: `class Base<E = string> { greet; constructor() {
   this.greet = (x) => .. } } class D<E> extends Base<E> { constructor() { super() } }` —
   `new D().greet('hi')` returns `""` and the base's `console.log` never prints.
2. Even for a NON-generic base, `super(..)` constructs a separate base object and copies
   its fields into the derived struct. A closure the base constructor stores captures the
   BASE object as `this`, so later derived-constructor writes are invisible to it
   (`this.router = ..` in `Hono`'s constructor, read by `HonoBase`'s `fetch`/`#dispatch`
   closures): `base::hi` where Node prints `base:smart:hi`.

The hand-port shape is "the base constructor initializes the DERIVED instance": either the
base constructor body lowered against the derived receiver (an `init(&this)` per base over
the flattened layout), or a base-fields trait the derived struct implements. Both change how
every derived class is constructed and belong in their own design round.

Also seen, not fixed: `Object.assign(this, rest)` in a constructor does not write declared
fields; a generic exported alias parameter type (`Options<E>`) used as a constructor
parameter erases to `SmeltUnknown`; a module-body (non-lifted) closure copy of
`_getQueryParam` makes the structurizer emit ~600k lines (rustc OOM) — the lifted `fn`
path is fine; `{ status?: number }` passed as `ResponseInit` reads `init.status` as
`Optional<Optional<Float>>` (codegen type-table error).

## Gates on this change (local)

* `cargo check --lib`, `cargo clippy --lib`: no diagnostics on added lines.
* `cargo test --lib -p smelt-frontend-ts -p smelt-codegen-rust`: 1138 + 1115 pass (3 new
  tests in `module_arrow_item_tests.rs`: class-method call targets the lifted arrow item;
  `fact`/`join` self-calls target their own items; `Response & Brand<T>` lowers to
  `Response`).
* New e2e `143_module_arrows_are_items` (stdout from Node): class members calling module
  arrows, private + annotation-mismatched exported recursion, a `Response & Brand` arrow
  returned as `Response | Promise<Response>`, and the join-region branch shape (prints
  `undefined` instead of `a=1;b;` without rule C). Golden suite passes.
* Golden changes: only `56_union_member_read/expected.rs` — `status_header_of`'s nested
  `if (x) return ..` inside an outer `if` now falls through to the function's shared
  `return "not-an-object"` instead of duplicating it in an `else`, and the ternary result
  local is no longer pre-declared with a default (both arms assign it). Same behaviour,
  structured form. No HIR/MIR golden other than 143's changed.
* examples SmeltUnknown invariant: avoidable **0 → 0**.
* hono: 266/14 → **268/12**, 0 lost (gained `url getqueryparam`, `url mergepath`).
* es-toolkit / other corpora not run locally (CI). Rule A lifts more arrows (every arrow a
  class body reads), rule B replaces a `Dict<String, Unknown>` spelling with the host class,
  rule C changes structured emission of branches inside forward join regions.
