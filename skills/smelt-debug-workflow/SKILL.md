---
name: smelt-debug-workflow
description: Debug and validate Smelt-generated Rust compatibility work through concise Markdown reports produced by the Smelt CLI. Use when multiple agents or repositories need to reproduce generated test failures, compare test-suite progress, protect focused regressions, inspect generated Rust diagnostics, or choose the next general transpiler defect to fix.
---

# Smelt Debug Workflow

Use Markdown reports as the handoff artifact between agents. Keep large Cargo
output out of reasoning context unless a focused failure excerpt is inadequate.

## Procedure

1. Build the source project and reproduce relevant generated tests with
   `smelt rust-test-report`.
2. Read the output Markdown report. Select a repeated semantic failure family,
   not a source-library function special case.
3. Inspect only the involved source fixture, emitted Rust/MIR, and transpiler
   code needed for that family.
4. Implement a general frontend, IR, emitter, or runtime fix and add focused
   compiler regression tests.
5. Generate a new Markdown report with focused filters, regression guards,
   `--full`, `--diagnostics`, and `--baseline-report` pointing to the prior
   report.
6. Run the repository's mandated validation before reporting completion.

## Command Shape

```bash
cargo run --bin smelt -- rust-test-report \
  --build-manifest path/to/Smelt.toml \
  --cargo-manifest path/to/generated/Cargo.toml \
  --focus relevant_generated_test_filter \
  --guard previously_fixed_filter \
  --full \
  --diagnostics \
  --suppress-warnings \
  --baseline-report blocker-logs/before.md \
  --output blocker-logs/after.md
```

Omit `--baseline-report` for the first measurement. Repeat `--focus` and
`--guard` as needed. The generated crate and report paths belong to the
repository under investigation; the workflow itself is not tied to Remeda.

## Constraints

- Make semantic decisions in transpiler code, not in the report workflow.
- Do not add function-name-specific lowering for third-party tests.
- Do not commit generated third-party distribution artifacts unless the
  repository explicitly tracks them as golden output.
- Preserve Smelt's incremental generated-file write behavior.
