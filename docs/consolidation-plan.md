# Consolidation plan: shared stdlib lowering, explicit MIR decisions, lean tests

Measured 2026-11-14 at `3b9d15fa8` by four read-only auditors (TS stdlib, Python
stdlib, emitter, tests). Nothing was compiled. Claims marked *(unverified)* are
inferred from structure, not read end to end. Prior record:
[`docs/untangle-decision-log.md`](untangle-decision-log.md); this plan does not
re-propose its done items and cites its open ones.

## Why

~420k lines of Rust, ~134k (32%) of it tests. The middle of the pipeline is thin
(`smelt-hir` 11k, `smelt-mir` 25k) and the ends are heavy (`smelt-frontend-ts`
146k, `smelt-codegen-rust` 136k). Two structural causes, both of which make each
new language cost the sum of the parts instead of the marginal part:

1. **Stdlib lowering lives per frontend.** The language-neutral ops already exist
   in HIR (`crates/smelt-hir/src/op_table.rs`, ~200 ops: `StringCase`,
   `StringTrim`, `ListSort`, `DictGet`, ...) and both frontends already emit 82 of
   them. What is duplicated is everything in front of the op: spelling match,
   receiver gating, argument adaptation, result type. TS does it in
   `lowering/stdlib/` (26k); Python in a source-ordered chain of ~30 handlers
   (`crates/smelt-frontend-py/src/lowering/call.rs:176-282`) each hard-coding
   `attr != "x"`. `smelt-stdlib` is TS-keyed metadata (`typescript_method_rule`,
   `TYPESCRIPT_METHODS`); Python touches it only through a 12-entry string match
   (`stdlib_dispatch.rs:9`).
2. **MIR drops representation intent; the emitter reconstructs it.**
   `ExprKind::TypeAssert` lowers to `Rvalue::Use` (`smelt-mir/src/lower/expr.rs:337`).
   MIR has no coercion kind, union arm, numeric kind or adapter plan, so the
   emitter re-derives them from `(operand_ty, dest_ty)`: `binary_expr` is a ~20-rung
   probe ladder (`emitter/binary_ops.rs:1811`), `inject_union_value` picks arms
   (`emitter/union.rs:336`), `function_adapter.rs` plans adapters. This is where
   `SmeltUnknown` creeps in (992 mentions in the emitter) and why
   `seam_assertions.rs` exists. Note: `smelt-specialize` is the host-runtime
   manifest, not a type pass; generic decisions live in codegen
   (`generic_bindings.rs`, `generic_lifting.rs`, `generic_elision.rs`, ~3.8k).

## Invariants for every step

- TS output byte-identical for pure moves: example goldens
  (`examples/typescript/end-to-end/*/expected.{hir,mir,rs}`) and the corpus oracle.
- Typedness and `SmeltUnknown` gates unchanged or better; recovery paths
  (`UnknownCast`, `TypeAssert`) move verbatim, never rewritten in a move PR.
- Python goldens change only where Python newly accepts source; each new
  behaviour gets a runtime test against CPython output.
- No special cases: spelling tables are data, semantics are general rules
  (`CLAUDE.md`).

## Workstream A: shared stdlib lowering, Python attached (priority)

Target shape. A new crate `smelt-stdlib-lowering` (depends on `smelt-hir` and
`smelt-stdlib`; it cannot live in `smelt-stdlib` because `smelt-hir` depends on
it):

- `ReceiverClass` (`Text`, `List`, `Dict`, `Set`, `Tuple`, `Number`, ...), derived
  from the HIR type; `PyStr` and `String` both map to `Text` via `Type::is_text`.
- Per-language spelling tables `(Lang, ReceiverClass, name) -> (SemOp, ArgShape)`:
  TS `toUpperCase` and Python `upper` both resolve to `StringCase(Upper)`.
  `ArgShape` covers required/optional/callback(arity)/keyword (`key=`, `sep=`,
  `maxsplit=`) and per-language defaults.
- `trait LowerCtx` implemented by each frontend: `expr_ty`, `intern`,
  `push_expr`, `coerce_to`, `lower_callback`, `text_ty`, `erased_receiver_ok`.
  Frontends keep AST walking (oxc / ruff) and hand over lowered `ExprId`s.
- `lower_semantic(op, receiver, args, ctx)`: receiver recovery, op build, result
  type (text type taken from the receiver, never hard-coded), span.
- Failure semantics stay explicit: where Python raises and JS returns
  `undefined`/`-1`, the op carries a flag (`ListPop { raises_when_empty }`) or a
  dedicated `Python*` op. Those ops are correct as they are.

Split of today's 26k TS stdlib lines *(unverified, sampled)*: ~5-6k
language-neutral (moves), ~11k JS/host-specific (Promise, fetch, Buffer, Date,
prototype quirks; stays), ~10k oxc/checker plumbing (stays).

