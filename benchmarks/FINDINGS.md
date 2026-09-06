# Library benchmark findings — September 6, 2026

Measured compiler: [`fe03b91`](https://github.com/Bombatomica64/smelt/commit/fe03b91aabb2fe21e0946dc40b9e24a8a113df85).
Both native runners were regenerated from a fresh release build. No compiler,
runtime, or benchmark-harness code was changed for this measurement.

[RESULTS.md](RESULTS.md) contains the generated tables;
[results/results.json](results/results.json) contains the measurements and throughput
observations; [results/environment.json](results/environment.json) records the exact
compiler/library commits, tool versions, machine details, and artifact hashes.
The [historical analysis](FINDINGS-2026-08-26.md) is retained separately.

## Current result

All **32 paired cases** have matching TypeScript/Rust checksums. All 35 cases
(including three Rust-only variants) completed three observations per measured side,
with no recorded execution errors. Checksum agreement validates these inputs, not
complete library semantics. The standard timing protocol was used, without `--quick`.

| Library | Geometric-mean slowdown vs Node | Paired cases faster in Rust | Idle Rust / Node RSS |
| --- | ---: | ---: | ---: |
| es-toolkit | 3.7× | 3 / 16 | 6.1 / 53.9 MiB |
| Remeda | 14.7× | 1 / 16 | 6.1 / 53.7 MiB |

The generated report rounds these geometric means to 4× and 15×. Its per-case
ratios and noise columns remain the authoritative detailed view.

Observed wins are es-toolkit `unique` (1.14×), `difference` (1.11×), and
`intersection` (1.16×), plus Remeda `flatten` (1.47×). The smaller wins should be
read alongside repeat noise; they are not guarantees across machines.

The August report's 872×/760× geometric-mean slowdowns no longer describe this
compiler on these workloads. This is **not** a controlled before/after speedup:
the machine differs, generated dependency resolution can differ, and the current
preparation script enables mimalloc for both libraries. Node 22.22.2 and Rust
1.96.1 match the August report, and both source-library refs remain pinned.

## What changed in the interpretation

The old analysis described `SmeltList::clone` as a deep copy. In the measured
source, `crates/smelt-runtime/src/value/list.rs` implements cloning by sharing an
`Rc<RefCell<Vec<T>>>`. Freshly generated es-toolkit `chunk` also borrows the input
for slicing. Those old deep-copy explanations must not be applied indiscriminately
to the current emitter.

`chunk` is now 2.7× slower than Node for es-toolkit and 3.1× for Remeda in this run.
There are still substantial workload-specific gaps:

| Case | Slowdown vs Node |
| --- | ---: |
| Remeda `unique_by` | 1,995× |
| Remeda `unique` | 358× |
| Remeda `difference` | 98× |
| Remeda `intersection` | 92× |
| es-toolkit `sort_by` | 22× |
| es-toolkit `sum_by` | 17× |

These are priorities for profiling; this rerun does not establish their root causes.
The es-toolkit numeric `unique_typed` and `chunk_typed` variants are about 1.79× and
1.77× faster than their erased Rust counterparts, with matching checksums. The
`partition_typed` workload uses different input data, so it is not a controlled
comparison of erasure overhead against the ordinary `partition` row.

## Limits and validation

This measures the repository's library-operation corpus against Node, not ScriptC,
handwritten Rust, server throughput, or arbitrary applications. There was no concurrent
compilation during the timed sweep. Both libraries used mimalloc. Generated-crate
Cargo lockfiles are retained beside the environment record for dependency provenance.

The report's `Rust retained/op` column is peak RSS above an idle baseline divided
by iteration count. It is a footprint heuristic, **not proof of a leak**; establishing
a leak requires memory-growth measurements as the number of calls increases.

The required broader checks were attempted on the same unchanged compiler commit
in the preceding session: `cargo clippy --all-targets` reported four existing errors
in `smelt-frontend-ts` (arguments forwarding, callback transforms, matcher diagnostics,
and function statics). `cargo test` completed groups totaling 937 passed tests and
125 ignored tests before stalling in `python_specialization_parity`; that run was
interrupted and must not be reported as a full test-suite pass. The benchmark builds,
three-repeat sweep, completeness checks, and paired checksums all passed.

## Generated erasure report

The benchmark emits source without upstream test files; the checked-in compatibility
baselines include a different corpus. The advisory deltas below therefore must not
be presented as an optimization improvement or used to replace those baselines.
Injected `smelt_bench_*` modules were excluded from the scan. No compiler erasure
behavior changed in this report-only update.

| Library | Category | Existing baseline | Benchmark corpus | Delta |
| --- | --- | ---: | ---: | ---: |
| es-toolkit | Runtime prelude | 3,859 | 1,140 | -2,719 |
| es-toolkit | Legitimate boundary | 39,359 | 4,085 | -35,274 |
| es-toolkit | Avoidable erasure | 34,072 | 2,986 | -31,086 |
| Remeda | Runtime prelude | 1,445 | 932 | -513 |
| Remeda | Legitimate boundary | 56,468 | 7,631 | -48,837 |
| Remeda | Avoidable erasure | 25,336 | 3,658 | -21,678 |
