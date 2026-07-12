# CI Plan: es-toolkit as a Regression Gate

*(Fable planning pass, 2026-07-12. Verified against ci.yml, compat.yml, library-probes.yml, .github/compat/, cli_parser.rs.)*

## Verified facts (file:line)

- **`ci.yml:158-203` `remeda-regression`** is the exact template: build smelt from source (`cargo build --bin smelt`, ci.yml:173), clone remeda at pinned ref `3c80f28b` with `--filter=blob:none` (ci.yml:179-180), overlay fixtures from `.github/compat/remeda/` (ci.yml:181), transpile, `cargo test --manifest-path .../dist-smelt/Cargo.toml --no-fail-fast` (ci.yml:187), then advisory `smelt smelt-unknown-report` (ci.yml:194-203). Cache: `Swatinem/rust-cache@v2` with `shared-key: build` (ci.yml:167-170).
- **"update coverage report"** commits come from `ci.yml:87-98` (`coverage` job); **"update library probe report"** from `library-probes.yml:78-82` (daily cron 06:17 UTC, non-failing by design, driven by `scripts/probe_libraries.py` + `.github/compat/libraries.json`).
- **`compat.yml`** is manual-dispatch only (compat.yml:7-16), matrix currently remeda-only (compat.yml:24-30), already invokes `smelt rust-test-report --full --diagnostics --suppress-warnings` (compat.yml:81-87).
- **Source provisioning**: `third_party/es-toolkit` is a **gitlink at `e008a2818c` with NO `.gitmodules` entry** — a `submodules: true` checkout will NOT fetch it. The same ref is pinned in `.github/compat/libraries.json`; fixtures (`Smelt.toml` with test-prefix globs and DOM excludes) exist at `.github/compat/es-toolkit/`. CI must clone by ref + overlay fixtures, like remeda. (Housekeeping: add es-toolkit to `.gitmodules` or drop the dangling gitlink.)
- **Tooling**: `smelt probe --format json --output <path>` (cli_parser.rs:147-160); `smelt rust-test-report` (cli_parser.rs:102-138) has `--full`, `--diagnostics`, `--baseline-report` but **no per-test timeout flag** — hang-safety must come from outside; `smelt rust-diagnostics` groups by family; `smelt-unknown-report --baseline` is the established advisory-baseline pattern.
- **Current state**: library `cargo check` = 0 errors / ~950 warnings; `cargo test --no-run` = 0 errors; runtime ~56 pass / 89 fail with async hangs (attemptAsync/combinations); 9 residual probe blockers, all `compat/**`.

## 1. Gate levels (drawn at today's high-water marks)

New job `es-toolkit-regression` in `ci.yml` (push + PR to main), four sequential gates:

- **G1 — probe blockers <= 9.** `smelt probe --format json --output probe.json`; compare files-with-blockers to `baseline.probe_blockers` (9). Fail if greater (`<=`, so improvements pass; ratchet auto-tightens on main, section 3).
- **G2 — library compiles: `cargo check` errors == 0** on `dist-smelt/Cargo.toml`. Hard zero.
- **G3 — tests compile: `cargo test --no-run` errors == 0.** Hard zero. `RUSTFLAGS=-Awarnings` so ~950 warnings don't drown output or gate yet. Optional advisory: warning count vs `baseline.warning_count`.
- **G4 — runtime pass-count ratchet (initially advisory, blocking once hangs are fixed).** Hang-safe strategy:
  - Wrap the run in a hard wall clock: `timeout --signal=KILL 20m cargo test ... -- --test-threads=4`. Exit 124/137 = "hang regression" — fail naming the last binaries that printed output.
  - Recommended when adopted: **cargo-nextest** (`cargo nextest run --no-fail-fast`, `slow-timeout = { period = "60s", terminate-after = 3 }` in a checked-in `.config/nextest.toml` overlay in `.github/compat/es-toolkit/`) — per-test timeouts convert hangs into countable failures + JUnit output. Nextest is the only robust answer to deadlocking tests; plain libtest has no per-test timeout.
  - Gate: `passed >= baseline.runtime_passed` (start 56); once green also `failed <= baseline.runtime_failed`. Never exact equality — flaky async ordering.

## 2. Integration with existing workflows

