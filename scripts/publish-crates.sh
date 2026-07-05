#!/usr/bin/env bash
#
# Publish the Smelt crates to crates.io in dependency order.
#
# Python is publishable on the NON-`ty` path. `smelt-frontend-py` now pulls the
# Ruff parser from crates.io (real `ruff_python_ast`/`ruff_python_parser`/
# `ruff_text_size` `0.0.3` releases), so the parser-based Python frontend can be
# uploaded. What is NOT publishable is the `ty` feature: it pulls
# `smelt-py-types`, whose `ty_project`/`ruff_db`/`ty_python_semantic` tree is
# git-pinned (not on crates.io). So the published crates keep the `python`
# feature but have every `ty` reference stripped:
#
#   * smelt-frontend-py  — strip the `ty` feature + the optional git-only
#                          `smelt-py-types` dep (keeps the crates.io Ruff deps).
#   * smelt-transpiler   — strip the `ty` feature + retarget `default` to
#                          `python`; keep the optional `smelt-frontend-py` dep
#                          (given a crates.io version).
#   * smelt-gui          — keep `python`/`smelt-frontend-py` as-is (no `ty`);
#                          only the dep needs a crates.io version.
#   * smelt-codegen-rust — strip the `ty` feature (which forwarded
#                          `smelt-frontend-py/ty`) and empty its `default`. Its
#                          `smelt-frontend-py` is a dev-dependency, which cargo
#                          drops from the published package automatically, so it
#                          needs no version.
#
# The working-tree manifests keep the `ty` surface so `--features ty` still
# builds the full pipeline from a source checkout; the strips below apply only to
# the manifests that are uploaded and are reverted right after.
#
# Because `cargo package -p <crate>` resolves the WHOLE workspace, a dangling
# `smelt-frontend-py/ty` reference in any sibling manifest fails resolution even
# while packaging a different crate. All manifests are therefore stripped ONCE
# up front (before the packaging loop) and restored ONCE at the end, rather than
# per-crate.
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
ROOT="$(pwd)"

EXECUTE=0
if [[ "${1:-}" == "--execute" ]]; then
  EXECUTE=1
fi

VERSION="0.1.0"

# Publish order: every crate appears after all of its path dependencies.
# `smelt-frontend-py` sits after its deps (smelt-hir/stdlib/specialize/asyncio)
# and before the crates that depend on it (codegen-rust/transpiler/gui).
CRATES=(
  smelt-hir
  smelt-stdlib
  smelt-runtime
  smelt-asyncio
  smelt-test
  smelt-specialize
  smelt-frontend-py
  smelt-mir
  smelt-codegen-rust
  smelt-frontend-ts
  smelt-transpiler
  smelt-gui
)

# Manifests whose `ty`/git-only surface must be stripped before packaging.
# Backups live OUTSIDE the crate directories (under target/) so they are never
# packaged into a crate; each entry is "backupfile|manifestpath".
BAKDIR="$ROOT/target/publish-bak"
mkdir -p "$BAKDIR"
BACKUPS=()
restore_manifests() {
  for entry in "${BACKUPS[@]:-}"; do
    [[ -n "$entry" ]] || continue
    local backup="${entry%%|*}"
    local target="${entry##*|}"
    [[ -e "$backup" ]] && mv -f "$backup" "$target"
  done
  BACKUPS=()
}
trap restore_manifests EXIT

# Back up a manifest before editing so it can be restored after packaging.
backup_manifest() {
  local crate="$1"
  local manifest="$ROOT/crates/$crate/Cargo.toml"
  local backup="$BAKDIR/$crate.Cargo.toml.bak"
  cp "$manifest" "$backup"
  BACKUPS+=("$backup|$manifest")
}

