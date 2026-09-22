# Round 34 item 4 — the MIR closure-capture frame defect (Agent M)

Branch: `worktree-agent-af59679e92ed8930c` (base: `main` + round 34, merged with Agent L's work
at `230d4553`).

This is the 8 `compose.rs` errors `hono-round34-hoisting.md` left unowned: 6 `E0308`
(`expected f64, found Context`) and 2 `E0599` (`no method len on Context`).

## Diagnosis

The defect was NOT in `crates/smelt-mir/src/lower`. MIR faithfully lowers the capture list HIR
hands it (`lower_closure_expr` maps each `ClosureCapture::source_local` through the current
frame's `locals`), so the wrong frame was already wrong in HIR. The capture list is computed in
the frontend, by an AST walk that runs BEFORE the closure body is lowered:
`ModuleBuilder::collect_statement_capture_names` in
`crates/smelt-frontend-ts/src/lowering/callbacks/body_lowering.rs`.

That walk had arms for a function EXPRESSION and an arrow, so a name read inside a nested arrow
was reported to the enclosing frame and captured there. It had **no arm for
`Statement::FunctionDeclaration`** — nested function declarations fell into the `_ => {}`
catch-all. Consequences, in order:

1. A name read only inside the declaration was invisible to the frame containing it, so that
   frame captured nothing (rule 1 violated: the capture was not threaded).
2. When the declaration itself was lowered, its own capture resolved the name through
   `self.scope`, which is a stack that still holds the enclosing frames' bindings. The lookup
   succeeded and returned a local id belonging to a FURTHER-OUT body (rule 2 violated).
3. Local ids are per-body, so that stale id still indexed a local in the enclosing body — a
   different one. In the minimal repro a grandparent parameter and a parent parameter were both
   `%0` of their own frames, so the innermost closure recorded two different values under one
   source local, exactly as the hoisting note observed:

```
closure ClosureId(1)  %0 param label      captures []
closure ClosureId(2)  %0 param i          captures [copy %0, copy %2, copy %0, copy %1]
```

## What landed

1. **Rule 1, where the capture list is actually computed.** A
   `Statement::FunctionDeclaration` arm in `collect_statement_capture_names`, walking the
   declaration's body exactly as the function-EXPRESSION arm does, with the declaration's own
   name added to the shadowed set (it is bound by the statement, not captured). Rule 2 then
   falls out: each intermediate frame binds the threaded name to its OWN capture local before
   lowering its body, so the inner lookup resolves in the immediately enclosing frame.

   After the fix, the same repro:

```
closure ClosureId(1)  %0 param label      captures [steps]
closure ClosureId(2)  %0 param i          captures [label, steps, walk]   (%0, %1, %2 — distinct)
```

2. **Rule 2, enforced.** `smelt_hir::validate` now reports a capture whose `source_local` lands
   on a differently NAMED local of the enclosing body. A stale id from a further-out frame
   almost always lands on a different named binding, and that is now a validation error rather
   than silently-wrong codegen. Only named-to-named mismatches are reported, so synthesized
   captures over unnamed temporaries stay silent.

3. **The emitter half of the same rule.** Every closure parameter is emitted as
   `closure_arg_{index}`, so a three-deep closure captures its parent's `closure_arg_0` AND its
   grandparent's — two distinct values under one identifier. The alias rule only compared a
   capture's source name against the closure's own PARAMETER names, and the capture prelude
   dedupes by name, so the second binding was dropped and both reads resolved to whichever
   value was bound first (`smelt_captured_closure_arg_0.len()` on a `String`).

   `FunctionEmitter::capture_aliases` (new, `emitter/closures.rs`) now chooses one distinct
   binding per captured local against ALL names the frame has claimed: the parameter names and
   every capture's source name. The second claim takes `smelt_captured_<name>`, and when that is
   also claimed the captured local's id is appended. Claiming the source names up front matters:
   prelude lines are emitted in order in the enclosing scope, so an alias that reuses another
   capture's source name shadows the very binding the next line clones from (the first attempt
   did exactly that and produced `let a = a.clone()` where `a` had just been rebound).

   This is not one of the three emitter workarounds the hoisting note reverted — those tried to
   tell two colliding MIR captures apart, which is impossible. With the capture lists correct,
   the emitter only has to give distinct locals distinct names.

