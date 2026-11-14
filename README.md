# SMELT

> Smelt your TypeScript and Python into Rust.

[![crates.io](https://img.shields.io/crates/v/smelt-transpiler.svg)](https://crates.io/crates/smelt-transpiler)
[![license](https://img.shields.io/crates/l/smelt-transpiler.svg)](LICENSE)

Smelt is a transpiler that compiles strictly-typed TypeScript and Python into a
Rust crate. It does not try to accept all TypeScript or Python: it accepts the
statically-typed subset that passes the language's own type checker, lowers it
through a shared intermediate representation, and emits Rust that compiles and
behaves like the source. Source-language tests (Vitest, pytest) are lowered too
and run as native `#[test]` functions, which is how behavior is checked.

```sh
cargo install --locked smelt-transpiler
smelt --help
```

The crates.io release lags `main`: `smelt-transpiler` and `smelt-ts-types`
cannot be published while the TypeScript 7 checker they embed (tsrs) is a git
dependency (`KNOWN_BLOCKED` in [`scripts/publish-crates.sh`](scripts/publish-crates.sh)).
For the current code, install from a checkout with
`cargo install --locked --path crates/smelt-transpiler`. The default features
include the TypeScript frontend, the Python frontend with ty-backed type
resolution, and the TypeScript 7 checker; `--no-default-features` builds a
TypeScript-only CLI.

## North star

Output is judged against one question: what would a team of engineers produce
if they rewrote this codebase in Rust by hand? Such a team gives each value the
most precise type the source supports and carries it all the way to runtime.
They reach for a concrete struct or enum first, then a generic `T` with trait
bounds when the code is really polymorphic, then `dyn Trait` for dynamic
dispatch, and only at a real dynamic boundary (source `unknown`, JSON, erased
interop) for a tagged runtime value. In Smelt that runtime value is
`SmeltUnknown`, and every use of it is a cost to justify, not a way to make
generated code type-check. The same team would refuse a rule that only fires
for one library's spelling, so Smelt has no library-specific special cases:
everything lowers through general rules. The full statement is in
[`AGENTS.md`](AGENTS.md) ("Project scope", "SmeltUnknown boundaries").

Because "it compiles" says nothing about either property, the project measures
both behavior (generated test suites of real libraries, run in CI) and type
preservation (the typedness and SmeltUnknown reports below).

## Status

Pre-alpha. The main line of work is conformance on real TypeScript utility
libraries and a web framework (Hono), with type preservation measured on every
change. Every number below names its source file; dates are the last commit
that changed that file.

### Generated test suites

| Library | Result | Gate | Source |
| --- | --- | --- | --- |
| remeda | 1789 passed, 0 failed | blocking on `main` pushes | `.github/workflows/ci.yml` job `remeda-regression` |
| es-toolkit | at least 1048 passed; 11 named tests allowed to fail | blocking ratchet | [`blocker-logs/es-toolkit-ci-baseline.json`](blocker-logs/es-toolkit-ci-baseline.json) (2026-11-07) |
| hono | at least 444 passed; 10 named tests allowed to fail; test target builds with 0 rustc errors | blocking ratchet | [`blocker-logs/hono-ci-baseline.json`](blocker-logs/hono-ci-baseline.json) (2026-11-08) |
| radash | exactly 381 passed, 6 failed (pinned) | blocking pin | `.github/workflows/ci.yml` job `radash-regression` |
| ts-pattern | exactly 419 passed, 19 failed (pinned) | blocking pin | `.github/workflows/ci.yml` job `ts-pattern-build` |
| date-fns | exactly 287 passed, 43 failed (pinned) | blocking pin | `.github/workflows/ci.yml` job `date-fns-build` |

Each library is checked out at the commit pinned in
[`.github/compat/libraries.json`](.github/compat/libraries.json). These jobs
always run on pushes to `main`; on a pull request they run only with the
`run-regressions` label. For es-toolkit, CI also requires zero files with probe
blockers and zero `cargo check` / `cargo test --no-run` errors in the generated
crate. The hono failures pinned in the baseline are `mount` routing,
pretty-json's indent argument and RegExpRouter parameter capture; the radash
failures are type-violating test inputs (`as any`, `null as unknown as []`) and
`Proxy` traps, see [`blocker-logs/globals-suite-failures.md`](blocker-logs/globals-suite-failures.md).

[`COMPAT.md`](COMPAT.md) holds the last run of the manual Compatibility
workflow: remeda at `3c80f28bb394`, 1789 passed, 0 failed, dated
2026-07-10. It is a snapshot; the CI pins above are the current contract.

The daily Library Probes workflow writes
[`blocker-logs/library-probes.md`](blocker-logs/library-probes.md). Its
2026-11-13 run transpiles remeda, es-toolkit, hono, radash, ts-pattern and
date-fns; valibot, neverthrow, immer and rxjs still stop on lowering or stdlib
blockers.

### Type preservation

`smelt typedness-report` counts, on optimized MIR and through the Rust
emitter's own type rendering, how many values (locals), property reads and
calls get a concrete Rust type, a real Rust generic, or an erased type
(`SmeltUnknown` or `SmeltErasedFunction` anywhere in the type). Methodology:
[`blocker-logs/typedness-report.md`](blocker-logs/typedness-report.md).

| Corpus (baseline file, date) | Kind | Concrete | Generic | Erased | Erased share |
| --- | --- | ---: | ---: | ---: | ---: |
| examples, 168 programs ([`typedness-baseline.json`](blocker-logs/typedness-baseline.json), 2026-11-14) | values | 8292 | 139 | 56 | 0.7% |
| | reads | 858 | 39 | 20 | 2.2% |
| | calls | 2273 | 14 | 2 | 0.1% |
| es-toolkit, 744 programs ([`typedness-baseline-es-toolkit.json`](blocker-logs/typedness-baseline-es-toolkit.json), 2026-11-14) | values | 13829 | 228 | 7803 | 35.7% |
| | reads | 341 | 33 | 1028 | 73.3% |
| | calls | 2141 | 39 | 1258 | 36.6% |
| remeda, 390 programs ([`typedness-baseline-remeda.json`](blocker-logs/typedness-baseline-remeda.json), 2026-11-05) | values | 12960 | 204 | 10147 | 43.5% |
| | reads | 365 | 22 | 842 | 68.5% |
| | calls | 1576 | 47 | 2838 | 63.6% |

The examples corpus is almost fully typed. The libraries are not: on es-toolkit
more than a third of values and calls are erased, and on remeda erased calls
outnumber concrete ones. The generated suites pass, but much of the code that
makes them pass runs through `SmeltUnknown`. Closing this gap is the main open
problem. CI blocks any rise in an erased count on the examples and es-toolkit;
the remeda report is advisory.

`smelt smelt-unknown-report` classifies every textual `SmeltUnknown` in the
generated Rust as runtime prelude (shared scaffolding emitted into every
crate), legitimate boundary (erased closures, promises, explicit adapters,
narrowing guards) or avoidable erasure (everything else). Methodology:
[`blocker-logs/smelt-unknown-report.md`](blocker-logs/smelt-unknown-report.md).

| Corpus (baseline file, date) | Files | Runtime prelude | Legitimate boundary | Avoidable erasure | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| examples ([`smelt-unknown-baseline.json`](blocker-logs/smelt-unknown-baseline.json), 2026-11-09) | 168 | 132775 | 6951 | 0 | blocking, must stay 0 |
| es-toolkit ([`smelt-unknown-baseline-es-toolkit.json`](blocker-logs/smelt-unknown-baseline-es-toolkit.json), 2026-11-14) | 744 | 2164 | 33230 | 15071 | blocking, may only fall |
| remeda ([`smelt-unknown-baseline-remeda.json`](blocker-logs/smelt-unknown-baseline-remeda.json), 2026-11-08) | 391 | 1506 | 62265 | 20086 | advisory |
| hono ([`smelt-unknown-baseline-hono.json`](blocker-logs/smelt-unknown-baseline-hono.json), 2026-11-08) | 78 | 7445 | 78054 | 10078 | advisory |

Every erasure site in the frontends names an `ErasureReason`
([`docs/erasure-reasons.md`](docs/erasure-reasons.md)); `smelt build` writes
them to `<output>/smelt-erasure.json` so the report can explain avoidable
erasure by reason and source line.

### Ownership

There is no general ownership or borrow inference. Lowering reads values by
copy and codegen clones by default; the MIR pass `move_on_last_use` turns a
copy into a move where liveness proves the value dead afterwards, some emitter
paths pass strings and receivers by reference, and class instances with
reference semantics are emitted as `Rc<RefCell<_>>` handles. Borrow inference
is listed as v2.0 work in [`specs/architecture.md`](specs/architecture.md).

### Python

The Python frontend (Ruff parser, types resolved by Astral's ty) has 11
golden end-to-end programs under `examples/python/end-to-end`, against 168 for
TypeScript.
No third-party Python library is gated in CI. In the 2026-11-13 library-probes
run, none of the six probed Python libraries (returns, result, more-itertools,
funcy, toolz, cachetools) transpiles. Definition-time metaprogramming
(metaclasses, decorators, dataclasses) is handled by build-time specialization,
described in [`docs/metaprogramming.md`](docs/metaprogramming.md). Python and
TypeScript modules can share one crate, but only for thin cases (a free
function with primitive types); see
[`blocker-logs/hono-crud-python-app.md`](blocker-logs/hono-crud-python-app.md)
(2026-10-17). Python is not measured by the typedness or SmeltUnknown
baselines.

### v1.0 goals

The milestone specs still define v1.0 as: a strict-mode Express app compiling
to a working `axum` server ([`specs/m7-express-to-axum.md`](specs/m7-express-to-axum.md)),
a fully typed FastAPI + Pydantic app doing the same
([`specs/m9-fastapi-pydantic.md`](specs/m9-fastapi-pydantic.md)), structurally
similar Rust from both, and a usable CLI and `Smelt.toml`
([`specs/m10-v1-release.md`](specs/m10-v1-release.md)). None of these is met.
As of [`blocker-logs/express-v1-baseline.md`](blocker-logs/express-v1-baseline.md)
(2026-09-06), an Express app transpiles to a crate that compiles but does
nothing, because `express` is an unresolved package; there is no `axum`
mapping in the code and no FastAPI app in the repository.

The server work goes through Hono instead, transpiled as source rather than
mapped. [`examples/typescript/hono_crud`](examples/typescript/hono_crud) (Hono,
`node:sqlite`, `@hono/node-server`) builds a server on the runtime's
`node:http` adapter; per
[`blocker-logs/hono-crud-status.md`](blocker-logs/hono-crud-status.md)
(2026-10-16 measurements) it answers 29 of 30 probe requests identically to
Node.

## What Smelt accepts and rejects

Input must pass its own type checker: `smelt build` refuses a program with
TypeScript or ty errors unless `--allow-type-errors` is given. Constructs with
no static meaning are rejected with a source-located error rather than lowered
to something approximate: `eval` and `new Function`, `with`, and
runtime-shaped metaprogramming. `any` is accepted: the value is erased to the
tagged runtime value and counted as an avoidable `explicit-any` erasure
([`docs/erasure-reasons.md`](docs/erasure-reasons.md)). Definition-time
metaprogramming that produces a stable shape is specialized at build time in a
sandbox and never runs in the generated program. Detail lives in:

- [`specs/frontend-ts.md`](specs/frontend-ts.md): TypeScript rules and the check pipeline.
- [`docs/metaprogramming.md`](docs/metaprogramming.md): what specializes, what fails loud, what is rejected.
- [`docs/host-runtime-specialization.md`](docs/host-runtime-specialization.md): the sandboxed build-time evaluator.
- [`specs/stdlib-mapping.md`](specs/stdlib-mapping.md): how standard-library calls map to Rust.

## Architecture at a glance

```
 TypeScript ── oxc parse ── smelt-frontend-ts ──┐   types: smelt-ts-types (tsrs, TS 7 checker)
                                                │          -> smelt-ts-model (plain-data facts)
 Python ───── ruff parse ── smelt-frontend-py ──┤   types: smelt-py-types (Astral ty)
                                                │
          smelt-specialize (build-time sandboxed partial evaluation of
          definition-time metaprogramming, fed to the frontends as a manifest)
                                                │
                                                v
                                   smelt-hir (shared HIR, validation)
                                                │
                                                v
                     smelt-mir (CFG over locals, optimization passes)
                                                │
                                                v
                           smelt-codegen-rust (MIR -> Rust source)
                                                │
                                                v
                Rust crate: src/ + Cargo.toml, plus the runtime text from
                smelt-runtime inlined into it

 smelt-stdlib: stdlib mapping rules shared by both frontends
 smelt-transpiler: the `smelt` CLI, manifest handling, reports
```

| Crate | Role |
| --- | --- |
| `smelt-frontend-ts` | Lowers strict TypeScript to HIR, parsing with oxc. |
| `smelt-ts-types` | Resolves TypeScript 7 type facts with the embedded tsrs checker (git dependency). |
| `smelt-ts-model` | Dependency-free data model of those facts, so the frontend stays publishable. |
| `smelt-frontend-py` | Lowers Python to HIR, parsing with Ruff. |
| `smelt-py-types` | Resolves Python types with Astral's ty, since idiomatic Python omits most annotations. |
| `smelt-specialize` | Build-time host-runtime specialization: runs definition-time code once in a sandbox and records the resulting shape. |
| `smelt-hir` | Shared high-level IR types and validation. |
| `smelt-mir` | Mid-level IR, HIR-to-MIR lowering, validation and optimization passes. |
| `smelt-codegen-rust` | Emits Rust source from MIR. |
| `smelt-runtime` | Source of truth for the runtime code inlined into every generated crate. |
| `smelt-stdlib` | Standard-library mapping metadata and diagnostic categories shared across frontends. |
| `smelt-transpiler` | The `smelt` binary: manifest handling, lowering orchestration, reports. |
| `smelt-gui` | GPUI desktop playground: edit TypeScript or Python and watch the Rust output. |

[`specs/architecture.md`](specs/architecture.md) is the original design
document; [`CONTEXT.md`](CONTEXT.md) defines the vocabulary the code uses.

## Usage

A project is described by `Smelt.toml` ([`specs/config.md`](specs/config.md);
JSON Schema in [`smelt-schema.json`](smelt-schema.json)). Every command takes
`--manifest-path <PATH>` (default `./Smelt.toml`).

| Command | What it does |
| --- | --- |
| `smelt new <name> [--python]` | Scaffold a project. |
| `smelt build` | Transpile and emit the Rust crate; also writes `smelt-typedness.json` and `smelt-erasure.json` next to it. `--allow-type-errors`, `--erasure <reason>=<allow\|warn\|deny>`, `--define KEY=VALUE`, `--hir`. |
| `smelt check` | Type-check and validate without emitting Rust. `--message-format json` collects every module's categorized diagnostics. |
| `smelt probe` | Report how far a project transpiles and group the remaining blockers by category (`--format md\|json`, `--run-tests`). |
| `smelt rust-diagnostics --cargo-manifest <Cargo.toml>` | Run `cargo check` on a generated crate and write diagnostics grouped by rustc code, largest group first. |
| `smelt rust-test-report` | Run generated tests (`--focus`, `--guard`, `--full`) and write a compact Markdown investigation report. |
| `smelt typedness-report <path>` | Measure concrete/generic/erased counts; `--baseline`, `--fail-on-regression`, `--update-baseline`. |
| `smelt smelt-unknown-report <src>` | Classify `SmeltUnknown` in generated Rust; same baseline flags. |
| `smelt ts7-parity-report <path>...` | Compare Smelt's TypeScript types with the TypeScript 7 checker's. Method: [`blocker-logs/ts7-parity.md`](blocker-logs/ts7-parity.md). |
| `smelt list-escape-report <Smelt.toml>` | Classify list-typed MIR locals as escaping, aliased or frame-local. |
| `smelt dump-ast` / `dump-hir` / `dump-mir <file>` | Print an intermediate form of one file. |
| `smelt dump-schema` | Print the JSON Schema for `Smelt.toml`. |
| `smelt clean` | Remove the output directory. |

`smelt <command> --help` lists every flag. Command design notes are in
[`specs/cli.md`](specs/cli.md).

## Development

Builds use [corgi](https://corgi.build/) instead of `cargo`; it keeps one
shared build cache per machine instead of a `target/` per worktree. Install it
with `cargo install corgi-build`. The tight loop:

```sh
corgi check --workspace --lib
corgi clippy --workspace --lib
# TypeScript-only work: add --no-default-features to skip the Python stack
```

Before a commit, run `cargo fmt --all`, `cargo clippy --workspace --all-targets`
and `cargo test` (corgi cannot run the last two yet). CI's `ci-gates` job
checks formatting with `cargo fmt --all -- --check`, and fails on any rise in the per-crate clippy
warning counts in `blocker-logs/clippy-baseline.json`. Everything else (the
baselines and how to update them, the generated-test debugging workflow in
[`skills/smelt-debug-workflow/SKILL.md`](skills/smelt-debug-workflow/SKILL.md),
rules for subagents) is in [`AGENTS.md`](AGENTS.md).

Repository layout outside `crates/`:

| Path | Contents |
| --- | --- |
| `examples/` | End-to-end programs with golden HIR/Rust (`typescript/end-to-end`, `python/end-to-end`) and sample projects such as `hono_crud`. |
| `specs/` | Design documents and milestone specs. |
| `docs/` | Current design notes (metaprogramming, erasure reasons, specialization). |
| `blocker-logs/` | CI baselines, report methodologies and current-status reports, see [`blocker-logs/README.md`](blocker-logs/README.md). Investigation reports are temporary. |
| `benchmarks/` | Hand-written TypeScript vs generated Rust, see [`benchmarks/README.md`](benchmarks/README.md). |
| `scripts/` | CI gate scripts, publishing, regeneration helpers. |
| `skills/` | Agent skills (debug workflow, dev log). |
| `site/` | Templates and assets for the documentation site the Pages workflow builds. |
| `devlog/` | Drafts and state for the dev-log routine. |
| `third_party/` | Library checkouts used for local conformance work. |

## Documentation

- [`docs/why-smelt.md`](docs/why-smelt.md): why the project exists.
- [`docs/architecture/index.html`](docs/architecture/index.html): visual architecture map (pipeline, crates, modules, erasure points, MIR passes, CI gates, churn).
- [`specs/architecture.md`](specs/architecture.md) and [`CONTEXT.md`](CONTEXT.md): pipeline design and vocabulary.
- [`specs/hir.md`](specs/hir.md), [`specs/mir.md`](specs/mir.md): the two IRs.
- [`specs/frontend-ts.md`](specs/frontend-ts.md), [`specs/frontend-py.md`](specs/frontend-py.md), [`specs/python-type-checking-with-ty.md`](specs/python-type-checking-with-ty.md): frontends.
- [`specs/testing-strategy.md`](specs/testing-strategy.md): how generated Rust is checked against source behavior.
- [`docs/erasure-reasons.md`](docs/erasure-reasons.md), [`blocker-logs/typedness-report.md`](blocker-logs/typedness-report.md), [`blocker-logs/smelt-unknown-report.md`](blocker-logs/smelt-unknown-report.md): how type preservation is measured.
- [`docs/metaprogramming.md`](docs/metaprogramming.md), [`docs/host-runtime-specialization.md`](docs/host-runtime-specialization.md): build-time specialization.
- [`specs/config.md`](specs/config.md), [`specs/cli.md`](specs/cli.md): `Smelt.toml` and the CLI.
- [`COMPAT.md`](COMPAT.md), [`blocker-logs/library-probes.md`](blocker-logs/library-probes.md): third-party library results.

## Publishing

Publishing to crates.io is handled by [`scripts/publish-crates.sh`](scripts/publish-crates.sh)
(`--execute` to publish for real; a bare run does a dry run). It publishes the
crates leaf-first in dependency order and refuses to run if the set of crates
that cannot be published changes. Maintainers can also run the manual
**Publish crates.io release** workflow after committing the new workspace
version. The `crates-io` GitHub environment must provide a `CRATES_IO_TOKEN`
secret; the dispatch requires the matching version and the confirmation text
`publish`.

## License

[GNU General Public License v3.0](LICENSE)

## AI use

This project leans heavily on AI, and that includes the code. A large portion
of the source is AI-generated, as are commit messages, issues and
documentation. AI-generated PRs are welcome as long as they stay reasonably
sized and make sense. Human review, the test suite and the baselines above are
the quality gate: anything that lands has to build, pass `cargo test` and
`clippy`, not regress a baseline, and be understandable.

## Contributing

Fork the repository and open a pull request. If it touches a frontend, HIR,
MIR, the specializer or the Rust emitter, add the `run-regressions` label so
the library gates run on it.

<!-- COVERAGE:START -->
## Coverage

![Coverage](coverage.svg)

### Workspace

| Metric | Coverage |
| --- | ---: |
| Functions | 78.93% |
| Lines | 81.56% |
| Regions | 74.51% |
| Branches | 69.44% |

### Per Crate

| Crate | Functions | Lines | Branches |
| --- | ---: | ---: | ---: |
| `smelt-codegen-rust` | 90.95% | 85.02% | 71.85% |
| `smelt-frontend-py` | 87.04% | 79.92% | 71.78% |
| `smelt-frontend-ts` | 90.17% | 83.20% | 72.29% |
| `smelt-hir` | 84.69% | 80.21% | 72.19% |
| `smelt-mir` | 92.27% | 91.32% | 76.86% |
| `smelt-py-types` | 84.62% | 92.86% | 68.97% |
| `smelt-runtime` | 27.45% | 31.86% | 29.31% |
| `smelt-specialize` | 70.02% | 78.90% | 62.31% |
| `smelt-stdlib` | 88.11% | 85.54% | 92.07% |
| `smelt-transpiler` | 82.79% | 81.84% | 70.86% |
| `smelt-ts-model` | 91.67% | 93.27% | 50.00% |
| `smelt-ts-types` | 85.94% | 90.58% | 84.72% |
<!-- COVERAGE:END -->

