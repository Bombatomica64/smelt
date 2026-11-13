#!/usr/bin/env python3
"""Select the workspace packages whose tests a change can affect.

Used by the `test` job of .github/workflows/ci.yml on pull requests. Given the
list of changed files, it prints the `cargo test` package arguments that cover
them:

* a file inside a workspace member's directory selects that member and every
  member that depends on it, directly or transitively (normal, dev and build
  dependencies alike), since their tests compile against it;
* `examples/**` selects `smelt-transpiler`, whose integration tests golden-check
  that corpus;
* a file on the IGNORED list selects nothing: no `cargo test` reads it;
* ANY OTHER file (root `Cargo.toml`/`Cargo.lock`, the toolchain file, this
  script, the CI workflow, or a path this script does not know) selects the
  whole workspace. Unknown means "run everything", never "run nothing".

The selection is sound only because no crate reads another crate's files by
path: every cross-crate input goes through a Cargo dependency edge (checked when
this script was written: no `include_str!`/`fs::read` of a sibling crate, and
`corgi.toml` lists only in-crate compile-time inputs). A test that starts
reading another crate's files must add that edge or extend `PATH_RULES`.

Output (also written to `$GITHUB_OUTPUT` when set):
  mode=all|some|none
  args=<cargo test package arguments>   (`--workspace` or `-p a -p b ...`)

Usage: ci_affected_packages.py <changed-file-list | -> [--metadata <json>]
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

#: Paths no `cargo test` of the workspace reads. Changing only these runs no
#: tests in the PR `test` job (the jobs that do read them, such as the library
#: regression gates for `third_party/` and `blocker-logs/`, have their own
#: triggers). Directory entries end with `/`.
IGNORED = (
    "third_party/",
    "blocker-logs/",
    "devlog/",
    "site/",
    "skills/",
    "specs/",
    "benchmarks/",
    "crates/smelt-gui/",  # not a workspace member; its own workspace
    "corgi.toml",
    "justfile",
    "tombi.toml",
    ".github/workflows/compat.yml",
    ".github/workflows/library-probes.yml",
    ".github/workflows/pages.yml",
    ".github/workflows/publish-crates.yml",
    ".github/workflows/publish-smelt.yml",
    ".github/workflows/coverage.yml",
    ".github/workflows/regressions-label.yml",
    ".github/workflows/shape-grid.yml",
)

#: Non-crate paths read by a specific member's tests. Directory entries end
#: with `/`.
PATH_RULES = {
    "examples/": ("smelt-transpiler",),
}


def workspace_graph(metadata: dict) -> tuple[dict[str, Path], dict[str, set[str]]]:
    """Return each member's directory (relative to the workspace root) and the
    members that depend on it (its direct reverse dependencies)."""
    root = Path(metadata["workspace_root"])
    member_ids = set(metadata["workspace_members"])
    members = [package for package in metadata["packages"] if package["id"] in member_ids]
    names = {package["name"] for package in members}
    directories = {
        package["name"]: Path(package["manifest_path"]).parent.relative_to(root)
        for package in members
    }
    dependents: dict[str, set[str]] = {name: set() for name in names}
    for package in members:
        for dependency in package["dependencies"]:
            if dependency["name"] in names and dependency["name"] != package["name"]:
                dependents[dependency["name"]].add(package["name"])
    return directories, dependents


def owning_member(path: str, directories: dict[str, Path]) -> str | None:
    """Return the member whose directory contains `path` (the deepest one)."""
    best: tuple[int, str] | None = None
    for name, directory in directories.items():
        prefix = directory.as_posix().rstrip("/") + "/"
        if path.startswith(prefix) and (best is None or len(prefix) > best[0]):
            best = (len(prefix), name)
    return best[1] if best else None


def matches(path: str, entry: str) -> bool:
    """Whether `path` is the file `entry` or lies under the directory `entry/`."""
    return path.startswith(entry) if entry.endswith("/") else path == entry


def select(changed: list[str], directories: dict[str, Path], dependents: dict[str, set[str]]):
    """Return (mode, packages, reasons) for the changed files."""
    seeds: set[str] = set()
    reasons: list[str] = []
    for path in changed:
        member = owning_member(path, directories)
        if member is not None:
            seeds.add(member)
            continue
        rule = next((entry for entry in PATH_RULES if matches(path, entry)), None)
        if rule is not None:
            seeds.update(PATH_RULES[rule])
            continue
        if any(matches(path, entry) for entry in IGNORED):
            continue
        reasons.append(f"{path} is outside every workspace member")
    if reasons:
        return "all", sorted(directories), reasons
    selected = set(seeds)
    pending = list(seeds)
    while pending:
        for dependent in dependents[pending.pop()]:
            if dependent not in selected:
                selected.add(dependent)
                pending.append(dependent)
    return ("some" if selected else "none"), sorted(selected), reasons


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("changed", help="file with one changed path per line, or - for stdin")
    parser.add_argument("--metadata", help="`cargo metadata --no-deps` JSON (default: run cargo)")
    options = parser.parse_args()

    text = sys.stdin.read() if options.changed == "-" else Path(options.changed).read_text()
    changed = [line.strip() for line in text.splitlines() if line.strip()]
    if options.metadata:
        metadata = json.loads(Path(options.metadata).read_text())
    else:
        metadata = json.loads(
            subprocess.run(
                ["cargo", "metadata", "--format-version", "1", "--no-deps"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout
        )

    directories, dependents = workspace_graph(metadata)
    mode, packages, reasons = select(changed, directories, dependents)
    args = "--workspace" if mode == "all" else " ".join(f"-p {name}" for name in packages)

    print(f"{len(changed)} changed file(s); mode={mode}", file=sys.stderr)
    for reason in reasons[:20]:
        print(f"  whole workspace: {reason}", file=sys.stderr)
    if mode == "some":
        print(f"  packages: {', '.join(packages)}", file=sys.stderr)
    print(f"mode={mode}")
    print(f"args={args}")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"mode={mode}\nargs={args}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
