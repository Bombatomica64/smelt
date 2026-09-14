#!/usr/bin/env bash
# Regenerate `expected.rs` for the TypeScript end-to-end examples.
#
# The golden is EVERY generated file, not just `main.rs`: a program whose
# lowering splits into modules leaves `main.rs` holding the runtime prelude and
# a `mod` declaration while its user code goes to `source_<entry>.rs`, so a
# `main.rs`-only golden checked the prelude and nothing the fixture was written
# to exercise. The files are concatenated in a deterministic order — `main.rs`
# first, the rest sorted — with each section after the first introduced by a
# `// ==== <file>` comment line.
#
# That concatenation is implemented ONCE, in the test harness
# (`crates/smelt-transpiler/tests/common/mod.rs`), and this script runs the
# harness in rewrite mode rather than rebuilding the same text in shell: a
# second implementation would drift from the one the suite asserts, and the
# first sign of it would be a regeneration that breaks `cargo test`.
#
# Because the goldens are rewritten in place, one pass regenerates every
# affected example — where the assertion path stops at the first mismatch.
#
# Usage:
#   scripts/regen-example-rust.sh [example ...]
#
# With no example names, every fixture in `END_TO_END_EXAMPLES` is regenerated.
# Set CARGO_TARGET_DIR/CARGO_INCREMENTAL as usual; the run compiles and RUNS
# each generated crate, because the harness also checks `expected.stdout`, and a
# stdout mismatch still fails (this script only rewrites the Rust golden).
set -euo pipefail

if [ "$#" -gt 0 ]; then
  only=$(IFS=,; echo "$*")
  export SMELT_EXAMPLE_ONLY="$only"
  echo "regenerating: $only"
else
  echo "regenerating: every end-to-end example"
fi

export SMELT_UPDATE_EXAMPLE_RUST=1
cargo test -p smelt-transpiler --test hir_cli_cross_language_tests \
  end_to_end_examples_match_expected_outputs -- --exact
