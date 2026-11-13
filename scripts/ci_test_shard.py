#!/usr/bin/env python3
"""Run one shard of the PR `test` job from a prebuilt nextest archive.

The `build` job of .github/workflows/ci.yml compiles every test binary once and
archives it; each `test` shard unpacks that archive in place and runs a slice of
it, so no shard compiles the workspace.

Most binaries run under nextest (one process per test, sliced across shards
with `--partition slice:K/N`). A few binaries rely on state shared between the
tests of ONE process, which nextest's process-per-test model would silently
drop, so they run under their own libtest harness exactly as `cargo test` runs
them (see PROCESS_BINARIES for each reason). Their work is spread over the
shards too:

* the end-to-end example loop (`END_TO_END_TEST`, ~10 minutes alone) runs on
  every shard, narrowed through `SMELT_EXAMPLE_ONLY` to a weighted round-robin slice of
  `examples/typescript/end-to-end/`. Every name the test's list holds is one of
  those directories (`example_dir` resolves it there), so the slices together
  cover the whole list by construction;
* every other test of the process-shared binaries runs on one shard, binary
  `i` on shard `i mod N + 1`.

Usage:
  ci_test_shard.py filter
      Print the nextest filterset that excludes the process-shared binaries.
  ci_test_shard.py serial --shard K --total N [--metadata PATH]
      Run shard K's share of the process-shared binaries.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Binaries whose tests must share one process, keyed by nextest binary id.
PROCESS_BINARIES = {
    # `SharedDepsCrate::claim` keeps a per-process registry of generated crate
    # names in the shared Cargo build dir; a duplicate name would run another
    # crate's binary. One process per test would empty that registry.
    "smelt-transpiler::hir_cli_cross_language_tests": "crates/smelt-transpiler",
    "smelt-transpiler::cross_language_matrix_tests": "crates/smelt-transpiler",
    # `SERIAL` serializes the timing shapes so they measure growth, not
    # contention; it is a process-local mutex.
    "smelt-codegen-rust::compile_scaling": "crates/smelt-codegen-rust",
}

END_TO_END_BINARY = "smelt-transpiler::hir_cli_cross_language_tests"
END_TO_END_TEST = "end_to_end_examples_match_expected_outputs"
EXAMPLES_DIR = ROOT / "examples/typescript/end-to-end"


def nextest_filter() -> str:
    """The filterset of every test binary except the process-shared ones."""
    excluded = " | ".join(f"binary_id({binary})" for binary in PROCESS_BINARIES)
    return f"not ({excluded})"


def example_names(directory: Path = EXAMPLES_DIR) -> list[str]:
    """Every end-to-end example directory name, sorted."""
    return sorted(entry.name for entry in directory.iterdir() if entry.is_dir())


def example_deck(total: int) -> list[int]:
    """The shard of each position in one round of the example deal.

    Every shard takes two examples per round, except the shard that also runs
    the other tests of the end-to-end binary (~2 minutes, sequential with its
    slice: the two must share one process), which takes one. Measured on CI:
    a quarter of the examples is ~3.5 minutes, so an even deal left that shard
    ~2 minutes behind the rest.
    """
    heavy = next(
        (shard for shard in range(1, total + 1) if END_TO_END_BINARY in assigned_binaries(shard, total)),
        None,
    )
    return [shard for shard in range(1, total + 1) for _ in range(1 if shard == heavy and total > 1 else 2)]


def example_slice(names: list[str], shard: int, total: int) -> list[str]:
    """Shard `shard` (1-based) of `total`: its cards of `names` dealt round-robin over `example_deck`."""
    deck = example_deck(total)
    return [name for index, name in enumerate(names) if deck[index % len(deck)] == shard]


def assigned_binaries(shard: int, total: int) -> list[str]:
    """The process-shared binaries whose other tests run on `shard`."""
    return [binary for index, binary in enumerate(PROCESS_BINARIES) if index % total == shard - 1]


def archived_binaries(metadata_path: Path) -> dict[str, str]:
    """Map every archived binary id to its path."""
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    return {
        binary["binary-id"]: binary["binary-path"]
        for binary in metadata["rust-binaries"].values()
    }


def present(binary: str, archived: dict[str, str]) -> bool:
    """Whether `binary` is in the archive; fail when its package is but it is not.

    A package outside this PR's affected set is legitimately absent. A binary
    missing from a package that IS archived means it was renamed or removed,
    which must not turn into a silently skipped gate.
    """
    if binary in archived:
        return True
    package = binary.split("::", 1)[0]
    if any(other.split("::", 1)[0] == package for other in archived):
        sys.exit(f"{binary} is not in the archive although {package} is; update PROCESS_BINARIES")
    print(f"{binary}: package not selected for this change, skipping")
    return False


def list_tests(path: str, cwd: Path) -> list[str]:
    """The non-ignored test names of a libtest binary."""
    output = subprocess.run(
        [path, "--list", "--format", "terse"], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout
    return [line[: -len(": test")] for line in output.splitlines() if line.endswith(": test")]


def run(command: list[str], cwd: Path, env: dict[str, str] | None = None) -> bool:
    """Run `command`, echoing it; return whether it passed."""
    print("+", " ".join(command), flush=True)
    return subprocess.run(command, cwd=cwd, env={**os.environ, **(env or {})}).returncode == 0


def serial(shard: int, total: int, metadata: Path) -> int:
    """Run this shard's share of the process-shared binaries; return the exit status."""
    archived = archived_binaries(metadata)
    ok = True
    if present(END_TO_END_BINARY, archived):
        cwd = ROOT / PROCESS_BINARIES[END_TO_END_BINARY]
        if END_TO_END_TEST not in list_tests(archived[END_TO_END_BINARY], cwd):
            sys.exit(f"{END_TO_END_TEST} is no longer in {END_TO_END_BINARY}; update this script")
        names = example_slice(example_names(), shard, total)
        if not names:
            sys.exit(f"shard {shard}/{total} got no end-to-end example")
        print(f"end-to-end examples on this shard ({len(names)}): {' '.join(names)}")
        ok &= run(
            [archived[END_TO_END_BINARY], END_TO_END_TEST, "--exact"],
            cwd,
            {"SMELT_EXAMPLE_ONLY": ",".join(names)},
        )
    for binary in assigned_binaries(shard, total):
        if not present(binary, archived):
            continue
        cwd = ROOT / PROCESS_BINARIES[binary]
        tests = list_tests(archived[binary], cwd)
        if binary == END_TO_END_BINARY:
            tests = [test for test in tests if test != END_TO_END_TEST]
        if not tests:
            sys.exit(f"{binary} lists no test")
        ok &= run([archived[binary], "--exact", *tests], cwd)
    return 0 if ok else 1


def main() -> int:
    """Parse the command line and dispatch."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("filter")
    serial_parser = sub.add_parser("serial")
    serial_parser.add_argument("--shard", type=int, required=True)
    serial_parser.add_argument("--total", type=int, required=True)
    serial_parser.add_argument(
        "--metadata", type=Path, default=ROOT / "target/nextest/binaries-metadata.json"
    )
    args = parser.parse_args()
    if args.command == "filter":
        print(nextest_filter())
        return 0
    if not 1 <= args.shard <= args.total:
        sys.exit(f"shard {args.shard} is not in 1..{args.total}")
    return serial(args.shard, args.total, args.metadata)


if __name__ == "__main__":
    sys.exit(main())
