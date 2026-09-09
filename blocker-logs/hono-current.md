# Probe report: hono

- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: not run (pass `--run-tests`)
- Files scanned: 258 · with blockers: 0


## Whole-crate `cargo check` (round 28, after the cross-kind type-name ruling)

Committed overlay, fresh clone at the pinned ref, full-feature `smelt`: the complete closure (258
files) transpiles and the crate is emitted. `cargo check` on `dist-smelt`: **326 errors**, down from
883 — 74% of the previous total was one wrongly resolved type name (H71,
`blocker-logs/hono-h71-cross-kind-type-name-collision.md`).

| code | round 27 | round 28 |
| --- | ---: | ---: |
| E0609 (no field) | 437 | 17 |
| E0107 (generic argument count) | 222 | 136 |
| E0308 (mismatched types) | 135 | 138 |
| E0599 (no method) | 33 | 1 |
| E0277 (trait bound) | 29 | 7 |
| E0560 (struct field does not exist) | 12 | 0 |
| E0382 (use of moved value) | 8 | 8 |
| E0121 (type placeholder) | 4 | 10 |
| E0425 (unresolved name) | 2 | 2 |
| E0063 (missing field) | 1 | 5 |
| E0271 (associated type mismatch) | 0 | 1 |
| other | 1 | 1 |

E0107's CAUSE changed: all 136 that remain are `struct takes 3 generic arguments but 1 generic
argument was supplied` — `Context<E>` written against
`class Context<E extends Env = any, P extends string = any, I extends Input = {}>`. That is the
**type-parameter-defaults** family (a type reference that omits trailing type arguments takes the
declaration's defaults) and it is now the largest single family in the crate.

This table, not the router slice, is phase 2's metric from here on.
