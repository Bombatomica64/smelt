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

# Round 8 — derived-class construction: the base constructor runs on the derived instance

Baseline: PR #265 (round 7), **268 passed / 12 failed**.

## How inheritance is represented today

* **Layout is flattened.** A derived class is its own Rust struct carrying the base's
  fields first (`effective_class_fields`), no base value inside it.
* **Methods are re-emitted, not dispatched.** Every inherited method's MIR body is
  emitted a second time into the derived `impl` (`effective_class_methods`), so
  `self.m()` inside it resolves statically to the derived override — virtual dispatch
  by monomorphization over the receiver.
* **Value vs reference is per class** (`classify.rs`): a by-value struct unless the class
  is mutated after construction or lets `this` escape into a closure, then an
  `Rc<RefCell<Inner>>` handle. Heritage closes DOWNWARD only (a subclass of a reference
  class is a reference class).
* **Constructors** are `fn new(..) -> Self` whose MIR allocates `this` (statement 0,
  `Rvalue::Struct`) and returns it.
* **`super(args)`** (`super_call.rs`): for a non-generic, non-abstract source base,
  `let __smelt_super = Base::new(args)` and then every inherited field is COPIED from
  `__smelt_super` into `this`. For a generic or abstract base the call is dropped.
  `Error`-like host bases assign their slots directly (own path, unchanged here).

Both defects fall out of the copy: the base constructor runs on ANOTHER object (a closure
it stores captures that object as `this`, so `base::hi` instead of `base:smart:hi`), and a
base whose constructed value cannot be moved field-by-field into the erased derived layout
(generic) cannot run at all. A third, found while reproducing: the derived constructor's
own field initializers were emitted BEFORE `super(..)`, so the copy overwrote a field the
derived class redeclares (`msg = 'hi'` came back as the base's `'x'`).

## JavaScript semantics to reproduce

One object. `new D(a)`: D's constructor runs; at `super(args)` the base constructor
initialises THAT object (base field initializers, base parameter properties, base body —
recursively for its own `super`), then D's field initializers and parameter properties,
then the rest of D's body. `this` in a base-created closure is the D instance; a method
the base constructor calls dispatches to D's override.

## Design: the base constructor is an initializer over the derived receiver

What a hand-porting team writes for "construct the derived value, then let each ancestor
initialise it" given flattened structs is an initializer taking the receiver:

```rust
impl Derived {
    fn new(..) -> Self {
        let mut this: Self = Derived { /* zero fields */ };
        this = Self::__smelt_init_Base(this, args);   // super(args)
        this.own_field = ..;                          // derived field initializers
        ..                                            // rest of the body
        this
    }
    // Base's constructor body, re-emitted over `Self` exactly like an inherited method.
    fn __smelt_init_Base(this: Self, ..) -> Self { ..; this }
}
```

This is the same move Smelt already makes for methods (re-emit the base body over the
derived receiver), applied to the constructor, so it composes with everything that already
works: field access through the flattened layout, `self.m()` static dispatch to overrides,
value vs reference representation (a reference-class handle passed in and returned is the
same `Rc`, so closures created by the base capture the derived instance), throwing
constructors (`-> Result<Self, ..>` + `?`), multi-level chains (the base initializer's own
`super(..)` is a call to the grand-base initializer, emitted into the same impl). Passing
and returning `Self` by value (rather than `&mut Self`) keeps the constructor MIR unchanged
— the only difference from `new` is that `this` arrives instead of being allocated — and
works identically for value structs and handles.

Rejected alternatives: (a) a `BaseFields` trait / `fn init<T: BaseFields>(this: &T)` —
every field access in the base body becomes a trait accessor, a much larger rewrite of
field emission for no semantic gain over re-emission, which Smelt already relies on for
methods; (b) embedding a real base struct (`struct D { base: B, .. }`) — changes every
field path and still needs `this` in base closures to be the D, which an embedded B
cannot be; (c) re-lowering the base constructor's AST inside the derived constructor —
the base may live in another module (Hono: `hono-base.ts`), and re-lowering duplicates
HIR instead of reusing the one MIR body.

### Pieces

1. HIR `ExprKind::BaseConstructorInit { base, receiver, args }` — "run `base`'s
   constructor over `receiver`, evaluate to the initialised receiver"; MIR lowers it to
   `Terminator::Call { callee: Callee::BaseInit(base_ctor), args: [receiver, ..] }`.
2. Frontend: `super(args)` to ANY source-declared base (plain, generic, abstract) lowers to
   `this = BaseConstructorInit(Base, this, args)`; the implicit derived constructor forwards
   the same way. The copy path is deleted. Derived field initializers and parameter
   properties are emitted right after a top-level `super(..)`, as JavaScript orders them.
   `Error`-like host bases keep their slot path.
3. Codegen: each class impl emits `__smelt_init_<Ancestor>` for every ancestor whose
   initializer its constructor (transitively) calls, from the ancestor constructor's MIR:
   receiver parameter instead of the allocation, `this` typed `Self`, type parameters
   resolved in the IMPL class's scope (the one its flattened layout is rendered in).
4. Classification: a class whose initializer body is re-emitted into a reference class's
   impl must share that representation, so heritage now closes UPWARD as well — a base
   of a reference class is a reference class (the downward rule's reasoning, "same
   JavaScript objects", applies in both directions once base bodies run on the derived
   receiver; inherited method copies had the same latent mismatch).

### Blast radius (surveyed before coding)

* examples: `83_class_expression_binding` (super() to a value base), `89_derived_default_
  constructor` (implicit forwarding, 3 levels, parameter properties, defaults); Error
  subclasses (`64`, `129`) keep their own path.
* hono: `Hono extends HonoBase<E,S,BasePath>` (generic, reference, closures over `this`),
  `EventProcessor<T>` subclasses and `JSXNode` subclasses (excluded test closures),
  `class<T> extends RegExpRouter<T>` (implicit ctor, generic base), 20 `Error` subclasses.
* es-toolkit: Error/DOMException subclasses (unchanged path), one spec-local
  `ImmutableCache extends CustomCache`. radash: one `Error` subclass. remeda: none.

### Known limitation carried (not introduced)

A generic base's type parameters are not substituted with the derived class's type
arguments anywhere yet: the flattened layout renders a base `T` field in the derived
struct's own scope (erased when the derived class does not declare a same-named
parameter), and inherited method copies spell `T` in a scope that may not declare it
(`fn get(&self) -> T` inside `impl NumBox`, E0425). The initializer copy follows the
LAYOUT's scope so it always agrees with the fields it writes. Real substitution
(`Base<T>` fields at `D`'s `Base<number>` argument, for layout, methods and initializers
together) is the follow-up.

