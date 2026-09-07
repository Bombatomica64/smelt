#!/usr/bin/env bash
# Regenerate `expected.rs` (and optionally the HIR/MIR dumps) for the
# TypeScript end-to-end examples.
#
# The examples' `expected.rs` is the whole generated `src/main.rs`, prelude
# included, so any change to a runtime prelude that a program pays for moves
# every affected golden at once. Regenerating them one at a time by hand is
# error-prone; this does it the way the test harness does, through a temporary
# project with the harness's own `Smelt.toml`.
#
# Usage:
#   scripts/regen-example-rust.sh <smelt-binary> <scratch-dir> [example ...]
#
# With no example names, every directory under examples/typescript/end-to-end
# that has an `expected.rs` is regenerated. Only files whose bytes actually
# change are written, so unaffected goldens keep their mtimes.
set -euo pipefail

smelt=$1
scratch=$2
shift 2

root="examples/typescript/end-to-end"
if [ "$#" -gt 0 ]; then
  examples=("$@")
else
  examples=()
  for dir in "$root"/*/; do
    name=$(basename "$dir")
    [ -f "$dir/expected.rs" ] || continue
    examples+=("$name")
  done
fi

mkdir -p "$scratch/src"
cat >"$scratch/Smelt.toml" <<'TOML'
[project]
name = "example-app"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "example_app"
build = false

[runtime]
clone-strategy = "aggressive"
TOML

for name in "${examples[@]}"; do
  input="$root/$name/input.ts"
  [ -f "$input" ] || { echo "skip $name (no input.ts)"; continue; }
  cp "$input" "$scratch/src/main.ts"
  rm -rf "$scratch/dist"
  "$smelt" --manifest-path "$scratch/Smelt.toml" build
  if cmp -s "$scratch/dist/src/main.rs" "$root/$name/expected.rs"; then
    continue
  fi
  cp "$scratch/dist/src/main.rs" "$root/$name/expected.rs"
  echo "updated $name"
done
