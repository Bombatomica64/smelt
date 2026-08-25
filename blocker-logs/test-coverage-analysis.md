# Test Coverage Analysis

Analysis date: 2026-08-08. Source: the committed CI coverage artifacts
(`coverage-summary.json`, `lcov.info`, produced by
`cargo +nightly llvm-cov --workspace --branch`) plus static inspection of the
test corpus and the CI workflows.

This report is about **Smelt's own test suite**. It is not about
`Test-TODO.md`, which tracks transpiling *source-language* test suites into
Rust `#[test]` functions.

## Headline numbers

| Metric | Covered | Total | % |
|---|---:|---:|---:|
| Lines | 74,425 | 100,394 | **74.1%** |
| Regions | 115,976 | 160,174 | **72.4%** |
| Functions | 4,781 | 6,126 | **78.0%** |
| Branches | 11,778 | 18,521 | **63.6%** |

**Branch coverage is the weak axis** — 6,743 uncovered branches. In a compiler
the branches *are* the product: each one is a lowering decision, a type case,
or an emitter variant.

The numbers are broadly honest. Test modules under `crates/*/src/tests/` are
excluded from the report entirely, and inline `#[cfg(test)]` blocks inside
counted files account for only ~3,861 of 100,394 counted lines (~3.9%). So the
74% is not meaningfully inflated by test code counting itself as covered.

### Per-crate

| Crate | Lines | Line % | Branch % | Uncovered lines | Uncovered fns |
|---|---:|---:|---:|---:|---:|
| smelt-frontend-ts | 46,367 | 75.8% | 65.5% | 11,225 | 426 |
| smelt-codegen-rust | 23,745 | 78.2% | 61.7% | 5,188 | 283 |
| smelt-frontend-py | 11,134 | 74.5% | 67.3% | 2,844 | 152 |
| smelt-mir | 8,639 | 76.5% | 68.0% | 2,026 | 103 |
| smelt-transpiler | 4,074 | 66.7% | 55.9% | 1,356 | 132 |
| smelt-specialize | 2,736 | 50.3% | **36.1%** | 1,359 | 180 |
| smelt-hir | 2,377 | **30.1%** | 55.0% | 1,662 | 60 |
| smelt-stdlib | 408 | 75.5% | 100.0% | 100 | 7 |
| smelt-test | 482 | 90.2% | 61.4% | 47 | 11 |
| smelt-py-types | 298 | 87.2% | 68.5% | 38 | 6 |

The two crates that look alarming — `smelt-hir` at 30.1% and `smelt-specialize`
at 36.1% branches — have specific, fixable causes, covered below. `smelt-mir`'s
*lowering and validation* are actually in good shape (`lower/expr.rs` 88.3%,
`validate/operands.rs` 94.6%); its deficit is concentrated in the formatter and
the optimizer.

---

## Gap 1 — The assertion oracle is substring matching

This is the most important finding, and it is about test *quality*, not test
*quantity*.

Across 2,102 test functions the suite uses:

- **1,991** `.contains(...)` substring assertions on emitted Rust
- **25** whole-module `insta` snapshot cases (via
  `assert_emitted_source_snapshot` / `assert_emitted_py_source_snapshot`)
- **19** `assert_eq!`

A `.contains()` assertion says "the emitted text mentions this fragment
somewhere". It passes on output that does not compile, output with the fragment
in the wrong scope, and output that is correct in shape but wrong in semantics.
It also silently keeps passing when surrounding code regresses, because it never
looks at the surroundings.

The repo already knows this. `crates/smelt-codegen-rust/tests/compile_corpus.rs`
says so in its own docstring:

> they assert on insta snapshots or `.contains()` substrings of the emitted
> Rust. That catches shape regressions but not whether the emitted Rust actually
> *compiles*. Precedence, erased-type (`SmeltUnknown`) and ABI bugs have
> historically only surfaced during full source-project regeneration.

**Proposal.** Do not attempt a mass rewrite of 1,991 assertions. Instead:

1. Adopt a rule for *new* emitter tests: whole-output `insta` snapshot by
   default, `.contains()` only when asserting a genuinely local property.
   The helper and the 450-line snapshot cap already exist and work well.
2. Convert the highest-traffic existing modules opportunistically — when you
   touch a `part_*_tests.rs` case for an unrelated reason, upgrade it.
3. Prioritise conversion where uncovered branches cluster (see Gap 2's file
   list) — those are the files where a shape-only oracle hides the most.

## Gap 2 — Generated Rust is compiled but almost never *executed*

There are three tiers of "does the output actually work" testing, and they are
weaker than they look:

