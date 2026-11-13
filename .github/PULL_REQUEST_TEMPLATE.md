## What this changes

<!-- Short description. Link any related issue. -->

## Regression gates

A pull request push runs only `build` (compile once, clippy, doctests) and the
sharded `test` jobs. The slow gates (Remeda / Radash / es-toolkit / Hono /
ts-pattern / date-fns regressions, runtime tiers, corpus, sandbox, package) run
on every push to `main`, nightly, and on a PR only when it carries the
**`run-regressions`** label (applying it starts a full run on the PR head).

- [ ] This PR does **not** touch transpilation behaviour (docs, CI, comments only), **or**
- [ ] I added the **`run-regressions`** label and the full run passed.

Add the label if you touched any of:

| Area | Paths |
| --- | --- |
| TypeScript / Python frontends | `crates/smelt-frontend-ts`, `crates/smelt-frontend-py` |
| HIR / MIR | `crates/smelt-hir`, `crates/smelt-mir` |
| Specializer | `crates/smelt-specialize` |
| Rust emitter / codegen | `crates/smelt-codegen-rust` |
| Runtime / stdlib | `crates/smelt-runtime`, `crates/smelt-stdlib` |

Without the label these gates are simply absent — not passing. `main` is not
branch-protected, so a green check on an unlabelled codegen PR is not evidence
that Remeda's 1789 tests, Radash's 387 tests, or the es-toolkit ratchet still
hold.

## Ratchets and baselines

- [ ] If this regenerates a corpus, the `smelt-unknown-report` delta is included
      (see `## SmeltUnknown enforcement` in `AGENTS.md`).
- [ ] `cargo clippy --all-targets` and `cargo test` were run locally.
