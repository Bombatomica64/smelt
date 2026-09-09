#!/usr/bin/env bash
# Regenerate `expected.hir` and `expected.mir` for the TypeScript end-to-end
# examples.
#
# The companion to `regen-example-rust.sh`. Both dumps print the module's whole
# TYPE TABLE, so any change to the order in which types are interned renumbers
# or reorders it across every example that reaches the changed path — a
# semantically empty diff that still has to land in the goldens. The
# cross-language test stops at the FIRST mismatch, so regenerating one example
# per run turns a table-wide shift into one test cycle per affected example;
# this does them all in one pass.
#
# Usage:
#   scripts/regen-example-dumps.sh <smelt-binary> [example ...]
#
# With no example names, every directory under examples/typescript/end-to-end
# that has an `expected.hir` is regenerated. Only files whose bytes actually
# change are written, so unaffected goldens keep their mtimes.
set -euo pipefail

smelt=$1
shift

root="examples/typescript/end-to-end"
if [ "$#" -gt 0 ]; then
  examples=("$@")
else
  examples=()
  for dir in "$root"/*/; do
    name=$(basename "$dir")
    [ -f "$dir/expected.hir" ] || continue
    examples+=("$name")
  done
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for name in "${examples[@]}"; do
  input="$root/$name/input.ts"
  [ -f "$input" ] || { echo "skip $name (no input.ts)"; continue; }
  for kind in hir mir; do
    "$smelt" "dump-$kind" "$input" >"$tmp/out"
    if cmp -s "$tmp/out" "$root/$name/expected.$kind"; then
      continue
    fi
    cp "$tmp/out" "$root/$name/expected.$kind"
    echo "updated $name/expected.$kind"
  done
done
