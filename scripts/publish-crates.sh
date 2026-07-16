#!/usr/bin/env bash
#
# Publish the Smelt crates to crates.io in dependency order.
#
# Ruff and ty's library-layer crates are available on crates.io, so the full
# Python frontend and its `ty` feature are published without manifest rewriting.
#
# Usage:
#   scripts/publish-crates.sh            # dry run: `cargo package` each crate
#   scripts/publish-crates.sh --execute  # real `cargo publish` (needs a token)
#
# Re-runnable: crates already at the current version on crates.io are skipped in
# --execute mode (new-crate publishing is rate-limited, so a batch may need more
# than one pass).

set -euo pipefail

cd "$(dirname "$0")/.."

EXECUTE=0
if [[ "${1:-}" == "--execute" ]]; then
  EXECUTE=1
fi

# Keep release selection tied to the version committed in the workspace. This
# avoids publishing a CI-only version that is not represented in source control.
VERSION="$(sed -n '/^\[workspace\.package\]$/,/^\[/ s/^version = "\([^"]*\)"$/\1/p' Cargo.toml)"
if [[ -z "$VERSION" ]]; then
  echo "Could not read workspace.package.version from Cargo.toml" >&2
  exit 1
fi

# Publish order: every crate appears after all of its path dependencies.
# `smelt-py-types` precedes `smelt-frontend-py`, which precedes its consumers.
CRATES=(
  smelt-hir
  smelt-stdlib
  smelt-runtime
  smelt-asyncio
  smelt-test
  smelt-specialize
  smelt-py-types
  smelt-frontend-py
  smelt-mir
  smelt-codegen-rust
  smelt-frontend-ts
  smelt-transpiler
  smelt-gui
)

# Returns 0 if <crate> already has VERSION on crates.io (so a resumed run can
# skip it — new-crate publishing is rate-limited, so a batch may need >1 pass).
already_published() {
  local crate="$1"
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    "https://crates.io/api/v1/crates/$crate/$VERSION" -A "smelt-publish")"
  [[ "$code" == "200" ]]
}

for crate in "${CRATES[@]}"; do
  echo "==============================================================="
  echo ">>> $crate"
  echo "==============================================================="

  if [[ "$EXECUTE" -eq 1 ]] && already_published "$crate"; then
    echo "already on crates.io at $VERSION — skipping"
    continue
  fi

  if [[ "$EXECUTE" -eq 1 ]]; then
    cargo publish -p "$crate" --no-default-features --allow-dirty
  elif { [[ "$crate" == "smelt-frontend-py" ]] && ! already_published smelt-py-types; } \
    || { [[ "$crate" == "smelt-transpiler" || "$crate" == "smelt-gui" ]] \
      && ! already_published smelt-frontend-py; }; then
    # Cargo insists that every versioned dependency already exist in the
    # registry even with --no-verify. On the first release, validate the staged
    # file set now; a full package dry run becomes available as soon as the
    # preceding local dependency publishes reach the crates.io index.
    cargo package -p "$crate" --list --allow-dirty >/dev/null
    echo "package dependency check deferred until its new local dependencies are indexed"
  else
    # --no-verify checks manifest publishability (versions present, no git deps)
    # without needing the dependency crates to already be on crates.io.
    cargo package -p "$crate" --no-default-features --no-verify --allow-dirty
  fi
done

echo
if [[ "$EXECUTE" -eq 1 ]]; then
  echo "All crates published."
else
  echo "Dry run complete. Re-run with --execute to publish (requires a crates.io token)."
fi
