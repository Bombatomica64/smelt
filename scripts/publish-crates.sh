#!/usr/bin/env bash
#
# Publish the Smelt crates to crates.io in dependency order.
#
# Ruff and ty's library-layer crates are available on crates.io, so the full
# Python frontend and its `ty` feature are published without manifest rewriting.
#
# Usage:
#   scripts/publish-crates.sh            # dry run: `cargo package` every crate
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

# Crates that cannot be published yet, with the dependency that stops them. Any
# change to this set is a decision, so a difference fails the run.
KNOWN_BLOCKED="smelt-transpiler smelt-ts-types"

BLOCKED="$(python3 scripts/publish_order.py --blocked)"
if [[ "$BLOCKED" != "$KNOWN_BLOCKED" ]]; then
  echo "Blocked crates changed. Expected: '$KNOWN_BLOCKED'. Found: '$BLOCKED'." >&2
  exit 1
fi

# Publish order: every crate after its path dependencies, derived from the
# manifests.
mapfile -t CRATES < <(python3 scripts/publish_order.py)

# Returns 0 if <crate> already has VERSION on crates.io (so a resumed run can
# skip it — new-crate publishing is rate-limited, so a batch may need >1 pass).
already_published() {
  local crate="$1"
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    "https://crates.io/api/v1/crates/$crate/$VERSION" -A "smelt-publish")"
  [[ "$code" == "200" ]]
}

if [[ "$EXECUTE" -eq 0 ]]; then
  # Multi-package mode stages each crate for the ones after it, so a crate that
  # has never been published still resolves.
  selected=()
  for crate in "${CRATES[@]}"; do
    selected+=(-p "$crate")
  done
  cargo package "${selected[@]}" --no-default-features --no-verify --allow-dirty
  echo
  echo "Packaged ${#CRATES[@]} crates. Not packaged: $BLOCKED."
  exit 0
fi

if [[ -n "$BLOCKED" ]]; then
  echo "Refusing to publish a partial release. Blocked: $BLOCKED." >&2
  exit 1
fi

published=0
for crate in "${CRATES[@]}"; do
  echo "==============================================================="
  echo ">>> $crate"
  echo "==============================================================="

  if already_published "$crate"; then
    echo "already on crates.io at $VERSION — skipping"
    continue
  fi

  cargo publish -p "$crate" --no-default-features --allow-dirty
  published=$((published + 1))
done

if [[ "$published" -eq 0 ]]; then
  echo "Nothing was published: every crate is already at $VERSION. Bump the version." >&2
  exit 1
fi
echo "Published $published crate(s)."
