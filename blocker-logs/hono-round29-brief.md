# Round 29 — MIR/IR-correctness first (orchestrator brief)

Ruling from the user: before the frontend families that dominate the whole-crate
`cargo check` table (type-parameter defaults, H20, H21), fix the items where the
IR itself carries a wrong or inconsistent answer. Two Opus agents, separate
worktrees, both compile. Read `blocker-logs/implementer-brief.md` first; every
rule there applies (fresh binary for every measurement, fixture numbers taken at
commit time — next free is 89 — and listed in `END_TO_END_EXAMPLES` in the same
commit, push the worktree branch after every commit, `rm -rf` your targets when
done).

Integration head: `claude/estoolkit-test-failures-4fuf9e` @ `9dac456e`.
Whole-crate Hono `cargo check` on that head: 326 errors (`hono-current.md`).

## Agent A — MIR semantics: H47, H48, H70, H69, H14

Order is priority order. Each item lands as its own commit with its own fixture.

1. **H47** (`hono-h47-out-of-range-element-read.md`). Ruling: option 1, *throw*.
   A constant or variable index read on a list with a CONCRETE element type that
   misses is `undefined`; the read's type is `Optional(T)` in MIR where the
   consumer can absorb it (`x ?? d`, truthiness, `=== undefined`, optional
   chain), and every consumer that needs a `T` (spread, member read, arithmetic,
   call) throws `TypeError` through the existing fallible-terminator machinery
   (`hono-fallible-ops.md`, round-28's call-terminator unwind edges) instead of
   defaulting. Tuple constant indexes past the tuple's arity are `tsc` errors
   and need no runtime path. Delete `array_hole_value`'s concrete-type default
   arm; the erased-element arm stays. Measure: examples invariant must stay 0 —
   an `Optional(T)` read is a concrete type, not an erasure.
2. **H48** (`hono-h48-contextual-rest-tuple.md`). Land site 5 together with the
   feature, as the note requires: the `ExprKind::Index` arm in
   `smelt-mir/src/lower/expr.rs` treats a constant index on a TUPLE receiver as
   a field position, never an optional read. Finish half (b) with the use-site
   condition the note calls "necessary but not sufficient": the binding is typed
   as the contextual tuple only when every use is a spread into a call or a
   constant-index read; any other use (passed as a value, `.length`, iteration)
   keeps the variadic list. Witness the site-5 fix with a MIR-dump fixture whose
   `expected.mir` shows `%n[1]` and not `%n?.[1]`.
3. **H70** (`hono-h66-promise-continuations.md` §Layer 3). A throw inside an
   awaited erased call must reach the enclosing source `try`/`catch`. Same
   family as round 26's optional-chain gap: extend the throwing-propagation pass
   (`smelt-mir/src/lower/passes/throwing.rs`) so an `await` of a may-throw
   operand is itself a fallible terminator with an unwind edge into the
   enclosing handler. Fixture: the two-module radash-shaped `guard.ts`/`main.ts`
   reproduction; expected stdout `user1|default-user|unknown error`.
4. **H69** (`hono-h68-class-expression-binding.md` §Found on the way).
   `synthesize_default_class_constructor` derives the synthesized constructor's
   parameter list from the base class's own constructor instead of one
   `Option<SmeltUnknown>` super argument. Prelude/host bases with variadic
   constructors (Date-like) keep the current shape; that is a real dynamic
   boundary and stays documented at the emit site. Fixture: a derived class
   with no constructor over a no-parameter base, constructed and used, with the
   report showing the 4 avoidable lines gone.
5. **H14** (campaign plan rows 26/60/102). Reproduce with the recorded shape
   (`Code = 200 | 404` literal union + generic interface reaching
   `call_runtime.rs:2145`), then make the index/field fallback at
   `emitter/types.rs:823` degrade at the site — resolve the literal operand's
   base primitive, or fail with `EmitError::with_site` naming the operand — and
   never intern `Type::Unknown` as the fallback (that flips `needs_unknown_type`
   crate-wide). Fixture asserts the program transpiles and runs.

After each item: `cargo check --lib --no-default-features`, focused tests. At
the end: all gates from the brief, and re-run the Hono whole-crate `cargo check`
(clone at `eebdf7be39abf0a872671835ccce0c4f03ea497a`, copy `.github/compat/hono/.`,
`smelt build`, `cargo check` in `dist-smelt` with `CARGO_TARGET_DIR=target-gen-priv`)
and report the per-code table against the 326 baseline. Write
`blocker-logs/hono-round29-mir.md` (allowlisted in `.gitignore`).

## Agent B — H42 architecture pass (campaign plan §D2 / H42, option 1)

Thread a `TypeSubstitution` into the value-rendering recursion in
`smelt-codegen-rust/src/emitter/coercion.rs` so a render position's erasure is
decided ONCE and used by both the annotation and the entries. `value_at_type_text`
(~190 call sites) and `extract_value_text` (~28) both take the substitution;
`target_is_erased`'s unconditional bare-`TypeParam` erasure and
`extract_value_text`'s scope-respecting arm are replaced by one query on the
threaded substitution: a `TypeParam` in the current function's lexical scope
renders as `T` on every side, one outside it erases on every side. H41a's
unconditional turbofish erasure in `collected_container_type_text` becomes
precise again (uses the same substitution). Do the wide signature change
mechanically first (pass the current function's substitution everywhere, zero
behaviour change, all goldens byte-identical), commit, then change the rule in a
second commit so the two diffs review separately.

Acceptance: the Hono router slice (`hono-router-slice-round26.md` names the
files) goes from 6 errors to 0; the whole-crate table's E0277 (7) and the
`expected type parameter T, found SmeltUnknown` E0308s fall, with the per-code
table reported against 326. Examples invariant 0; es-toolkit ratchet equal or
lower (a fall re-snapshots in the same commit); remeda 1789/0; radash 84/0.
Write `blocker-logs/hono-h42-erasure-substitution.md` (allowlisted).

Do not touch the frontend type-parameter-defaults family, H20 or H21; they are
the next round.
