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
