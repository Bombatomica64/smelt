# Branch rejoin: every branch's continuation is emitted once

Status: implemented. Code: `smelt-codegen-rust` `emitter/post_dominance.rs` (the join),
`emitter/control_flow_match.rs` (`switch`), `emitter/control_flow/switch.rs` (`if`),
`emitter/control_flow/loops.rs` (loop bodies), `emitter/throwing_join.rs` (throwing calls), `emitter/closures.rs`
(closure bodies). Follows #555 (`loop_exit_join`: a loop's early exits rejoin once).

## 0. The problem

MIR is a control-flow graph; Rust is structured. The emitter rebuilds `if`/`match`/`loop` from the graph, and where
it cannot see where the arms of a branch meet again it follows each arm to the end of the function, so the code
after the branch is emitted once per arm. A sequence of `n` such branches emits the rest of the function
`arms^n` times: transpile time and generated size are exponential.

```ts
function risky(v: number): number { if (v > 100) throw new Error("big"); return v + 1; }
export function run(seed: number): number {
  let total = seed;
  switch (total % 3) { case 0: total = risky(total); break; case 1: total += 1; break; default: total -= 1; }
  // ... n such statements: `total * 7919` below was emitted 3^n times
  return total * 7919;
}
```

Measured before this change (`n` sequential statements, copies of the code after them at `n = 5`):

| statement | function | loop body | closure | `if` arm |
| --- | --- | --- | --- | --- |
| `switch` with a call in a `case` | 243 | 243 | 243 | 243 |
| `switch` with a throwing `case` | 243 | 243 | 243 | 243 |
| `switch` with a returning `case` | 32 | 32 | 32 | 32 |
| `switch` inside `try` | 32 | 1024 | 32 | 32 |
| `try`/`catch`, `try`/`finally` | 1 | 243 | 1 | 1 |
| `if`/`else` chain with calls inside `try` | 1024 | 1 | 1 | 1 |
| `if`/`else` whose arm may throw | 32 | 1 | 32 | 1 |
| `if`/`else` whose arm may return | 32 | 1 | 32 | 1 |

After it every cell is 1. Labelled blocks, ternaries, `&&`/`||` with calls and `switch` with fallthrough were
already 1 everywhere.

## 1. The rule

A branch (an `if`, a `match` from a `switch`, the `Ok`/`Err` arms of a call inside a `try`) rejoins at its
**immediate post-dominator** `J`: the nearest block every path out of it must pass through. Each arm is emitted up to
`J`, and `J` (with everything after it) once after the branch, exactly as a hand-written `if`/`match` statement
followed by the rest of the block.

Two kinds of path do not count, because they leave through their own `return` and never reach the code after the
branch:

* a path that **throws** (a `throw` block has no successor in the post-dominator dataflow); and
* a path that **returns from inside one arm**: a `return`/`throw` block only one arm reaches
  (`branch_rejoin_post_dominator`). The function's final `return`, which every arm reaches after the join, still
  ends the paths. Only a branch with such an arm pays for the per-branch table; the cached function-wide table
  (throws neutral) answers everywhere else.

A join is used only inside the region being emitted: not past an open loop's header, continue target, exit or
exit join, nor a pending loop-body join, nor the stop of a forward `if` arm (`join_region_branch_join`); an arm that
reaches one of those, or the branch itself, without passing `J` keeps the per-arm emission. A call result bound
inside an arm and read after `J` keeps it too (`region_result_read_after`). A join that only one arm reaches (the
others all leave) rejoins nothing and is not used.

Where each context applies it:

| context | rule |
| --- | --- |
| function body | `emit_match` (`branch_region_join`), `emit_switch_at_post_dominator` |
| forward `if` arm (region with a stop) | `join_region_branch_join`, now for `match` too |
| loop body | `loop_branch_join` / `loop_region_join` (the region's own dataflow), `emit_loop_match`, `throwing_loop_join` |
| closure body | `closure_branch_join`, now for `match` too |

## 2. Requirements

* **Behavior**: every path runs the same blocks in the same order as before; only the textual copies are shared.
  Checked by Node parity (`sequential_branch_rejoins`, every shape above in a function, a loop body and a closure,
  with inputs that take each arm, throw and return; `sequential_loop_exits`) and by the end-to-end examples, whose
  goldens changed only structurally (`if c { return a } else { rest }` became `if c { return a } rest`).
* **Size**: `rejoin_tests` asserts the code after five sequential statements is emitted once, and that ten
  statements emit less than four times the text of four, for every shape and context of the table.
* **Types**: unchanged. The es-toolkit avoidable `SmeltUnknown` count fell (a duplicated tail carried its
  erasures once per copy).

Planted defects, each detected:

* `emit_match` without the post-dominator join: `rejoined_branches_emit_their_shared_tail_once` and
  `rejoined_branch_emission_grows_linearly` fail.
* The "reaches the exit" test read from the post-dominator set: a block with no path to the exit keeps the full
  set, which holds the exit node too, so it passed, and the per-branch table named a block inside a loop
  (es-toolkit `compat/array/some`: "closes a cycle no loop was opened for").
  `switch_whose_cases_all_return_keeps_its_loops` fails.

## 3. Related fix

An unlabelled `break` below a `case`'s top level (`case 1: if (c) break; ..`) left the enclosing *loop*: the TS
frontend lowered it as a plain `Break`. It now leaves the `switch`: the `Match` is lowered inside a labelled region
the `break` targets (`stmt/switch_statement.rs`, `switch_break_label`); a loop inside the case clears the label,
and a nested `switch` sets its own, so their `break` stays theirs.

A `switch` lowered as a `===` chain (fallthrough, or labels that are not literals) was a single-iteration
`while (true) { ..; break }` loop, so a nested `continue` bound to it: `case 0: if (c) continue;` re-ran the chain
instead of continuing the enclosing loop (silently wrong), and a top-level one was refused. The chain now sits in
the same kind of labelled region, which every `break` of the switch leaves (`stmt/switch_fallthrough.rs`); a
`continue` binds to the enclosing loop at any depth. Node parity: `switch_chain_continue`.

## 4. Not covered

* A shared `return`/`throw` reached by two arms before the join (two `case`s falling into one `return`) is an exit:
  such a branch keeps the per-arm emission. It does not compound across sequential statements.