# Strip every `ty`/git-only reference from the manifests that are uploaded.
# `python` (parser-based Python) is intentionally KEPT.
strip_ty_surface() {
  local fpy="$ROOT/crates/smelt-frontend-py/Cargo.toml"
  local transpiler="$ROOT/crates/smelt-transpiler/Cargo.toml"
  local gui="$ROOT/crates/smelt-gui/Cargo.toml"
  local codegen="$ROOT/crates/smelt-codegen-rust/Cargo.toml"

  backup_manifest smelt-frontend-py
  backup_manifest smelt-transpiler
  backup_manifest smelt-gui
  backup_manifest smelt-codegen-rust

  # smelt-frontend-py: drop its own `ty` feature and the git-only
  # `smelt-py-types` dep; the default was `["ty"]`, drop it too (publish uses
  # --no-default-features so the default is irrelevant, but it must not name a
  # removed feature). Keeps the crates.io `ruff_*` deps.
  sed -i \
    -e '/^default = \["ty"\]$/d' \
    -e '/^ty = \["dep:smelt-py-types"\]$/d' \
    -e '/^smelt-py-types = { path = "\.\.\/smelt-py-types", optional = true }$/d' \
    "$fpy"

  # smelt-transpiler: keep `python`; drop the `ty` feature; retarget the default
  # from `ty` to `python`; give the optional frontend-py dep a crates.io version.
  sed -i \
    -e 's/^default = \["ty"\]$/default = ["python"]/' \
    -e '/^ty = \["python", "smelt-frontend-py\/ty"\]$/d' \
    -e 's|^smelt-frontend-py = { path = "\.\./smelt-frontend-py", optional = true }$|smelt-frontend-py = { path = "../smelt-frontend-py", version = "'"$VERSION"'", optional = true }|' \
    "$transpiler"

  # smelt-gui: `python` is already the default and there is no `ty` feature; just
  # give the optional frontend-py dep a crates.io version.
  sed -i \
    -e 's|^smelt-frontend-py = { path = "\.\./smelt-frontend-py", optional = true }$|smelt-frontend-py = { path = "../smelt-frontend-py", version = "'"$VERSION"'", optional = true }|' \
    "$gui"

  # smelt-codegen-rust: drop the `ty` feature (it forwarded frontend-py/ty) and
  # empty its `default`. The frontend-py reference is a dev-dependency, which
  # cargo drops from the published package, so it needs no version rewrite.
  sed -i \
    -e 's/^default = \["ty"\]$/default = []/' \
    -e '/^ty = \["smelt-frontend-py\/ty"\]$/d' \
    "$codegen"
}

# Returns 0 if <crate> already has VERSION on crates.io (so a resumed run can
# skip it — new-crate publishing is rate-limited, so a batch may need >1 pass).
already_published() {
  local crate="$1"
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    "https://crates.io/api/v1/crates/$crate/$VERSION" -A "smelt-publish")"
  [[ "$code" == "200" ]]
}

# Strip once, up front — a sibling's dangling `ty` reference would otherwise
# break `cargo package -p <other-crate>` (workspace-wide resolution).
strip_ty_surface

for crate in "${CRATES[@]}"; do
  echo "==============================================================="
  echo ">>> $crate"
  echo "==============================================================="

  if [[ "$EXECUTE" -eq 1 ]] && already_published "$crate"; then
    echo "already on crates.io at $VERSION — skipping"
    continue
  fi

  if [[ "$EXECUTE" -eq 1 ]]; then
    # --allow-dirty: the up-front strips leave manifests modified in the working
    # tree; that is intentional and restored on exit.
    cargo publish -p "$crate" --no-default-features --allow-dirty
  else
    # --no-verify checks manifest publishability (versions present, no git deps)
    # without needing the dependency crates to already be on crates.io.
    cargo package -p "$crate" --no-default-features --no-verify --allow-dirty
  fi
done

# Restore every stripped manifest (also runs on the EXIT trap for safety).
restore_manifests

echo
if [[ "$EXECUTE" -eq 1 ]]; then
  echo "All crates published."
else
  echo "Dry run complete. Re-run with --execute to publish (requires a crates.io token)."
fi
