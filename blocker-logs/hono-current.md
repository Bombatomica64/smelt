# Probe report: hono

- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: not run (pass `--run-tests`)
- Files scanned: 258 · with blockers: 0


## Whole-crate `cargo check` (orchestrator, integration head after standards round 27)

Committed overlay, fresh clone at the pinned ref, full-feature `smelt`: the complete closure (258
files) transpiles and the crate is emitted. `cargo check` on `dist-smelt`: **883 errors**.

| code | count |
| --- | ---: |
| E0609 (no field) | 437 |
| E0107 (generic argument count) | 222 |
| E0308 (mismatched types) | 135 |
| E0599 (no method) | 33 |
| E0277 (trait bound) | 29 |
| E0560, E0382, E0121, E0425, E0063 | 12, 8, 4, 2, 1 |

This table, not the router slice, is phase 2's metric from here on.