Nothing here special-cases a library, a function name, or a spelling.

## Tests

* `examples/typescript/end-to-end/115_transitive_closure_captures/` — three-level nesting in
  four shapes: arrow → arrow → hoisted SELF-RECURSIVE `function` declaration using both the
  grandparent's and the parent's parameter; the same with a NON-recursive declaration; the same
  through arrows only; and a grandparent `const` of the outer body read only by the innermost
  frame. `expected.stdout` was diffed against Node 22 (`node --experimental-strip-types`) and
  the generated crate reproduces it byte for byte. Registered in `END_TO_END_EXAMPLES`.
* `crates/smelt-codegen-rust/src/tests/closure_capture_frame_tests.rs` — asserts the capture
  LISTS per closure level (threading, and that no closure records two values under one source
  local), plus the emitted-name half.

The fixture was confirmed to reproduce the wrong MIR (`ClosureId(1) captures []`, two `%0`s in
`ClosureId(2)`) before the fix.

## Hono

Clean clone at `eebdf7be` + `.github/compat/hono/.`, fresh full-feature binary, repo-root
absolute `--manifest-path`, `cargo clean -p hono_probe` before the measurement.

| code | site | before | after |
| --- | --- | ---: | ---: |
| E0308 | `compose.rs` (`dispatch(i)` gets the `Context`) | 6 | **0** |
| E0599 | `compose.rs` (`middleware.length` on the `Context`) | 2 | **0** |
| E0425 | `main.rs` `__smelt_fn_value_627` (Agent L) | 1 | **0** |
| E0609 | `main.rs` `router` on `Hono` (Agent L) | 1 | **0** |
| | **total** | 10 | **0** |

33 modules, 418 warnings, **0 errors**. `cargo build` on the generated crate **links**:
`target-gen-priv/debug/hono_probe` is produced, exit 0, no link-stage errors.

## Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable **0** (+0); prelude 67330 → 68321 (+991, fixture 115's own prelude), re-snapshotted in this commit |
| es-toolkit ratchet (`--fail-on-regression`) | avoidable **31431** (+0), legitimate 40197 (+0), prelude 2587 (+0) — unchanged, no re-snapshot needed |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1096 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | 1083 passed / 0 failed |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-mir` | 55 passed / 0 failed |
| `hir_cli_cross_language_tests` | 18 passed / 0 failed (whole corpus, goldens regenerated) |
| `cargo clippy --all-targets` | 0 errors; the only findings in files touched are the repo-wide pedantic `expect()`-in-tests lint that every existing test file trips |

## SmeltUnknown delta

Net **zero avoidable** everywhere it is measured: examples 0 → 0, es-toolkit 31431 → 31431,
remeda's advisory baseline FELL (avoidable 24873 -> 24167, -706, with Agent L's merged work in the same measurement) and was re-snapshotted in this commit. Nothing in this change introduces a `SmeltUnknown` conversion — it removes
erasure pressure if anything, since the captures it repairs now carry their concrete types
(`List<String>` instead of colliding with a `String`).

## Found, not fixed

* `collect_statement_capture_names` still has a `_ => {}` catch-all. `Statement::ClassDeclaration`
  is the remaining declaration form whose body can read an enclosing binding (a field
  initializer or a method body); a class declared inside an arrow and referring to an outer
  local would hit the same shape. No corpus reaches it today, so it is recorded rather than
  fixed blind.
* A nested function declaration SHADOWS an outer binding of the same name for its sibling
  statements. The capture scan is per-statement, so a sibling that mentions the name before the
  declaration is lowered can still report it as a capture of the outer binding. Hoisting makes
  this observable only for a name that is both declared as a nested function and bound outside;
  nothing in the corpora does that.
