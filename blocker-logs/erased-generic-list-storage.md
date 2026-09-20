# Preserve storage across already-erased generic array conversions

Base: `464adddb113ac0a86adbf7a8791fca745fe897bb`.
Remeda: `3c80f28bb394edbf89f1fc9978571dec8ed20edc`.

## Change

Reuse the existing scope-aware representation predicate for outbound list erasure and list-to-list coercion, in both operand and rendered-value paths. An out-of-scope generic element already renders as `SmeltUnknown`; rebuilding its buffer both wastes work and breaks shared mutations. Concrete element types and in-scope Rust generics retain element adapters. No library-name special cases or runtime ABI changes.

The callback fixture also exposed an identity map while passing `unknown[]` to an erased generic parameter. This conversion now preserves the buffer too.

## Targeted performance

Identical existing 10,000-record Remeda `unique_by` case, release builds, mimalloc, unchanged generated Cargo.lock, Node 22.22.2, Rust 1.96.1, three full-budget repetitions on the same machine. No concurrent compilation during measurements. Values below are medians of the three throughput observations, not the runner's selected best observation.

| Native throughput | Before | After |
| --- | ---: | ---: |
| Operations/second | 0.864576 | 1.710384 |

Speedup: **1.98×**. All Node and Rust output checksums match (`3475094833`). Node median after: 2531.0 ops/s; native output remains approximately 1480× slower in this case. This removes one copying source, not the entire bottleneck. No full-library speedup is claimed.

Raw observations: `erased-generic-list-benchmark-before.json` and `erased-generic-list-benchmark-after.json`.

Reproduction after preparing the pinned Remeda benchmark with `benchmarks/prepare.py` and building the generated crate:

```sh
python3 benchmarks/run.py --only remeda --case unique_by --repeats 3 --out results.json
```

## Validation

- Codegen unit suite: 978 passed.
- New explicitly executed `list_reference_semantics_runtime` integration test: passed.
- Generated fixture baseline: 1 passed, 2 failed. Patched: 3 passed, 0 failed. Reports alongside this file.
- `cargo check --lib --no-default-features`: passed.
- `cargo clippy --lib --no-default-features`: passed with existing warnings.
- `cargo clippy --all-targets --no-default-features`: blocked by existing diagnostics outside the changed code (including `list_escape_report.rs`, `type_substitution.rs`, and older tests in `part_7_tests.rs`; an earlier run also stopped in `map_lookup_runtime.rs`).
- `timeout 240s cargo test --no-default-features`: timed out in `python_specialization_parity` after 978 passed / 157 ignored across completed test binaries. The workspace still builds Python members with this flag. This is not a green full-workspace test result.
- `git diff --check`: passed.

## Erasure audit and remaining review gate

Same 214 generated Remeda source files before/after, excluding injected `smelt_bench_*` files:

| Classification | Before | After | Delta |
| --- | ---: | ---: | ---: |
| Total occurrences | 12,277 | 12,277 | 0 |
| Runtime prelude | 1,098 | 1,098 | 0 |
| Legitimate boundary | 7,612 | 7,590 | -22 |
| Avoidable erasure | 3,567 | 3,589 | +22 |

The existing classifier is line-based: removing an element adapter also removes its boundary marker, reclassifying occurrences on those lines. No new tagged values or erasure sites are introduced. This reported avoidable increase still needs classifier/baseline review under AGENTS.md before merge; this PR does not silently reset the ratchet or claim it passed. Keep the PR draft until that review and full CI validation are complete.
