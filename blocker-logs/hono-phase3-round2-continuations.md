# Hono phase 3 round 2 — continuation duplication and `toContain` (Agent R)

Items: head-of-queue rows 1 (sequential `toThrow` → 2^N module) and 4 (`toContain` on an erased
actual), plus the `trailing-slash` row of the remaining table.

## Row 1 — a throwing terminator's arms now JOIN (landed)

**Where the clone was.** Not the matcher: `expect(() => f()).toThrow()` lowers to an ordinary HIR
`try { f() } catch { did_throw = true }` (`matchers.rs`), so the bug was general `try`/`catch`
emission. A throwing `Call` is emitted as a 3-arm Rust `match` (`Ok(Ok)`, `Ok(Err)` error
channel, `Err` panic route; a throwing `await` has 2). Each arm emitted its successor through the
enclosing `Continuation`, and under `Continuation::Block` that meant the WHOLE rest of the body.
N sequential statements → 3^N tails. Measured with plain programs (no vitest):

| N sequential `try { parse('') } catch { .. }` | 1 | 2 | 4 | 8 | 16 |
| --- | ---: | ---: | ---: | ---: | ---: |
| before, catch `caught += 1` (lines, whole crate) | 2863 | 2924 | 3646 | 68 450 | — |
| before, catch with `if (e instanceof Error)` | 2873 | 3024 | 7526 | 2 932 530 | — |
| after (either shape) | 2859 | 2876 | 2910 | 3026 | 3210 |

**Rule** (`crates/smelt-codegen-rust/src/emitter/throwing_join.rs`): a fork's successors
(`catch_block` + normal target, or a branch's two arms) have a JOIN when some block J is reachable
from every successor and every path from each successor reaches J or diverges at a block one of
the successors dominates (a rethrow / failed-assertion `throw` inside an arm). The dominance
condition matters: every path eventually diverges at the function's final `return`, so without it
the search accepted the `try`'s own catch block, or a failure-`throw` leaf past the real join (the
second one silently dropped a `throw` in a snapshot before it was caught). J must also not be
read-after by any arm-scoped binding (call/`await` destinations, catch bindings). When J exists,
the arms are emitted as `Continuation::Region { stop: J }` and J is emitted ONCE after the `match`;
otherwise the old per-arm emission stays. Closure bodies keep per-arm emission (their emitter's
tail-expression `return` handling is separate).

**Second defect found on the way.** A `Switch` inside a forward region
(`emit_block_until_goto_inner` with `RegionExit::Join`) fell through to `emit_terminator`, i.e. was
emitted as a top-level branch whose arms ran on PAST the region's stop; the caller then emitted the
join again. That doubled the tail at every `if` inside a catch arm (the second row of the table)
and could run a join twice when the arms did not `return`. It now goes through
`emit_forward_region_switch`: arms stop at their own inner join (same finder) or at the region
stop, and the inner join continues the region once.

Goldens: 3 end-to-end `expected.rs` shrank (`48_form_data`, `53_top_level_try_tail` −2.8k lines,
`94_throwing_getter_in_callback`); `async_rejects_to_throw_emission` snapshot now emits its tail
once. Fixture `121_sequential_try_catch_join` (stdout diffed against Node 22). Unit tests
`sequential_try_catch_emits_linear_rust` (N=8 < 2× N=4, call and `await`) and
`try_catch_tail_is_emitted_once`; vitest tier: 12 sequential `toThrow`s run, and a false 12th fails.

**Hono.** `utils/cookie.test.ts`: 616 MB → **114 KB / 1562 lines**. It still cannot leave the
overlay: the `utils/cookie.ts` it tests has the known library errors (E0425 `_smelt_tmp_37`,
E0605 `SmeltUnknown as f64`), so it moved to the "library modules with generated-Rust errors"
group. es-toolkit avoidable erasure fell 30547 → **28037** (the duplicated tails were erasure too).

## Row 4 — `toContain`/`toMatch` (landed; the Hono actual was not erased)

The 3 Hono sites are `expect(res.headers.get(..)).toMatch('application/json')` — the actual is
`string | null` (`Optional(String)`), not `SmeltUnknown`. Two general rules in `contains_expr`:

* **nullable actual** → `actual != null && contains(actual!, expected)` (actual bound once), the
  typed string/array/set/tuple path on the present value; vitest fails on a null actual too.
* **erased actual** (`unknown`/type param) → `ListContains` over the erased value, emitted as the
  prelude boundary adapter `SmeltUnknown::to_contain` (substring for a string, SameValueZero member
  for an array / set / map entries). It replaces the old projection to an erased LIST, which
  panicked on a string. Documented at both emit sites as a genuine dynamic boundary.

The 3 files still do not leave the overlay; `smelt check` now reports their next blocker:
`context.test.ts` `new URL(path, base)` (2-arg form); `basic-auth`/`bearer-auth` library modules
(`unshift` item type; string case on a non-string receiver). Comment updated in the overlay.

## `trailing-slash` — a different construct (measured, not fixed)

`middleware/trailing-slash/index.ts` is unchanged by row 1 (3 046 855 B / 11 357 lines, 448
`redirect` calls from 4 in the source). The duplicated construct is the short-circuit `&&`/`||`
chain inside the returned `async function`'s **closure body** (`emit_closure_block_inner`): each
`a && b` / `a || b` lowers to a `Switch` whose arms assign a bool temp and rejoin at a block that
itself branches, and the closure emitter emits that rejoin inside BOTH arms, so k chained
operators emit 2^k copies of everything after them (two `if` conditions with 4–5 operands each,
plus the `await next()` tail between them).

**Proposed rule:** the closure emitter must use the same forked-region join as the function
emitter (`forked_region_join` on a `Switch`'s arms, arms emitted until the join, join emitted once),
i.e. route closure-body `Switch` emission through one shared structured-region emitter instead of
its own recursive walk. The closure emitter's `return`-as-tail-expression rule is the part that
needs care: a join is emitted after an `if` statement, so the arms must not be tail expressions.
