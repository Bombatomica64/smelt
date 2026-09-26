# Hono phase 3, round 3 — the test binary runs to completion

Same checkout as round 2 (`honojs/hono` @ `eebdf7be`, overlay unchanged: the 64 `phase 3 pending`
exclusions stay). Fresh full-feature `smelt`, `smelt build --manifest-path <abs>`, then
`cargo test --no-fail-fast -- --test-threads=4` on `dist-smelt`.

| gate | round 2 | round 3 |
| --- | --- | --- |
| `cargo check --tests` | 0 errors | 0 errors |
| `cargo test` | `router/trie-router/node.test.ts` hangs (runner SIGKILLed) | **completes, no hang** |
| tests | 71 passed / 121 failed / 88 not run | **192 passed / 88 failed** (280) |

Every fix is a general lowering/codegen rule, each with an end-to-end example diffed against Node
(`examples/typescript/end-to-end/122`–`127`, golden HIR/MIR/Rust) or a frontend unit test.

## Fixed

| # | family | rule | proof |
| ---: | --- | --- | --- |
| H8a | trie-router hang: an inner `for (let i ..)` clobbered the outer `i` | Block-scoped lexical bindings: a `let`/`const` declared in a block (for-head, bare block, test-callback block) restores the shadowed outer binding when the block closes (`LocalScope::close_lexical_block`). | e2e 122 |
| H8b | trie-router hang: an `if` inside a loop read as a `while` header | `while_header` requires a genuine BACK EDGE — a path back to the header that avoids its strict dominators — not plain reachability, which every block inside an outer loop has (`is_back_edge_target`). A loop nested three deep is now recognized at any depth (the `EMIT_UNTIL_DEPTH > 1` cut-off is gone). | e2e 122 |
| H8c | the back-edge fix exposed tail DUPLICATION: an `if` in a loop body emitted everything after it into both arms (es-toolkit `mergeWith_1.rs` 30k → 118k lines) | A loop-body branch is emitted up to its JOIN — its immediate post-dominator within the loop region — and the join once after it (`emit_loop_branch_at_join`). If an arm reaches the join from inside a nested loop or forward region the attempt is discarded and the arm-contained form is used. `mergeWith_1.rs`: **30,244 → 403 lines**. | all goldens, es-toolkit/radash/remeda |
| H11 | 42 `encode.test.ts` failures: `describe.each` rows read `utf8Encoder` before the suite `const` that declares it | Table rows and suite-setup statements are bound in SOURCE order (`lower_setup_with_table_bindings`), as JS evaluates them. | unit `vitest_describe_each_rows_read_preceding_suite_setup` |
| — | `String.fromCharCode` / `String.fromCodePoint`, `str.search(regex)` | Recognized statics / regex search returning the UTF-16 index. | e2e 123 |
| — | `f(x)` where an optional parameter without a default is omitted; `ident?.method()` on an optional string/list local | Missing optional arguments pad with `undefined`; the optional builtin receiver narrows, calls, and re-wraps. | e2e 124 |
| — | a module `const` container written through (`cache[k] = v`, `seen.push(x)`) was re-created per use; `Record<K, A> \| Record<K, B>` stored as two record arms | Written-through module const containers are shared module slots; a union of same-keyed records is one `Dict<K, A \| B>`. | e2e 125 |
| — | trie-router `search('post', ..)` missed: `(m[method] \|\| m['ALL']) as HandlerParamsSet<T>` then `if (set)` folded to `if true` | (1) `Optional<X> \|\| Optional<X>` over object values is presence-coalescing and stays `Optional<X>`. (2) An assertion `as T` cannot make an absent value present: an `Optional<U>` operand asserted to an object type stays `Optional<T>`. (3) A downcast assertion (`base as Derived`, target adds fields) builds the struct and defaults the added fields. (4) A required-field interface literal whose keys are all supplied is built as the struct, not an erased record. | e2e 126 |
| — | `if (!callbacks?.length) return` did not narrow `callbacks` (html.ts) | A truthy optional chain rooted at a local (`x?.a`, `x?.m()`) proves the root present; `x?.length` on an absent `x` is `undefined` (`Optional<number>`), while a plain `x.length` stays a presence assertion. | fixtures, e2e 126 |
| — | `matcher[0]` on `Optional<[RegExp, ..]>` rendered `and_then(\|_\| None)` | A tuple read through an `Optional` receiver: `x?.[i]` is present ? element : `undefined`; a plain `x[i]` is a presence assertion and projects. A dict read whose value is a tuple is `Optional` like a class value (a fabricated tuple default is truthy). | reg-exp-router `matcher.ts` |
| — | `!(a && b)` built the operand-selecting VALUE (erased union) just to negate it | `!` over a logical expression lowers the operand as a condition. | e2e 125 (was +2 avoidable) |
| — | `new Request(url, { body })` with an erased `URLSearchParams`/`FormData` body panicked "body arm is not modeled yet" | The erased body conversion has `URLSearchParams` (query string + form content type) and `FormData` (multipart + boundary) arms, the same extraction as their typed arms. | body.test.ts (24 remain, see below) |
| — | a callback-table `const` (`{ a: lower }`) lifted to a global broke function-table dispatch in callback bodies | Only NON-literal container consts are lifted for reads from items; literals stay with the const folder unless written through. | unit `guards_callback_function_table_calls_selected_through_a_local` |
| — | radash `defer` (found by the gate): a binding written two closure levels down captured a private copy in the middle closure (E0425) | A closure that builds a nested closure re-capturing and writing a binding writes it too (`nested_closure_writes_capture`). | e2e 127 (fails without the fix) |

