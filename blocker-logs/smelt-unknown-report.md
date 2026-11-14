# SmeltUnknown usage metric

_Produced by `smelt smelt-unknown-report` (crate `smelt-transpiler`, module `unknown_report`)._

This report quantifies how much generated Rust falls back to the erased
`SmeltUnknown` tagged value, so the reduction expected from concrete unions,
generics, and flow narrowing (PR #47 wave) is a measured number rather than a
projection.

## What it measures

The tool scans generated `.rs` files and counts every `SmeltUnknown` occurrence,
classifying each into one of three categories:

- **Runtime prelude** — the fixed `SmeltUnknown` enum, its trait impls, and the
  `smelt_*` helper functions emitted into every crate. Shared scaffolding, not
  program-specific erasure. The emitter writes a stable sentinel comment
  (`// @smelt:prelude-end — generated program below`) between this prelude and
  the program body; the scanner treats everything above it as prelude
  authoritatively. A module file of a multi-file crate carries no prelude at
  all and opens with a `// source: <file>` header (`MODULE_SOURCE_HEADER`);
  every line of such a file is program code, so a program item whose name the
  emitter minted (a module global's `smelt_global_init__x__0`) is measured like
  any other. Only output that carries neither sentinel falls back to the
  per-line `fn smelt_*` heuristics. (Before 2026-11, module files went through
  that heuristic and their global initializers were counted as prelude.)
- **Legitimate boundary** — genuine dynamic edges: erased closures/interop
  (`SmeltUnknown::Function`), promises (`SmeltUnknown::Promise`), explicit
  boundary adapters (`IntoSmeltUnknown` / `into_smelt_unknown`), and runtime
  narrowing guards. These are sanctioned uses of `SmeltUnknown`.
- **Avoidable erasure** — everything else: program value storage, function
  signatures, and collections where a concrete type, a generated union, or a
  scoped generic could have carried the static shape. **This is the number the
  metric is designed to drive down.**

Heuristics are conservative: an ambiguous line is counted as *avoidable*, so the
metric never hides erasure — the worst case is over-reporting, which is safe for
a gate that must not silently regress. The classification rules live and are
documented in `crates/smelt-transpiler/src/unknown_report/classify.rs` (`classify_line`).

## Methodology

Counting is textual and deterministic:

1. Roots are expanded to a sorted, de-duplicated list of `.rs` files
   (directories walked recursively; `target/` skipped).
2. Each line is scanned for `SmeltUnknown` tokens and classified. Pure
   fallibility-annotation spellings — the `Ok::<SmeltUnknown, Box<dyn
   std::error::Error>>` return turbofish and `-> Result<SmeltUnknown, ...>`
   return annotations — are stripped before counting: they annotate types, not
   erased values, so wrapping a function in `Result` stays count-neutral.
3. Occurrences are grouped by a normalized *shape* (string/char literals →
   `"S"`/`'C'`, digit runs → `N`, whitespace collapsed; identifiers kept), then
   sorted by count descending with category and shape as tie-breakers.

Because it needs no compiler, the scan works on any generated crate — even ones
that do not yet type-check — and diffs cleanly between runs.

### Reproducing

```sh
# Human-readable report for a generated crate or corpus directory:
smelt smelt-unknown-report path/to/generated/src

# Machine-readable baseline (also the format `--baseline` consumes):
smelt smelt-unknown-report examples --format json --output blocker-logs/smelt-unknown-baseline.json

# Regression view against the committed baseline (exits 0 whatever it finds):
smelt smelt-unknown-report examples --baseline blocker-logs/smelt-unknown-baseline.json

# The gate CI runs (exits 1 on a regression, see "Regression gating"):
smelt smelt-unknown-report examples --baseline blocker-logs/smelt-unknown-baseline.json --fail-on-regression

# Re-snapshot after avoidable erasure fell:
smelt smelt-unknown-report examples --baseline blocker-logs/smelt-unknown-baseline.json --update-baseline
```

Paths in examples are recorded relative to the working directory, so a corpus
scanned from the repo root produces a byte-identical baseline on any machine.

## Baselines

There is one committed baseline per corpus. The current counts are in the SmeltUnknown table of
the root `README.md`; the gating policy is in `AGENTS.md` (`## SmeltUnknown enforcement`).

| Baseline | Corpus scanned in CI | CI step |
| --- | --- | --- |
| `smelt-unknown-baseline.json` | `examples` (the whole checked-in example tree) | `corpus` job, blocking, `--fail-on-regression` |
| `smelt-unknown-baseline-es-toolkit.json` | es-toolkit's generated `dist-smelt/src` | `es-toolkit-regression` job, blocking, `--fail-on-regression` |
| `smelt-unknown-baseline-remeda.json` | Remeda's generated `dist-smelt/src` | `remeda-regression` job, advisory (`\|\| true`) |
| `smelt-unknown-baseline-hono.json` | Hono's generated `dist-smelt/src` | `hono` job, advisory (`\|\| true`) |

All four run in CI's full mode (pushes to `main`, nightly, manual dispatch, PRs labelled
`run-regressions`). es-toolkit, Remeda and Hono each emit a whole Rust crate, which those jobs
build and scan. The advisory steps still fail when their baseline file is missing.

The example programs are small and hand-audited, so the examples corpus holds **zero** avoidable
erasure, and any appearing there is a lowering regression with a readable reproduction.

## Regression gating

Without `--fail-on-regression`, `--baseline` only adds a "Change vs baseline" section and the
command exits 0. With it, the command exits 1 when:

- avoidable erasure rose above the baseline's total, or
- the scan cannot judge the baseline's corpus: it found no `.rs` file, or it scanned more than
  10% fewer files than the baseline did (`coverage_loss` in
  `crates/smelt-transpiler/src/unknown_report/baseline.rs`).

A rise in *legitimate-boundary* occurrences never fails the gate, so a documented boundary
adapter does not block CI. `--update-baseline` rewrites the baseline only when the scan did not
lose coverage. Enforcement policy, including when a rise may be reclassified, lives in `AGENTS.md`
(`## SmeltUnknown enforcement`).

## Erasure reasons (decision-time records)

When a `smelt-erasure.json` sidecar sits beside the scanned `src/` (written by
`smelt build`) or is passed with `--erasure <path>`, the report adds an
"Avoidable erasure by reason and source line" section: every erasure decision
the frontends and the backend recorded, counted by reason, and the avoidable
ones grouped by source line. The per-generated-line classification above is
unchanged, so the three baselines keep their meaning; baselines written without
a sidecar parse unchanged (`erasure` is optional). Reasons, classes and
severities: `docs/erasure-reasons.md`.