## Status of this change

Landed: the four pieces above, plus two rules the Hono run needed once `HonoBase`'s
constructor actually ran on the derived instance:

* **D. The base-initializer receiver is upcast where its declaring base type is asked
  for.** `onError = (h): Hono<..> => { ..; return this }` stored by the base constructor
  returns the receiver, which in the initializer copy is `Self` (the derived struct) while
  the slot declares the base (E0271 `expected Hono_1, found Hono`). The receiver, and every
  closure capture of it (`inherit_base_initializer_receiver`), is rebuilt at the base type
  through the existing member-wise structural adapter
  (`base_initializer_receiver_upcast_text`). A reference class's callable members are
  shared `Rc`s capturing the original instance, so calls made through the upcast value
  still reach it. (Rejected: re-typing the inherited slot to the derived class — it made the
  existing base->derived structural copy (`handle(app: Hono)` given `basePath`'s
  `HonoBase`) need a function-return adapter that can only truncate to a default.)
* **E. An argument read through a reference class's cell is bound before a call on a
  reference receiver.** `this.#addRoute(m, this.#path, h)` held the `Ref` guard of
  `this.#path` across `#addRoute`'s `borrow_mut` ("RefCell already borrowed"); it was
  latent because HonoBase's constructor closures never ran on the app.

## Gates (local)

