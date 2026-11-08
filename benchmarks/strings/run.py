#!/usr/bin/env python3
"""Run the UTF-16 string benchmark on Node and as Smelt-generated Rust.

The workload is `utf16_index.ts` beside this file. The script transpiles it
with `smelt build`, builds the generated crate in release mode, runs both
sides, and prints one row per phase: Node's time, the generated crate's time,
and whether the two checksums agree. A row whose checksums differ is a wrong
answer, and its time means nothing.

The library harness (`benchmarks/run.py`) drives prebuilt library bench crates
through a case protocol; this workload is a single program, so it has its own
runner.

Usage:
    python3 benchmarks/strings/run.py --smelt target/release/smelt --work DIR [--peak-rss]

`--work` holds the generated crate and its Cargo target directory; delete it
afterwards. `--peak-rss` also reports each side's peak resident set size
(through GNU `time -f %M`).
"""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "utf16_index.ts"
LINE = re.compile(r"^(?P<phase>.+?) ms=(?P<ms>\d+) checksum=(?P<checksum>\d+)$")

MANIFEST = """[project]
name = "utf16-index"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "utf16_index"
build = false

[runtime]
clone-strategy = "aggressive"
"""


def run(cmd: list[str], peak_rss: bool, **kwargs) -> tuple[str, int | None]:
    """Runs `cmd`, returning its stdout and, when asked, its peak RSS in KB."""
    if peak_rss and shutil.which("/usr/bin/time"):
        cmd = ["/usr/bin/time", "-f", "%M", *cmd]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=True, **kwargs)
    rss = None
    if peak_rss:
        tail = proc.stderr.strip().splitlines()
        rss = int(tail[-1]) if tail and tail[-1].isdigit() else None
    return proc.stdout, rss


def phases(stdout: str) -> dict[str, tuple[int, str]]:
    """The `phase -> (ms, checksum)` rows a run printed."""
    rows = {}
    for line in stdout.splitlines():
        match = LINE.match(line.strip())
        if match:
            rows[match["phase"]] = (int(match["ms"]), match["checksum"])
    return rows


def main() -> int:
    """Builds, runs and prints the comparison; exits 1 on a checksum mismatch."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--smelt", required=True, help="path to the smelt binary")
    parser.add_argument("--work", required=True, help="scratch directory for the generated crate")
    parser.add_argument("--node", default="node")
    parser.add_argument("--peak-rss", action="store_true")
    args = parser.parse_args()

    work = pathlib.Path(args.work).resolve()
    (work / "src").mkdir(parents=True, exist_ok=True)
    shutil.copy(SOURCE, work / "src/main.ts")
    (work / "Smelt.toml").write_text(MANIFEST)
    subprocess.run([args.smelt, "--manifest-path", str(work / "Smelt.toml"), "build"], check=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(work / "target"))
    manifest = str(work / "dist/Cargo.toml")
    subprocess.run(["cargo", "build", "--release", "--quiet", "--manifest-path", manifest], check=True, env=env)
    binary = work / "target/release/utf16_index"

    node_out, node_rss = run([args.node, str(SOURCE)], args.peak_rss)
    rust_out, rust_rss = run([str(binary)], args.peak_rss)
    node_rows, rust_rows = phases(node_out), phases(rust_out)

    mismatch = False
    print(f"{'phase':<22} {'node ms':>8} {'rust ms':>8}  checksums")
    for phase, (node_ms, node_sum) in node_rows.items():
        rust_ms, rust_sum = rust_rows.get(phase, (-1, "missing"))
        agree = "equal" if rust_sum == node_sum else f"DIFFER (node {node_sum}, rust {rust_sum})"
        mismatch |= rust_sum != node_sum
        print(f"{phase:<22} {node_ms:>8} {rust_ms:>8}  {agree}")
    if args.peak_rss:
        print(f"peak RSS KB: node {node_rss}, rust {rust_rss}")
    return 1 if mismatch else 0


if __name__ == "__main__":
    sys.exit(main())
