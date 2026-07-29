## What this changes

<!-- Short description. Link any related issue. -->

## Regression gates

The Remeda / Radash / es-toolkit regression jobs are **opt-in on pull requests**.
They run automatically on pushes to `main`, but a PR only runs them if it carries
the **`run-regressions`** label.

- [ ] This PR does **not** touch transpilation behaviour (docs, CI, comments only), **or**
- [ ] I added the **`run-regressions`** label and the three regression jobs passed.

Add the label if you touched any of:

| Area | Paths |
| --- | --- |
| TypeScript / Python frontends | `crates/smelt-frontend-ts`, `crates/smelt-frontend-py` |
| HIR / MIR | `crates/smelt-hir`, `crates/smelt-mir` |
| Specializer | `crates/smelt-specialize` |
| Rust emitter / codegen | `crates/smelt-codegen-rust` |
| Runtime / stdlib | `crates/smelt-runtime`, `crates/smelt-stdlib`, `crates/smelt-asyncio` |

Without the label these gates are simply absent — not passing. `main` is not
branch-protected, so a green check on an unlabelled codegen PR is not evidence
that Remeda's 1789 tests, Radash's 84 tests, or the es-toolkit ratchet still
hold.

## Ratchets and baselines

- [ ] If this regenerates a corpus, the `smelt-unknown-report` delta is included
      (see `## SmeltUnknown enforcement` in `AGENTS.md`).
- [ ] `cargo clippy --all-targets` and `cargo test` were run locally.