## Gates on this head

* examples invariant: avoidable **0 → 0**.
* es-toolkit ratchet: avoidable **30547 → 27179** (−3368, from the join emission; baseline re-snapshotted in this commit, legitimate-boundary 39276 → 37286); `cargo check` and
  `cargo test --no-run` clean; runtime ratchet 1053 passed / 6 failed.
* radash `384 passed; 3 failed` (pin holds), remeda `1787 passed; 2 failed` (pin holds).
* hono (advisory): 192 passed / 88 failed.

## Open, next queue

| # | family | evidence |
| ---: | --- | --- |
| H15 | `str.match(re)` returns `Option<SmeltList<String>>`: an unmatched capture group reads `""`, not `undefined`. RegExpRouter's `match.indexOf('', 1)` depends on exactly that difference (22 `router_test_2` failures). `SmeltMatch` already models absent groups as `None`; `match` should return it for a non-global regex. Changes the type of every `m[i]` read, so it gets its own round. | `router/reg-exp-router/matcher.ts` |
| H16 | a module-level object literal read inside a function is rebuilt at the use site with erased element types, and a `\|\| m['ALL']` fallback over it is dropped | fixture: `const matchers: Record<string, Matcher \| null> = { ALL: [..] }` read in a function |
| H17 | `Pattern \| null` where `Pattern` is `readonly [..] \| '*'` (a tuple/string-literal union plus null) renders `SmeltUnknown` | Hono `utils/url.ts` `getPattern` |
| H18 | a user class named `Box` shadows `std::boxed::Box` in the prelude | any module declaring `class Box` |
| H19 | a class method named `find` is dispatched as `Array.prototype.find` ("array callback methods currently require arrow function callbacks") | `b.find('get')` on a user class |
| H20 | `body.test.ts` 24, `node.test.ts` 19 (param/regexp matching), `url`/`crypto`/`index`/`mime` 2–4 each | `smelt rust-test-report` on `dist-smelt` |
| H9, H10, H14 | unchanged from round 2 | `hono-phase3-round2.md` |

## Round 4, part 1 — H15 (match arrays)

`str.match(re)` now returns the precise match value. When the regex's flags are statically known
(a literal, a module regex const, `RegExp(p)` without flags, a string pattern), a known-global
regex yields `Optional<List<String>>` (the plain array of whole matches) and anything else yields
the same `Optional<match class>` as `exec`: an unmatched group is `undefined`. For a regex whose
`g` flag is only known at run time, `match_string` builds `SmeltMatch::from_global_matches`.
A match used AS an array (non-mutating array methods from `smelt_stdlib::ARRAY_NON_MUTATING_METHODS`,
`for..of`, spread, `m || [..]`, a concrete-list coercion) goes through `SmeltMatch::to_array_view()`
(`SmeltList<Option<String>>`, keeping the match's identity), so every existing list rule applies.
Example `128_match_array_groups`; the 74 other golden diffs are the same 40-line prelude hunk.

Gates: examples avoidable 0; es-toolkit 27179 (+0), lib/tests compile; radash 384/3, remeda
1787/2; hono unchanged at 192/88 — the reg-exp-router's 22 failures now come from:

| # | family | evidence |
| ---: | --- | --- |
| H21 | 9 panic "optional value was absent after narrowing" at `matcher.rs`: `(matchers[method] \|\| matchers[METHOD_NAME_ALL]) as Matcher<T>` over the GENERIC `MatcherMap<T>` (the non-generic fixture works), and `if (staticMatch)` on an erased value folded to `if true` | `router/reg-exp-router/matcher.ts` |
| H22 | 13 `expect(() => router.add(..)).toThrowError(UnsupportedPathError)`: the router never throws (`throw PATH_ERROR` / `e === PATH_ERROR ? new UnsupportedPathError(path) : e` over a `Symbol()` sentinel) | `router/reg-exp-router/node.ts`, `router.ts` |
| H23 | string operations on elements of a non-global/dynamic match (`s.match(/x/)!.map(t => t[0])`): elements are `string \| undefined`, string ops on an optional string do not lower (no corpus hit) | — |
| H24 | `xs.includes(undefined)` on `(string \| undefined)[]` folds to `false` (pre-existing) | — |
| H25 | a conditional inside a call argument skips truthiness narrowing (`console.log(a ? a.join('+') : 'none')` → "array join requires an array receiver"; fine as a `const`/`return`) (pre-existing) | — |
