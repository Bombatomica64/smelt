# Round 34 item 1 — nested function declaration hoisting (Agent K)

Branch: `worktree-agent-a1f95687cf895f491` (base: `main` + the round-34 brief commit).

## What landed

1. **The hoisting fix itself** — `a6b308a2` cherry-picked from
   `origin/worktree-agent-aefb17f85813045db` (the WIP `afaf178e` with the `SMELT_NO_HOIST`
   switch was NOT taken). It lowers a nested `function` declaration just before the first
   sibling statement that mentions its name, and binds the declaration's own name inside
   its body to the enclosing body's local. Hono's `E0425 dispatch` is gone.

2. **The es-toolkit ratchet regression it caused, fixed.** The `+4` came from
   `curry.rs`/`curryRight.rs` line 100:

   ```rust
   let smelt_capture_wrapper: Rc<RefCell<SmeltErasedFunction>> =
       Rc::new(RefCell::new(SmeltErasedFunction { callback: Rc::new(move |_smelt_args: Vec<SmeltUnknown>| SmeltUnknown::Null), length: 0.0, object: None }));
   ```

   Diagnosis differs from the brief's hypothesis. The knot ALREADY carries the binding's own
   type: both cell emitters (`emitter/core.rs::emit_mutable_local_preludes` and the two
   `control_flow.rs` sites) annotate the cell with `type_text_with_impl_trait(local.ty)`. Fixture
   112 proves it: a self-recursive `function` declaration and a self-recursive arrow emit
   `Rc<RefCell<Rc<dyn Fn(f64) -> f64>>>`, `Rc<RefCell<Rc<dyn Fn(f64, String) -> String>>>` and
   `Rc<RefCell<Rc<dyn Fn(String) -> String>>>` — no `SmeltUnknown` anywhere.

   es-toolkit's `wrapper` is genuinely erased in the SOURCE: `function wrapper(this: any,
   ...providedArgs: any[])`, which is exactly what `SmeltErasedFunction` carries. What the four
   tokens actually were is the fabricated PLACEHOLDER the cell holds until the real closure is
   built — runtime machinery, not program data. It is now factored into a prelude
   `impl Default for SmeltErasedFunction`, and both `default_value` sites emit
   `SmeltErasedFunction::default()` (`emitter/types.rs::ERASED_FUNCTION_DEFAULT`). That removes
   the same fabricated tokens everywhere else they appeared too, so the ratchet FALLS.

3. **Three real bugs the now-live `dispatch` body exposed**, all general:
   - `Context.req` is a getter-only accessor, and `context.req.routeIndex = i` makes MIR
     synthesize a writeback through it (`PlaceWritebacks`). `descriptor_setter_statement`
     hard-errored ("materialized descriptor write has no source setter") and the whole Hono
     crate failed to emit. `tsc` rejects a source assignment to a getter-only accessor, so such
     a write is always the synthesized writeback, and through a getter it is always a no-op
     (JavaScript returns the object by reference). It now emits a one-line note instead.
   - Emitter snippets bound `index`/`len`/`normalized` INTO user bodies. `compose` captures a
     mutable `index`, and `replace_shared_capture_uses` rewrote the emitter's own binding into
     `let (*smelt_capture_index.borrow_mut()) = ..` — not a pattern, 7 parse errors. The snippet
     locals are now `smelt_index`/`smelt_len`/`smelt_normalized` (reserved prefix, no source
     identifier can reach them).
   - The async-closure capture prelude did not recognise a source binding that already renders
     as an enclosing shared cell, so `() => dispatch(i + 1)` emitted `let dispatch =
     dispatch.clone();`, rewritten into the same invalid pattern. It now clones the CELL, the
     way the sibling wrapper prelude does.

## Not fixed — the remaining 8 Hono errors (a MIR capture-frame defect)

`compose.rs` has 8 errors (6 E0308 `expected f64, found Context_1`, 2 E0599 `no method len on
Context_1`). They are NOT about hoisting; the hoisting fix only made the body live enough to
reach them. `smelt dump-mir` on a minimal repro
(`(steps) => (label) => { return walk(0); function walk(i) { .. steps.length .. label .. } }`):

```
closure ClosureId(0)  %0 param steps
closure ClosureId(1)  %0 param label      captures []      <-- steps is NOT captured here
closure ClosureId(2)  %0 param i          captures [copy %0, copy %2, copy %0, copy %1]
```

`ClosureId(1)` does not capture `steps` at all even though its body needs it, and
`ClosureId(2)` records TWO different values (`steps`, `label`) under the same source local
`%0`: a transitive capture is not threaded through the intermediate closure, and the capture's
`source_local` is resolved in the wrong frame. Every name-level repair is therefore hopeless —
codegen cannot tell the two `%0`s apart. Three emitter-level attempts (unique capture aliases
keyed by `target_local`, aliases suffixed with the local id, renaming a colliding closure
parameter) were written, verified insufficient against the repro, and reverted.

The fix belongs in `crates/smelt-mir/src/lower` closure-capture lowering: an inner closure's
capture of a grandparent local must first be captured by each intermediate closure, and the
recorded `source_local` must name the IMMEDIATELY enclosing frame's local.

## Numbers

| gate | before | after |
| --- | --- | --- |
| es-toolkit avoidable erasure | 31645 (baseline) → 31649 with the raw cherry-pick | **31431** (−214), baseline re-snapshotted |
| es-toolkit legitimate / prelude | 40297 / 2619 | 40197 / 2587 |
| examples avoidable | 0 | **0** (prelude 66339 → 67330, stale-by-991 re-snapshotted) |
| Hono `cargo check` | 3 errors | 10 errors (see above) |
| remeda generated `cargo test` | 1789 / 0 | 1789 / 0 |
| radash generated `cargo test` | 84 / 84 | 84 / 84 |
| `cargo test -p smelt-codegen-rust` | — | 1079 passed / 0 failed |
| `cargo test -p smelt-frontend-ts --no-default-features` | — | 1096 passed / 0 failed |
| `cargo test --bin smelt` | — | 57 passed / 0 failed |
| `hir_cli_cross_language_tests` | — | 17 passed / 0 failed |

Hono per-code, measured on a clean clone at `eebdf7be…` + `.github/compat/hono/.` with a fresh
full-feature binary and a repo-root absolute `--manifest-path`:

| code | site | owner |
| --- | --- | --- |
| E0308 ×6 | `compose.rs` (`dispatch(i)` gets the `Context`) | MIR capture frames — unowned |
| E0599 ×2 | `compose.rs` (`middleware.length` on the `Context`) | same defect |
| E0425 ×1 | `main.rs` `__smelt_fn_value_627` | Agent L |
| E0609 ×1 | `main.rs` `router` on `Hono` | Agent L |