| Step | Content | TS lines moved | Python gains |
|---|---|---|---|
| A1 | Crate skeleton, `LowerCtx`, tables; pilot `trim`/case/affix | ~250 | `strip`/`upper`/`startswith` through the shared path |
| A2 | Python `PYTHON_METHODS` table replaces the order-dependent chain; one shared builtin-shadowing check (decision log: `abs`/`len`/`print` ignore user definitions) | 0 | correctness |
| A3 | Strings remainder: split, pad, repeat, slice, search, replace, join | ~600 | `split()`/`maxsplit`, `strip(chars)`, `ljust`/`rjust`/`center`/`zfill`, `count`, `title`, `capitalize`, `isspace`, `splitlines`, `partition` |
| A4 | Math, numbers, primitive casts | ~700 | `round()` half-even, `int()`/`float()` parsing parity, `pow` on ints |
| A5 | Lists + `callbacks/list_ops` | ~1.3k | `min`/`max(key=, default=)`, `enumerate(start=)`, n-ary `zip`, `map`/`filter` on named functions, `index`/`count(start, stop)` |
| A6 | Collections (Map/Set to Dict/Set), JSON, neutral `Object.keys/values/entries`; one `is_json_serializable_type` (three copies today: ts `json.rs:335`, py `stdlib_types.rs:3`, codegen `type_facts.rs:536`) | ~1.2k | `dict` views and iteration parity |
| A7 | Shared `StringFormat` op: f-string format specs (`{x:.2f}`, `{x=}`, `!a`), `str.format`, TS `toFixed`/`padStart` reuse | n/a | f-strings with specs, rejected today (`literals.rs:436`) |
| A8 | Python `isinstance` (incl. tuples, builtin types) on the instance-of registry edges | n/a | `isinstance` |
| A9 | Frontend twins to `smelt-hir`/shared: `materialized_static_type` + specialization helpers (py `specialization.rs:1081`, ts `:767`), `type_assignable_to`, `list_element_type`, `erase`, console-log item | dedup | n/a |

Not translating, by design: arbitrary-precision `int`, lazy generators, the
`Python*` read/division ops, raising list/dict methods, JS host objects. Later:
`collections.Counter`/`defaultdict`/`deque`, `functools`/`itertools`.

## Workstream B: explicit representation decisions in MIR

Independent of A (touches `smelt-mir` + `smelt-codegen-rust`, not frontends).

| Step | Move | Emitter lines out *(est.)* | Risk |
|---|---|---|---|
| B1 | Representation table: `Concrete / Generic / Erased` per `TypeId`, computed once; typedness reads it instead of emitter predicates | ~300, unblocks B2-B4 | low-medium |
| B2 | `Rvalue::Binary` gets a resolved `BinaryKind` (F64, I64, FixedInt, BigInt, StrConcat, DynAdd, OptionalEq, ...) classified in MIR from today's ladder, order preserved | ~1000 | medium |
| B3 | `Rvalue::Coerce { kind }` (InjectUnion{arm}, ProjectUnion{arm}, Erase, Extract, Widen); `TypeAssert` keeps its intent; MIR validator rejects type-mismatched `Use`; `Erase`/`Extract` become the only `SmeltUnknown` sites | ~2100 (union + coercion) | high |
| B4 | `AdapterPlan` in MIR for function adapters (param conversions, return wrapping, rest packing, by-ref params) | ~1100 + part of `closures.rs` | high |
| B5 | Unify closure-body and function-body block emission; isolate JS semantics (string index units, typeof tags, `Function.length`, loose equality) behind named ops | ~500 | low-medium |

#187 (typed Rust AST) comes after B3: before it, the AST would encode the
type-driven branching verbatim.

## Workstream C: tests

| Step | Move | Lines *(est.)* |
|---|---|---|
| C1 | `smelt-test-support` dev crate: `lower_ts`, `lower_py`, `emit`, `ensure!`, `TestResult`, `class_named`; the pipeline helper is re-implemented in 24+ codegen test files | -2k |
| C2 | `lower_case(src)` fixture for frontend tests (381 `HirCtx::new()` in `lower_calls_packing_tests.rs` alone; ~25-30% of each test is setup) | -8 to -10k |
| C3 | `rust_items` + `assert_eq!` emitter tests to `insta`/example goldens; fix assertions that cannot fail (any-over-crate `ClosureCall`/`Closure` checks, `!source.contains(..)` that pass on empty output, prelude-wide substring checks; list in the PR) | -3k+, stronger |
| C4 | Optional: HIR `Assertion` node shared by TS matchers and pytest `assert` (deep equality, failure messages) | refactor |

Never weakens coverage (`docs/verification-and-audit.md`): every deleted test is
either folded into a table/golden that checks the same thing or shown unable to
fail for a defect.

## Workstream D: file splits and dead code

Split along named concerns: `testing/matchers.rs` (2.8k), `expr/references.rs`
(2.4k), `emitter/strings.rs` (strings vs regex), `emitter/map.rs`,
`testing/suites.rs`, `stdlib/fetch_types.rs` (by Web type). `emitter/union.rs`
and `binary_ops.rs` wait for B2/B3. Delete zero-caller pub fns
(`smelt-mir/src/traversal.rs:707`, `definite_assignment.rs:78`,
`smelt-hir/src/type_normalize.rs:41`, others in the PR) and close the stale
`blocker-logs/` reports (`OPEN-QUESTIONS.md`, `hono-phase3-round1-families.md`,
`express-v1-baseline.md`, `hono-crud-status.md`) into `docs/`.

## Order

1. C1, D (cheap, make the rest easier to review).
2. A1 → A2 → A3..A6 in sequence (each byte-identical for TS), A7/A8 after A3/A4.
   B1 → B2 → B3 → B4 in parallel with A (disjoint crates).
3. A9, C2, C3, B5 as capacity allows; C4 and #187 last.

Relates to #38 (roadmap), #93/#94/#95 (Python frontend), #187 (typed Rust AST).