- **Job layout**: one new job in `ci.yml`, parallel sibling of `corpus`/`coverage`/`remeda-regression`. NOT `library-probes.yml` (never-failing daily cron — wrong place) or `compat.yml` (manual). DO add es-toolkit to `compat.yml`'s matrix for rich COMPAT.md reports — reporting, not gating.
- **Caching**: two lanes.
  - Smelt build: reuse `shared-key: build` (shares the debug smelt build across jobs).
  - Generated crate target dir: the mtime-preserving emitter makes incremental reuse work IF generated sources and `target/` survive between runs. Use plain `actions/cache` on `target/compat-repos/es-toolkit/dist-smelt/target` keyed on `hashFiles('Cargo.lock', '.github/compat/es-toolkit/**')` + restore-keys fallback to latest main. Clone must be deterministic; restore dist-smelt/target after transpile so the emitter leaves unchanged .rs untouched and Cargo reuses artifacts. Verify fingerprints locally; if they miss, cold `cargo check` is the fallback cost.
  - Expected minutes: smelt build ~2-4 warm; probe ~2-5; generated check+test-build ~5-20 cold / ~5-8 warm; test run capped at 20. Total ~15 min warm / ~35 cold.

## 3. Ratchet mechanics

- **Checked-in baseline**: `blocker-logs/es-toolkit-ci-baseline.json` (mirrors smelt-unknown-baseline.json), e.g.
  `{ "ref": "e008a2818c…", "probe_blockers": 9, "check_errors": 0, "test_build_errors": 0, "runtime_passed": 56, "runtime_failed": 89, "runtime_gate": "advisory", "warning_count": 950 }`
- **Compare script**: `scripts/check_estoolkit_gate.py` reading probe.json, test results, baseline. On failure: run `smelt rust-diagnostics --output gate-diagnostics.md`, print top diagnostic families into `$GITHUB_STEP_SUMMARY`, plus `rust-test-report --baseline-report` naming newly-failing tests. Message format: "es-toolkit gate: cargo check regressed 0 -> 3 errors; largest families: E0308 mismatched types (2, src/compat/...), E0277 (1, ...). See gate-diagnostics.md artifact."
- **Updates**: improvements pass without a bump (`<=`/`>=`); a push-to-main step (same auto-commit pattern as ci.yml:87-98, `stefanzweifel/git-auto-commit-action@v7`) **auto-tightens** when actuals beat baseline ("chore(ci): ratchet es-toolkit baseline"). Loosening is manual-only with PR justification (mirrors the SmeltUnknown-enforcement rule).
- Upload probe.json, test report, gate-diagnostics.md as artifacts always.

## 4. Source provisioning

Clone-by-pinned-ref + fixture overlay (the proven remeda pattern, ci.yml:175-181):
```
git clone --no-tags --filter=blob:none https://github.com/toss/es-toolkit.git target/compat-repos/es-toolkit
git -C target/compat-repos/es-toolkit checkout e008a2818cd8d07469a5cc12ee0c02405d523e07
cp -R .github/compat/es-toolkit/. target/compat-repos/es-toolkit/
```
Read the ref from `.github/compat/libraries.json` (single source of truth). Do not rely on the `third_party/es-toolkit` gitlink (no .gitmodules entry; CI submodule checkout silently skips it).

## 5. Minimal first PR vs full version

**Minimal (PR-able today, ~60 lines of YAML, zero new scripts):**
- `es-toolkit-regression` job cloned from `remeda-regression`: clone+overlay -> `smelt probe` (advisory log) -> `RUSTFLAGS=-Awarnings cargo check` (**gate: exit code**) -> `RUSTFLAGS=-Awarnings timeout 20m cargo test --no-run` (**gate**). No runtime run, no baseline JSON, no target cache. Locks in both compile campaigns (184->0 and 222->0).
- Plus: es-toolkit in `compat.yml` matrix for on-demand reports.

**Full (follow-up PRs):**
1. Baseline JSON + `scripts/check_estoolkit_gate.py` (probe gate <=9, rust-diagnostics failure messaging, step summary).
2. `actions/cache` for dist-smelt/target exploiting the mtime-preserving emitter; measure warm-vs-cold.
3. cargo-nextest with per-test slow-timeout; advisory runtime pass-count vs baseline.
4. Flip runtime gate to blocking + auto-ratchet once the async hangs are fixed and pass count stabilizes.