- **`compile_corpus.rs`** — 52 cases, lowered through the real pipeline and run
  through `cargo check`. This is a genuinely good tier and CI runs it in a
  dedicated job. But it only *type-checks*: it never runs the program and never
  compares behavior against the TypeScript/Python original. It also carries a
  `KNOWN_COMPILE_FAILURES` exclusion (`async_await`: async bodies emit a bare
  `return <expr>` against a synthesized `Result<..>` return type).
- **Runtime tests** — `generator_runtime.rs`, `timer_runtime.rs`,
  `map_reference_runtime.rs`, and the two `*_specialization_parity.rs` files do
  execute. This is the right idea, but it is five files against a ~53k-line
  emitter.
- **`compat.yml`** — the only tier that runs a real library's own test suite
  against generated Rust. It is `workflow_dispatch`-only and defaults to
  `run_compat=false`. `COMPAT.md` currently shows **one row**: remeda, last
  updated **2026-07-10**. `es-toolkit` is in the matrix but has never published
  a result.

So the strongest behavioral signal the project has is opt-in, manual, and a
month stale.

**Proposal.**

1. Put `compat.yml` on a schedule (nightly or weekly `schedule:` trigger) in
   addition to `workflow_dispatch`, so drift is detected rather than discovered.
   Get es-toolkit to publish a row.
2. Grow an always-on **behavioral** corpus tier alongside `compile_corpus`:
   same harness, but `cargo run` the emitted crate and assert on stdout against
   the expected result of the original TS/Python. The `examples/*/end-to-end/`
   directories are already shaped for exactly this.
3. Track `KNOWN_COMPILE_FAILURES` as a ratchet that can only shrink, the same
   way `smelt-unknown-baseline*.json` is handled today.

## Gap 3 — The sandbox is green in CI without running

> **Status: addressed.** All three proposals below have landed, along with two
> defects the work uncovered. See "Gap 3 follow-up" at the end of this report.


`smelt-specialize` has the worst branch coverage in the workspace (36.1%), and
its two most security-relevant modules are the least covered:

| File | Lines | Line % |
|---|---:|---:|
| `src/node.rs` — batched Node guest orchestration | 536 | **11.4%** |
| `src/sandbox.rs` — fail-closed hard-sandbox backends, bounded guest execution | 657 | **30.3%** |

The cause is not missing tests. `node.rs` contains **473 lines of inline
`#[cfg(test)]` code**. Those tests guard themselves:

```rust
if backend.availability() != crate::BackendAvailability::Available {
    return Ok(());
}
```

