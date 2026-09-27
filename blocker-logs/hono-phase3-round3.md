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

## Round 4, part 2 — H21 + H22 (reg-exp router)

Hono **192 → 216 passed** (88 → 64 failed); `router_test_2` 22 → 3 failures. Both families were
several independent general bugs:

H22 (`add` never threw `UnsupportedPathError`):
* an unannotated method called from inside its own class (`this.#insertPath(..)`) was dropped as
  `undefined` — the in-progress method is now resolved (`receiver_declares_in_progress_method`);
* an optional-chained call inside `try` propagated past the `catch` — inside an active catch it is
  a presence branch plus a real call with an unwind edge (`smelt-mir/src/lower/optional_call.rs`);
* `if (!record[k][j])` over object values folded to `false` — truthiness of a keyed dict read with
  an always-truthy value type is a containment test, and a keyed read into an optional slot is an
  optional lookup;
* a local both assigned a keyed read and presence-tested in the same function is widened to
  `T | undefined` (`presence_tested_locals.rs`, a syntactic, per-function scan);
* an `if` without `else` whose arm contains a loop exiting to a lower-numbered join lost the join
  (emitter join rule + `block_reaches_within_region`);
* look-around patterns panicked the `regex` crate — `.test`/replace/split compile with
  `fancy-regex` (falls back to `regex` for simple patterns);
* `e instanceof UserClass` on an erased value folded to `false` (now checks the recorded class and
  its subclasses), and `class E extends Error {}` forwards `message`.

H21 (matcher panic, `if (staticMatch)` → `if true`):
* a typed function-valued class DATA field called on an instance now receives that instance as
  `this` (lazy `SmeltThisBinding`; `UnobservedReceiverBind` restores the plain callee when `this`
  is never read);
* writes through an erased `this` reach the instance (`__smelt_set_field` + erased-view hook,
  `emitter/view_write_through.rs`, only in programs with an erased field write); classes bound as
  `this` in such programs are reference classes;
* constantly-truthy union guards fold only for a concrete generated union; an erased union gets
  the runtime test.

Examples `129_keyed_presence_and_throw_sentinel`, `130_this_bound_field_write_through`; 11 unit
tests. `unknown_report.rs` gains two legitimate-boundary markers (the `__smelt_class` instanceof
guard, the `__smelt_set_field` adapter), each documented with a regression test.

Gates: examples avoidable 0; es-toolkit avoidable **27179 → 27109** (baseline re-snapshotted);
radash 384/3, remeda 1787/2; all 78 runtime tiers pass.

Open (next queue): H26 — the 3 remaining `router_test_2` failures: `paths[p][1].reduceRight(..)`
over a `[number, ParamAssocArray]` tuple stored in a `Record` lowers as an erased list and builds
`{}`. Also: unannotated `void` methods return `SmeltUnknown`; receivers are not yet bound for calls
inside the class's own methods, for interface/record receivers, or for async field functions; a
class `implements` an interface whose method is satisfied by a function-typed field reports a
missing method.

## Round 4, part 3 — H26 (reg-exp router parameter maps)

Hono **216 → 219 passed** (64 → 61 failed); `router_test_2` 3 → 0 failures, no other test changed
state. The tuple read `trie.paths[p][1]` was already precisely typed; the real gap was that
`Array.prototype.reduceRight` was not modeled, so an unmodeled list method fell to dynamic dispatch
and became a `none` stub. General rules:
* `reduceRight` shares `reduce`'s path (`ListReduce { from_right }`): the same typed `fold` over
  `.iter().enumerate().rev()`, so callbacks see original indices and a fold without an initial
  value starts from the last element (also the `ns.reduceRight(coll, cb, init)` spelling);
* folded module object-const metadata is retargeted to the declared `Dict<_, V>` (array → tuple of
  the same arity, array → `List<E>`, object → `Dict`) instead of the literal-inferred shape;
* annotated array/`Set` consts fold their elements when every element has exactly the declared
  element type; bindings lifted to mutable globals drop the folded copy.

Example `131_reduce_right_and_declared_const_shapes`; 3 frontend + 1 codegen unit tests. Golden
changes: only interned-type tables in 7 `expected.hir` files (several `Unknown`/`List<Unknown>`
entries become precise tuples). Gates: goldens 20/20, examples avoidable 0, es-toolkit avoidable
27109 (unchanged), radash 384/3, remeda 1787/2, all 78 runtime tiers pass.

Open (pre-existing, next queue):
* an uninstantiated callee type parameter (`createNullObject<T = any>()` inside `class Router<T>`)
  leaks the callee's `T` into the caller instead of its default/contextual type;
* `module_global_expression` fabricates a declared-type default when neither a slot nor folded
  metadata exists (a non-foldable module const reads `{}`; a mutated annotated array read only
  from a module-body closure reads `[]`);
* `list_reduce_call` runs before class-method resolution, so a user class method named
  `reduce`/`reduceRight` is hijacked;
* array methods on erased receivers still go through `smelt_get_unknown_field`.
