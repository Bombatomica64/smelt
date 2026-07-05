#!/usr/bin/env bash
#
# Publish the Smelt crates to crates.io in dependency order.
#
# The Python frontend (`smelt-frontend-py`) depends on the Ruff parser via a git
# dependency, which crates.io forbids, so it is never published. Three crates
# reference it and must have those references stripped from the manifest that is
# uploaded (the working-tree manifests keep them so `--features python` still
# builds from source):
#
#   * smelt-transpiler  — optional `smelt-frontend-py` dep + `python`/`ty` features
#   * smelt-gui         — optional `smelt-frontend-py` dep + `python` feature
#   * smelt-codegen-rust — `ty` feature forwarding `smelt-frontend-py/ty`
#                          (its Python dev-dependencies are path-only and cargo
#                          drops them from the published package automatically)
#
# Usage:
#   scripts/publish-crates.sh            # dry run: `cargo package` each crate
#   scripts/publish-crates.sh --execute  # real `cargo publish` (needs a token)
#
# Re-runnable: crates already at the current version on crates.io are skipped by
# cargo with an "already uploaded" error, which this script tolerates in
# --execute mode is left to the operator (publish one version once).

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

EXECUTE=0
if [[ "${1:-}" == "--execute" ]]; then
  EXECUTE=1
fi

# Publish order: every crate appears after all of its path dependencies.
CRATES=(
  smelt-hir
  smelt-stdlib
  smelt-runtime
  smelt-asyncio
  smelt-test
  smelt-specialize
  smelt-mir
  smelt-codegen-rust
  smelt-frontend-ts
  smelt-transpiler
  smelt-gui
)

# Manifests that need Python references stripped before packaging/publishing.
BACKUPS=()
restore_manifests() {
  for backup in "${BACKUPS[@]:-}"; do
    [[ -n "$backup" ]] || continue
    local target="${backup%.publish-bak}"
    mv -f "$backup" "$target"
  done
}
trap restore_manifests EXIT

strip_python() {
  local crate="$1"
  local manifest="$ROOT/crates/$crate/Cargo.toml"
  cp "$manifest" "$manifest.publish-bak"
  BACKUPS+=("$manifest.publish-bak")
  case "$crate" in
    smelt-transpiler)
      sed -i \
        -e '/^smelt-frontend-py = { path = "\.\.\/smelt-frontend-py", optional = true }$/d' \
        -e '/^default = \["python"\]$/d' \
        -e '/^python = \["dep:smelt-frontend-py"\]$/d' \
        -e '/^ty = \["python", "smelt-frontend-py\/ty"\]$/d' \
        "$manifest"
      ;;
    smelt-gui)
      sed -i \
        -e '/^smelt-frontend-py = { path = "\.\.\/smelt-frontend-py", optional = true }$/d' \
        -e '/^default = \["python"\]$/d' \
        -e '/^python = \["dep:smelt-frontend-py"\]$/d' \
        "$manifest"
      ;;
    smelt-codegen-rust)
      sed -i \
        -e '/^ty = \["smelt-frontend-py\/ty"\]$/d' \
        "$manifest"
      ;;
  esac
}

needs_strip() {
  case "$1" in
    smelt-transpiler|smelt-gui|smelt-codegen-rust) return 0 ;;
    *) return 1 ;;
  esac
}

for crate in "${CRATES[@]}"; do
  echo "==============================================================="
  echo ">>> $crate"
  echo "==============================================================="

  if needs_strip "$crate"; then
    strip_python "$crate"
  fi

  if [[ "$EXECUTE" -eq 1 ]]; then
    cargo publish -p "$crate" --no-default-features
  else
    # --no-verify checks manifest publishability (versions present, no git deps)
    # without needing the dependency crates to already be on crates.io.
    cargo package -p "$crate" --no-default-features --no-verify --allow-dirty
  fi

  if needs_strip "$crate"; then
    restore_manifests
    BACKUPS=()
  fi
done

echo
if [[ "$EXECUTE" -eq 1 ]]; then
  echo "All crates published."
else
  echo "Dry run complete. Re-run with --execute to publish (requires a crates.io token)."
fi