They return `Ok(())` — a **pass** — when the backend is missing. And **no
workflow installs bubblewrap or Node**: grepping `.github/workflows/` for
`bwrap`, `bubblewrap`, or `setup-node` returns nothing (the only `apt-get` is
for the GUI's X libraries in `publish-crates.yml`).

The result: every sandboxed-execution test silently no-ops in CI while reporting
success. This is the fail-closed security boundary — policy records, bounded
guest processes, timeout and output limits — and CI has never exercised it.

**Proposal.**

1. Install `bubblewrap` and Node in the coverage and corpus jobs.
2. Make the skip loud and fail-closed in CI: keep the local skip for developer
   machines, but honour a `SMELT_REQUIRE_SANDBOX=1` env var that turns an
   unavailable backend into a test failure, and set it in CI. A silently
   skipping security test is worse than an absent one, because it reports green.
3. Add direct unit tests for the policy/limit logic that do **not** need a live
   backend — timeout enforcement, output truncation, manifest validation,
   `SandboxError::Unavailable` propagation — so a meaningful floor exists even
   where the backend is genuinely absent.

## Gap 4 — HIR/MIR dump formatters are near-untested

| File | Lines | Line % |
|---|---:|---:|
| `smelt-hir/src/format/call.rs` | 706 | **1.8%** |
| `smelt-hir/src/format/control_flow.rs` | 98 | 11.2% |
| `smelt-hir/src/format/types.rs` | 225 | 23.6% |
| `smelt-mir/src/format.rs` | 1,104 | 27.7% |

Together that is roughly 1,700 uncovered lines — the single largest concentrated
block in the workspace.

The cause: `format_compact` is reached through `smelt dump-hir` / `dump-mir` and
the `--hir` flag, and the CLI tests drive it against
`examples/typescript/hir/` — **10 tiny files** (`01_number.ts` through
`10_async_class_method.ts`). Meanwhile `ExprKind` has **178 variants**.
`expr_text` is one large match over all of them, so a handful of literal and
function cases light up ~2% of it.

This is diagnostic surface, so it does not corrupt generated code — but it is
exactly the surface that `skills/smelt-debug-workflow` and every human or agent
debugging a lowering issue depends on. A `todo!()`, a panic, or a
silently-wrong rendering in a rare arm is currently invisible.

**Proposal.** This is the cheapest large win available.

1. Run `format_compact` over the corpus that already exists — the 30
   `examples/typescript/end-to-end/` and 8 `examples/python/end-to-end/` cases —
   and snapshot the output. That is a small harness against existing fixtures.
2. Add an exhaustiveness guard so a new `ExprKind` variant cannot be added
   without a formatter arm (a non-wildcard match in the formatter, or a test
   that constructs every variant).
3. Check whether `smelt-mir/src/opt/mod.rs` (689 lines, 31.8%) is under-tested
   or partly dead — its sibling `opt/move_on_last_use.rs` is at 94.0%, which
   suggests the optimizer *driver* is what lacks direct tests.

## Gap 5 — Python is a second-class citizen

| Signal | TypeScript | Python |
|---|---:|---:|
| Test functions | 942 | 189 |
| Source lines | 87,811 | 20,446 |
| Line coverage | 75.8% | 74.5% |
| `end-to-end` examples | 30 | 8 |
| Libraries in `compat.yml` | 2 (remeda, es-toolkit) | **0** |

Line coverage looks comparable, but the surrounding infrastructure is not. The
Python half of `compile_corpus` is gated behind `#[cfg(feature = "ty")]`, the
example corpus is a quarter the size, and **no Python library is executed
end-to-end anywhere** — there is no Python equivalent of the remeda signal.

`Test-TODO.md` already names the intended targets (`Textualize/rich`,
`encode/httpx`); neither is wired into CI.

**Proposal.** Add one Python library to the `compat.yml` matrix, even a small
one, so the Python path has a real-world execution signal at all. Bring the
`examples/python/end-to-end/` corpus closer to parity with TypeScript's 30 —
these fixtures are cheap and they feed Gap 2 and Gap 4 simultaneously.

## Gap 6 — CLI and reporting tooling

| File | Lines | Line % |
|---|---:|---:|
| `smelt-transpiler/src/specialization.rs` | 445 | 22.7% |
| `smelt-transpiler/src/main.rs` | 158 | 26.6% |
| `smelt-transpiler/src/test_report.rs` | 206 | 28.2% |
| `smelt-transpiler/src/probe.rs` | 388 | 44.6% |
| `smelt-transpiler/src/python.rs` | 132 | 47.7% |

These are the tooling subcommands — `probe`, `rust-diagnostics`,
`rust-test-report`, `smelt-unknown-report` — that `AGENTS.md`/`CLAUDE.md`
instruct agents to rely on for every blocker investigation. The documented
workflow depends on their output format, and `probe_diagnostics_tests.rs` is
67 lines.

If `rust-test-report`'s grouping or Markdown shape breaks, the debug workflow
degrades silently and no test notices.

**Proposal.** Golden-file the report renderers: feed a recorded `cargo
check`/`cargo test` output fixture through `rust-diagnostics` and
`rust-test-report` and snapshot the Markdown. This is pure string-in/string-out
and needs no toolchain, so it is fast and stable.

(Note: `cli_parser.rs` is absent from the coverage report because it is pure
`clap` derive definitions with no executable lines — that one is not a real
gap.)

## Gap 7 — Negative testing is real but shares the weak oracle

Worth stating precisely, because the raw counts mislead. There are **116**
`reject*`/`invalid*`/`fails*`-named tests, and they do assert on diagnostics —
via a `lower_errors` + `first_error` helper idiom rather than `is_err()`
directly (hence only 4 uses of `is_err`/`unwrap_err`/`should_panic`).

So error paths are tested. But they are tested with
`error.message.contains("argument must match")` — the same substring oracle as
Gap 1, and it is more fragile here, since diagnostic wording changes routinely.
Asserting on a stable diagnostic *code* rather than message prose would make
these both stronger and less brittle.

---

## Recommended priority

Ranked by (risk × cost-to-fix):

1. ~~**Make the sandbox tests actually run in CI** (Gap 3).~~ **Done** — see
   the follow-up section below.
2. **Snapshot the HIR/MIR formatters over the existing example corpus**
   (Gap 4). Largest concentrated coverage block, ~1,700 lines, and the fixtures
   already exist.
3. **Put `compat.yml` on a schedule and land an es-toolkit row** (Gap 2). The
   project's best behavioral signal currently only fires when someone remembers.
4. **Golden-file the report renderers** (Gap 6). Cheap, and it protects the
   documented agent workflow.
5. **Adopt snapshot-by-default for new emitter tests** (Gap 1). A policy change
   that compounds, with no big-bang rewrite.
6. **Give Python one real compat target** (Gap 5).

---

## Gap 3 follow-up — what landed, and two defects it exposed

All three proposals were implemented. Doing so surfaced two real bugs that the
silent skip had been hiding.

### What changed

- **`smelt_specialize::prereq`** (new module). One place that decides
  skip-versus-fail. `missing_prerequisite(test, requirement)` prints a `SKIP`
  notice naming both, and returns an error instead when `SMELT_REQUIRE_SANDBOX`
  is set. Its return type is generic over `E: From<String>`, so a single call
  site serves tests returning `Result<(), String>` and
  `Result<(), Box<dyn Error>>` alike. The environment is read in exactly one
  function and every decision below it is pure, so the behavior is unit-tested
  without mutating process environment — which `unsafe_code = "deny"` forbids
  anyway and which is racy under the threaded runner.
- **All 22 silent `return Ok(())` skips replaced** across `node.rs`,
  `python.rs`, `typescript_specialization_parity.rs`, and
  `python_specialization_parity.rs`.
- **A `sandbox` CI job**, separate from `coverage` on purpose: it installs
  bubblewrap and `typescript`, asserts `bwrap` can actually start a guest, then
  runs the specialization tests with `SMELT_REQUIRE_SANDBOX=1`. It is red
  exactly when the boundary is not being exercised. The `coverage` job installs
  the same prerequisites but does *not* set the strict flag, so a runner-side
  sandbox quirk cannot blank the coverage badge.
- **Seven backend-free tests** added to `sandbox.rs`, covering wall-time
  enforcement, the output budget, environment replacement, and the fail-closed
  ordering inside `SandboxRunner::run` (verifying that a mismatched policy and
  an unavailable backend are both refused *before* the command is prepared, via
  a `#[cfg(test)]` backend double that records whether `prepare` was reached).
  These need no `bwrap` and no guest runtime, so they are a floor that stays
  meaningful on any POSIX host.

### Defect 1 — executable discovery could never succeed on CI

The per-test helpers probed hardcoded absolute paths only:

```rust
["/usr/bin/node", "/usr/local/bin/node"].iter().map(Path::new).find(|p| p.is_file())
```

GitHub's `setup-node` installs into the hosted tool cache, so this check would
have failed even after adding Node to CI — installing the prerequisite alone
would not have un-skipped a single test. It was also skipping on the development
container used for this analysis, where Node and `tsc` are present at
`/opt/node22/bin`. `prereq::locate_executable` searches `PATH` first and keeps
the absolute paths as fallbacks.

### Defect 2 — the wall-time limit was not actually bounded

Found by the new `wall_time_limit_bounds_a_guest_that_orphans_a_descendant`
test, which took **30 seconds to enforce a 250 ms deadline**.

`run_prepared` killed the child at the deadline and then joined the output
reader threads. But killing a launcher does not reap descendants it forked, and
an orphan inherits the pipe write ends — so the join blocked until the orphan
exited on its own. `/bin/sh -c 'sleep 30'` forks rather than execs, which
reproduces it exactly: the deadline fires on time, then the call sits for the
orphan's full 30 seconds. A guest that ignores its deadline could stall a build
for as long as it liked.

The fix returns the bound violation *before* joining the readers, for both the
wall-time and output-budget paths. The reader threads are detached and exit once
the descriptors close; their partial output is discarded on those paths anyway.
In production the Linux backend usually contains this with
`--unshare-all --die-with-parent` (the guest tree dies with its PID namespace),
but the limit now holds even where the backend does not provide that — which
matters for the macOS `sandbox-exec` backend, where no PID namespace exists.

The `smelt-specialize` suite went from 30.38 s to 0.41 s as a side effect.

### Caveat

Whether `bwrap` runs on a GitHub-hosted runner is unverified from here. Ubuntu
24.04 restricts unprivileged user namespaces through AppArmor; the job sets
`kernel.apparmor_restrict_unprivileged_userns=0` (tolerating absence of the key
on older images). The explicit "verify the hard sandbox can start a guest" step
exists so that if this is wrong, the failure is one readable line in a
purpose-named job rather than an inscrutable Rust test error. **The first CI run
on this branch is the real test of that assumption.**

---

Three small cleanups worth folding in:

- (Resolved: `smelt-py-ty-spike` was removed once `smelt-py-types` superseded
  it.) It was at 0.0% across 123 lines and 12 functions — if it is a
  finished spike, delete it rather than carry it in the denominator.
- `KNOWN_COMPILE_FAILURES`' `async_await` entry is a known real bug still
  awaiting a fix.
- **Unreachable reference:** `compile_corpus.rs` states three times that every
  `KNOWN_COMPILE_FAILURES` entry MUST reference an entry in
  `blocker-logs/compile-snapshots-findings.md`. That file is not in the
  repository — `.gitignore` ignores `blocker-logs/*.md` by default and has no
  allowlist entry for it, so it was never committed. The rationale for the one
  current exclusion is unreadable from a fresh clone. Either allowlist the log
  or move the explanation inline into the constant's comment.
