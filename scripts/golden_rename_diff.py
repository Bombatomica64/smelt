#!/usr/bin/env python3
"""Review a golden re-bless by its residue: the diff a known rename does not explain.

When a representation change rewrites nearly every `expected.rs` (UTF-16
strings spell `JsString::from("x")` where they spelled `"x".to_owned()`), the
raw diff is too large to read. This script applies the known mechanical
rewrites to the OLD golden, diffs the result against the NEW golden, and
prints only what is left: the residue a reviewer has to read.

Rules apply to program code only. A line of the runtime crate's item files,
or one that three old goldens share, is runtime prelude text, which changes in the runtime crate and is left
as it was, so a prelude change appears as residue rather than being renamed
away.

The residue is grouped. A hunk that appears identically in many goldens (a
runtime item every crate carries) is printed once, with the number of goldens
and a few of their names, so a prelude change is reviewed once rather than 168
times. Hunks are compared with their line numbers removed.

Usage:
    scripts/golden_rename_diff.py [--base REV] [--rules NAME ...] [--show N] [PATH ...]

PATH defaults to every `examples/typescript/end-to-end/*/expected.rs`. The old
side is `git show REV:PATH` (default `HEAD`); the new side is the working tree.
`--rules` picks rule sets from `RULE_SETS` (default: all of them).
"""

from __future__ import annotations

import argparse
import difflib
import glob
import re
import subprocess
import sys
from collections import defaultdict

# Each rule set is a list of (pattern, replacement) applied in order with
# `re.sub`. A rule must be a rename the change makes everywhere; anything
# conditional belongs in the residue, where a reviewer reads it.
RULE_SETS: dict[str, list[tuple[str, str]]] = {
    # The JavaScript string type is spelled through the `JsString` API.
    "js_string_spelling": [
        (r'(?<![A-Za-z_])""\.to_owned\(\)', "JsString::new()"),
        (r'("(?:[^"\\]|\\.)*")\.to_owned\(\)', r"JsString::from(\1)"),
        (r'(?<![:\w])String\b(?!::)', "JsString"),
    ],
}


def old_text(rev: str, path: str) -> str | None:
    """The golden at `rev`, or None when it did not exist there."""
    result = subprocess.run(
        ["git", "show", f"{rev}:{path}"], capture_output=True, text=True, check=False
    )
    return result.stdout if result.returncode == 0 else None


def apply_rules(text: str, names: list[str], prelude: set[str]) -> str:
    """`text` with every rule of the named sets applied in order to each line
    that is not a prelude line.

    The rules describe how the emitter spells a program; runtime prelude text
    changes in the runtime crate, where it is reviewed once, so its lines are
    left as they were and any change to them shows up as residue.
    """
    out = []
    for line in text.split("\n"):
        if line not in prelude:
            for name in names:
                for pattern, replacement in RULE_SETS[name]:
                    line = re.sub(pattern, replacement, line)
        out.append(line)
    return "\n".join(out)


def prelude_lines(rev: str, texts: list[str]) -> set[str]:
    """The runtime prelude's lines: every line of the runtime crate's item
    files at `rev` (an alternate `//= ` line as the text it emits), and every
    line at least three goldens share, which catches prelude text the emitter
    holds itself."""
    lines: set[str] = set()
    listing = subprocess.run(
        ["git", "ls-tree", "--name-only", f"{rev}:crates/smelt-runtime/src/runtime/"],
        capture_output=True, text=True, check=False,
    ).stdout.split()
    for name in listing:
        source = old_text(rev, f"crates/smelt-runtime/src/runtime/{name}") or ""
        for line in source.split("\n"):
            lines.add(line[4:] if line.startswith("//= ") else line)
    counts: dict[str, int] = defaultdict(int)
    for text in texts:
        for line in set(text.split("\n")):
            counts[line] += 1
    lines.update(line for line, count in counts.items() if count >= 3)
    return lines


def hunks(old: str, new: str) -> list[str]:
    """The unified-diff hunks between `old` and `new`, without line numbers."""
    diff = difflib.unified_diff(old.splitlines(), new.splitlines(), lineterm="", n=1)
    out: list[str] = []
    current: list[str] = []
    for line in diff:
        if line.startswith(("---", "+++")):
            continue
        if line.startswith("@@"):
            if current:
                out.append("\n".join(current))
            current = []
            continue
        current.append(line)
    if current:
        out.append("\n".join(current))
    return out


def main() -> int:
    """Prints the grouped residue; exits 1 when any residue is left."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", default="HEAD")
    parser.add_argument("--rules", nargs="*", default=list(RULE_SETS))
    parser.add_argument("--show", type=int, default=3, help="golden names listed per hunk")
    parser.add_argument("paths", nargs="*")
    args = parser.parse_args()
    paths = args.paths or sorted(glob.glob("examples/typescript/end-to-end/*/expected.rs"))

    groups: dict[str, list[str]] = defaultdict(list)
    new_files = []
    pairs = []
    for path in paths:
        before = old_text(args.base, path)
        if before is None:
            new_files.append(path)
            continue
        with open(path, encoding="utf-8") as handle:
            pairs.append((path, before, handle.read()))
    prelude = prelude_lines(args.base, [before for _, before, _ in pairs])
    for path, before, after in pairs:
        for hunk in hunks(apply_rules(before, args.rules, prelude), after):
            name = path.split("/")[-2]
            if name not in groups[hunk]:
                groups[hunk].append(name)

    for hunk, names in sorted(groups.items(), key=lambda item: (-len(item[1]), item[0])):
        shown = ", ".join(names[: args.show]) + (" ..." if len(names) > args.show else "")
        print(f"=== {len(names)} golden(s): {shown}")
        print(hunk)
        print()
    for path in new_files:
        print(f"=== new golden (no base to diff): {path}")
    total = sum(len(names) for names in groups.values())
    print(f"residue: {len(groups)} distinct hunk(s), {total} in all, {len(paths)} golden(s) read", file=sys.stderr)
    return 1 if groups else 0


if __name__ == "__main__":
    sys.exit(main())