* `cargo check --lib` / `cargo clippy --lib`: no diagnostics on added lines.
* `cargo test --lib`: smelt-codegen-rust 1121, smelt-frontend-ts 1138, smelt-hir 9,
  smelt-mir 55 — all pass (7 derived-construction codegen tests new or rewritten).
* `hir_cli_cross_language_tests`: 21/21 (after merging main). New e2e
  `151_derived_constructor_runs_on_instance` (stdout from `node
  --experimental-transform-types`).
* Golden changes: `64_error_subclass_optional_message`, `83_class_expression_binding`,
  `89_derived_default_constructor` (`__smelt_super` + field copies become
  `Self::__smelt_init_<Base>(this, ..)` + the re-emitted initializer; MIR `call init fnN`).
  stdout unchanged for all three. No other golden moved (all 132 examples regenerated
  byte-identically).
* examples SmeltUnknown: avoidable **0 -> 0**.
* radash **384 passed; 3 failed** (unchanged). remeda **1787 passed; 2 failed**
  (unchanged).
* hono (main's overlay, main binary vs this branch): **283/12 -> 282/13**. The 8 app tests
  still fail, now LATER: `app.request` dispatches into the router and
  `SmartRouter`/`RegExpRouter` `match` returns `null` (`matchers[method]` absent after
  `(this as any).buildAllMatchers()` in `reg-exp-router/matcher.ts`, a module
  `function match(this: R, ..)` installed as a method — the erased-`this` method family,
  not derived construction). One test flips ok -> FAILED:
  `adapter for Next.js > should not use route if path argument is not passed`. It was a
  FALSE POSITIVE: with the base constructor never run, `HonoBase`'s `#notFoundHandler` was
  a default closure answering `null`, and dispatch threw "Context is not finalized. Did you
  forget to return a Response object or `await next()`?" — which
  `toThrowError('Custom Error')` accepted because the generated matcher does not check the
  message. With the real 404 handler installed nothing throws, and the route itself never
  matches because of the router family above. Instrumented in both builds to confirm
  (`dispatch path= match=Null` in both; baseline throw message as quoted).

## Next blocker

The router `this`-method family: `export function match<R>(this: R, method, path)` assigned
as `RegExpRouter.match` (and `SmartRouter.match`'s `this.match = router.match.bind(router)`)
reads `this` erased, so `buildAllMatchers()` answers `null`. Fixing it should move the 8 app
tests and restore the Next.js one honestly. Also: `toThrowError(message)` should compare the
message (it currently accepts any throw), which is what hid the false positive.

## Open issues found (pre-existing, not caused here)

* **Overridden/abstract method slots are never filled.** A method a subclass overrides (or
  an abstract one) becomes a callable FIELD on the base (`add_overridden_base_method_fields`)
  and every `this.m()` inside the class calls that field, whose value is the default
  closure: `new Shape().label` is `made ` (Node `made shape`), `this.step()` on an abstract
  method answers `0`. Reproduced with the round-7 binary. Virtual dispatch from a base
  constructor therefore cannot be shown yet; the initializer copy itself is ready for it
  (it is emitted with `Self` = the derived class, so a direct `self.m()` would dispatch to
  the override).
* Generic-base type substitution (see "Known limitation" above): inherited method copies of
  a generic base spell the base's `T` out of scope (`fn get(&self) -> T` in `impl NumBox`,
  E0425), unchanged by this round.

# Round 9 — `this`-parameter functions as methods, and what stood behind them

Baseline: main at `7417c58` (#270), measured locally with a main build on the same overlay:
**311 passed / 16 failed**.

## What the family actually was

`reg-exp-router/matcher.ts` declares `function match<R extends Router<T>, T>(this: R, ..)`
and `RegExpRouter` installs it as a field (`match: typeof match<Router<T>, T> = match`);
`SmartRouter.match` rebinds itself (`this.match = router.match.bind(router)`). The
function's `this` is NOT erased in Smelt: `this` is the dynamically scoped receiver
channel (`smelt_push_this` / `smelt_this()`, a documented boundary), and a direct
`instance.field(..)` call already installed the instance (`Rvalue::BindThis`). `R` is a
bounded type parameter, which the existing rule erases, so a real first parameter would
also have been `SmeltUnknown`; the channel was already the precise type. The receiver was
lost at a different seam:

* **Interface views of an instance.** Hono only ever reaches the router through
  `Router<T>` (an interface record): `HonoBase.router`, `SmartRouter.#routers`. Building
  that record from a class copied each callable FIELD as a bare value, so
  `router.match(..)` ran `match` with no receiver installed (`this` = `undefined`,
  `buildAllMatchers` → `null`), and never saw the field's later reassignment.

Behind it, once routing ran, three more defects each silently dropped a write:

* `if (!this.#routes) throw ..; this.#routes.push(x)` — a push on an array field declared
  optional lowered to `none` (SmartRouter.add registered nothing).
* `this[method] = handler` on a class instance (HonoBase's verb installation over
  `[...METHODS, 'all']`) was emitted as `let _ = handler;` — `app.get(..)` stayed the
  field's default closure and registered no route.
* A flat callback table: `const m = (..) => ..` inside one function resolved `m` in an
  unrelated function's `for (const [m, p] of ..)` (RegExpRouter's `buildAll` keyed its
  table by `" /a"`).

## Rules

* **A. A class's callable field viewed through an interface record dispatches through the
  instance.** The record slot (`emitter/receiver_bound_slot.rs`) installs the source
  instance as `this` (lazily erased) and reads the field AT CALL TIME. The same holds one
  step later, for a record projected out of an ERASED object (`routers: [new RegExpRouter()]`
  with an erased element type): the slot reads the member live and installs the object as
  `this`. Both are pay-for-use (`program_reads_this`) and apply to INTERFACE record
  targets only: narrowing to a CLASS rebuilds a class value whose callable fields must
  stay the very function values the object holds (a first version also wrapped class
  targets and broke es-toolkit `cloneDeep > should clone instance`, `b.d === d`).
* **B. A live read of a callable own field through an erased view.** The erased view is a
  snapshot, but the instance keeps changing (`SmartRouter` replaces `match` on its first
  call). A reference class with callable own fields, in a program that reads `this`,
  carries `__smelt_get_callable_field` (`view_write_through.rs`), the read mirror of the
  existing `__smelt_set_field`; the prelude's `smelt_live_member` asks it first.
  Reclassified in `unknown_report.rs` with its boundary argument and a test.
* **C. An in-place array method on an optional array receiver asserts it present**
  (`push`/`pop`/`shift`/`unshift`/`reverse`/`sort`): `tsc` rejects the call on a
  possibly-absent receiver, so the array is present; the `TypeAssert` keeps it a mutation
  place (field → write back; a local asserted at another type now takes the copy +
  write-back route in MIR). Optionality inherited from an optional BASE keeps the old path.
* **D. A computed-key write into a class instance dispatches on the key**
  (`emitter/class_keyed_write.rs`): `match key { "get" => self.get = v, .. }` over the fields
  the value is assignable to (same type, same call signature, or a callable-interface
  field for a function value). Other keys are discarded as before.
* **E. A non-function binding shadows another body's local callback** (`visible_callback`).
* **F. A class field holding a function satisfies an implemented interface method**
  (`validate_implements`).
* **G. `toThrow(expected)` / `toThrowError(expected)` compare the thrown error** with
  vitest's rules (`testing/to_throw.rs`): string ⊂ message, RegExp tests the message, a class
  is `instanceof`; no argument → any throw. The message is a `string`-typed catch binding
  (`smelt_thrown_message`: `message` when a string, else `String(e)`), so the handler gets
  no branch (a branch there re-emits the rest of the test body per arm — a first version
  with a `typeof` conditional grew `helper/ssg/utils.test.ts` from 7.6 MB to 18 MB and
  OOM'd rustc). `toThrowErrorMatchingInlineSnapshot` compares a serialization, not a
  message, and keeps the any-throw check (applying the substring rule to it broke remeda
  `conditional > runtime (dataFirst) > throws when no matching case`, whose snapshot is
  `[Error: conditional: data failed for all cases]`).

`f.bind(obj)` / `f.bind(obj, a)` and `this.m = other.m.bind(other)` then `this.m(..)` needed
no change: they already bind through the channel, and with A/B the bound member is live.

**Not done — a real typed first parameter for `this`.** Every corpus `this:` annotation is
`any`, `unknown` or a bounded (hence erased) type parameter, for which a parameter would
be `SmeltUnknown` exactly like the channel, while threading it would change the ABI of
every erased callable (es-toolkit has ~30 `function (this: any, ..)` wrappers). A typed
parameter pays off only for a concrete `this: SomeClass` / record annotation (only
`jsx/dom/css.ts`, excluded). Left as a follow-up, pending a decision.

## Gates (local, merged with main `7417c58`)

* `cargo check --lib` / `cargo clippy --lib`: no diagnostics on added lines.
* `cargo test --lib`: smelt-codegen-rust 1134, smelt-frontend-ts 1147, smelt-hir 9,
  smelt-mir 55; `unknown_report` tests 31 (incl. the new boundary test).
* `hir_cli_cross_language_tests`: 21/21; `hir_cli_typescript_tests::build_runs_typescript_to_throw_expected_error_comparison` (new, `.not.toThrow(other)` only passes when the comparison runs).
* New e2e `153_this_param_function_as_method` (stdout from `node --experimental-strip-types`).
  Golden changes: `130_this_bound_field_write_through/expected.rs` only — the
  `smelt_live_member` prelude helper, the `__smelt_get_callable_field` method and its
  erased-view entry (rule B), stdout unchanged. HIR/MIR goldens: only 153's.
* SmeltUnknown: examples avoidable **0 → 0**; es-toolkit avoidable **25,549 → 25,549**
  (prelude +10, boundary +20).
* hono (main build vs this branch, same overlay): **311/16 → 312/15**, 0 lost; gained
  `timeout API > no timeout should pass`.
* radash **384 passed; 3 failed**, remeda **1787 passed; 2 failed**, es-toolkit
  **1053 passed; 6 failed** — each with identical failing names to a main build. No test
  in any corpus flips because of the `toThrow` comparison: every argument-bearing
  `toThrow` there already threw the expected error.

## The 8 app tests

All eight now ROUTE (on main they panic in `matcher.rs`: no route was ever registered).
One passes. The rest fail later:

| test | now |
| --- | --- |
| powered-by ×3 | `X-Powered-By` header missing: a middleware's `c.header(..)` after `await next()` does not reach the response |
| timeout ×3 | status is not 504/408/500: the timeout race's exception path |
| nextjs `should return 200` | `res.json()` → `EOF`: `c.json(..)`'s body is empty |
| nextjs `should not use route()` | honest failure now: `handler(req)` does not throw `Custom Error` synchronously |

## Next blocker

Response-body / header propagation out of handlers (`c.json` / `c.text` bodies empty,
headers set after `next()` lost) — it owns the powered-by, nextjs and likely the timeout
tests. Minimal: `const app = new Hono(); app.get('/a', (c) => c.text('root'));
(await app.request('/a')).text()` answers `''`.

## Open issues found (not fixed here)

* `app.router.name` read through the `Router` record is a snapshot: after
  `SmartRouter.match` sets `this.name`, the view still says `SmartRouter` (data fields of
  interface views are copies; only callable fields are now live).
* `unshift` on a class field is unsupported ("requires a local array receiver").
* `return this.#items ? this.#items.join(..) : ''` fails (`array join requires an array
  receiver`): ternary narrowing does not reach a private member path.
* An erased generic `T` return through a class field slot (`SmeltList<SmeltUnknown>` vs
  `SmeltList<T>`) and `Router<T>` projections at a concrete `T` (union arm mismatch) — both
  pre-existing, seen while reducing fixtures.

# Round 10 — the four host singletons (`node:path`, `createHash`, runtime `RegExp`, global object)

Baseline: main @ 6c66003 (#272), rebuilt locally: **315 passed / 12 failed** (327).
End: **318 passed / 9 failed**. Previously passing now failing: **0**.
Gained: `crypto should create hash for buffer`, `defaultJoin … behave like path.posix.join`,
`pattern duplicate param name > self`.

## Rules

* **Host-module functions are resolved from the import, not the spelling.** A callee whose
  root identifier is an import from a modeled host module (`smelt_stdlib::host_module_id`)
  resolves to `(module, export path)`: default/namespace imports are the module, a named
  import starts at its EXPORTED name (`join as posixJoin` → `join`), and `posix` segments of
  `node:path` are dropped (on the POSIX profile `path.posix === path`). Only a local
  binding shadows. `node:path`, `path`, `node:path/posix`, `path/posix` are one
  `HostModuleId::Path`. A bare specifier naming a modeled host module never resolves to a
  sibling source file (`crypto` next to Hono's own `utils/crypto.ts`).
* **`node:path` (POSIX)**: `join`/`resolve`/`normalize`/`dirname`/`basename`/`extname`/
  `relative`/`isAbsolute` are a line-for-line port of Node's `lib/path.js`
  (`host_module_prelude.rs`); `sep`/`delimiter` fold to `/`/`:`. Differentially tested
  against Node on 25 join cases, 16 single-path cases × 7 functions, 7 `relative` pairs and
  a 256-pair cross product of 16 atoms over all nine functions (identical digest).
  `parse`/`format`/`win32` stay declared blockers.
* **`createHash`** answers a concrete `Hash` (`SmeltHash`: shared state, so `update` chains on
  one hasher) over `md-5`/`sha1`/`sha2` (md5, sha1, sha224/256/384/512, sha512-224/256,
  `sha-256`/`RSA-SHA256` aliases); `update(string, enc?)` decodes utf8/latin1/binary/ascii/
  utf16le/hex/base64(url); bytes from any byte-backed value; `digest(enc)` hex/base64/
  base64url/latin1/ascii/utf8/utf16le; `digest()` a `Buffer`. Unknown algorithm, a second
  `digest`/`update` and odd-length hex throw Node's catchable errors (fallible
  `BuiltinFn::HostModule`).
* **Runtime `new RegExp(p, f)`**: a pattern or flags that are not both source literals lower
  to `ExprKind::RegExpCompile` → fallible `BuiltinFn::RegExpCompile`, which validates the
  flags (`dgimsuvy`, no repeats, not `u`+`v`), rejects duplicate capture-group names (V8)
  and compiles through `SmeltRegExp::try_compiled`; failure is a catchable `SyntaxError` at
  construction. Literal patterns keep the infallible `New`.
* **The global object is one shared object** (`ExprKind::GlobalObject` → per-thread
  `smelt_global_object()`), replacing a fresh marker record per read.
  `Object.assign(globalThis|global, ..)` writes in place (`smelt_unknown_assign`); an ambient
  `declare const X` the profile does not model reads `globalThis.X` through a checked cast to
  its declared type. Both are classified legitimate-boundary
  (`global_object_members_are_run_time_facts`). Top-level expression statements of a test
  module replay in each test's setup, like statements in a `describe` body.

## Open

* cloudflare KV `getContentFromKVAsset` still fails, now one step later: the globals are
  read correctly, but `content as unknown as ReadableStream` over a STRING value casts into
  `SmeltBody`, which has no erased recovery, so the cast fabricates an empty default body.
  Needs a `SmeltBody` boundary adapter that keeps the value it was narrowed from (so
  re-erasure is identity) — a fetch-types change of its own.
* `Buffer.prototype.toString('hex'|'base64')` answers the comma-joined bytes (pre-existing;
  affects `createHash(..).digest().toString('hex')`).

## Gates

* `cargo test --lib` stdlib/hir/mir/frontend-ts/codegen-rust: 9 / 55 / 65 / 1157 / 1138 pass;
  unknown_report tests 32 pass; `hir_cli_cross_language_tests` 21/21.
* New e2e `157_node_path_hash_and_runtime_regexp` (stdout from Node). No other golden changed.
* examples avoidable **0 → 0**; es-toolkit avoidable **25,512 → 25,424** (−88, baseline
  re-snapshotted; main's binary measures 25,512 on the same checkout).
* radash `384 passed; 3 failed`; remeda `1787 passed; 2 failed`.
