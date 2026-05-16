#!/usr/bin/env python3
"""Update README coverage sections from coverage artifacts.

This script updates or appends a dedicated coverage section in the README with:
- Workspace coverage totals from `coverage-summary.json`.
- Per-crate coverage table computed from `lcov.info` source-file records.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from pathlib import Path

START_MARKER = "<!-- COVERAGE:START -->"
END_MARKER = "<!-- COVERAGE:END -->"


@dataclass
class Counter:
    """Simple found/hit coverage counters."""

    found: int = 0
    hit: int = 0

    def ratio(self) -> float:
        """Return percentage in the `[0, 100]` range."""
        if self.found <= 0:
            return 0.0
        return (self.hit / self.found) * 100.0


def load_workspace_summary(path: Path) -> dict[str, float]:
    """Load workspace coverage percentages from cargo-llvm-cov summary JSON."""
    data = json.loads(path.read_text(encoding="utf-8"))
    totals = data.get("data", [{}])[0].get("totals", {})

    def pct(key: str) -> float:
        bucket = totals.get(key, {})
        value = bucket.get("percent")
        if value is None:
            return 0.0
        return float(value)

    return {
        "functions": pct("functions"),
        "lines": pct("lines"),
        "regions": pct("regions"),
        "branches": pct("branches"),
    }


def parse_lcov_per_crate(path: Path) -> dict[str, dict[str, Counter]]:
    """Parse LCOV and aggregate per-crate counters for Rust workspace crates."""
    per_crate: dict[str, dict[str, Counter]] = {}

    current_file: str | None = None
    current_crate: str | None = None

    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()

        if line.startswith("SF:"):
            current_file = line[3:]
            current_crate = None
            token = "/crates/"
            if token in current_file:
                after = current_file.split(token, 1)[1]
                crate_name = after.split("/", 1)[0]
                if crate_name:
                    current_crate = crate_name
                    per_crate.setdefault(
                        current_crate,
                        {
                            "functions": Counter(),
                            "lines": Counter(),
                            "branches": Counter(),
                        },
                    )
            continue

        if current_crate is None:
            continue

        parts = line.split(":", 1)
        if len(parts) != 2:
            continue
        key, value = parts
        try:
            n = int(value)
        except ValueError:
            continue

        bucket = per_crate[current_crate]
        if key == "FNF":
            bucket["functions"].found += n
        elif key == "FNH":
            bucket["functions"].hit += n
        elif key == "LF":
            bucket["lines"].found += n
        elif key == "LH":
            bucket["lines"].hit += n
        elif key == "BRF":
            bucket["branches"].found += n
        elif key == "BRH":
            bucket["branches"].hit += n

    return per_crate


def format_pct(value: float) -> str:
    """Format a percentage with two decimals."""
    return f"{value:.2f}%"


def render_coverage_block(workspace: dict[str, float], per_crate: dict[str, dict[str, Counter]]) -> str:
    """Render Markdown block for README coverage section."""
    crate_rows: list[str] = []
    for crate in sorted(per_crate):
        item = per_crate[crate]
        fn = format_pct(item["functions"].ratio())
        ln = format_pct(item["lines"].ratio())
        br = format_pct(item["branches"].ratio())
        crate_rows.append(f"| `{crate}` | {fn} | {ln} | {br} |")

    if not crate_rows:
        crate_rows.append("| _no crates found in lcov_ | 0.00% | 0.00% | 0.00% |")

    lines = [
        START_MARKER,
        "## Coverage",
        "",
        f"![Coverage](coverage.svg)",
        "",
        "### Workspace",
        "",
        "| Metric | Coverage |",
        "| --- | ---: |",
        f"| Functions | {format_pct(workspace['functions'])} |",
        f"| Lines | {format_pct(workspace['lines'])} |",
        f"| Regions | {format_pct(workspace['regions'])} |",
        f"| Branches | {format_pct(workspace['branches'])} |",
        "",
        "### Per Crate",
        "",
        "| Crate | Functions | Lines | Branches |",
        "| --- | ---: | ---: | ---: |",
        *crate_rows,
        END_MARKER,
    ]
    return "\n".join(lines) + "\n"


def update_readme(readme_path: Path, block: str) -> None:
    """Replace an existing coverage block or append a new one."""
    text = readme_path.read_text(encoding="utf-8")

    if START_MARKER in text and END_MARKER in text:
        start = text.index(START_MARKER)
        end = text.index(END_MARKER) + len(END_MARKER)
        new_text = text[:start] + block + text[end:]
    else:
        sep = "\n\n" if text.endswith("\n") else "\n\n"
        new_text = text + sep + block

    readme_path.write_text(new_text, encoding="utf-8")


def main() -> int:
    """CLI entrypoint."""
    if len(sys.argv) != 4:
        print("usage: update_readme_coverage.py <coverage-summary.json> <README.md> <lcov.info>")
        return 1

    summary_path = Path(sys.argv[1])
    readme_path = Path(sys.argv[2])
    lcov_path = Path(sys.argv[3])

    workspace = load_workspace_summary(summary_path)
    per_crate = parse_lcov_per_crate(lcov_path)
    block = render_coverage_block(workspace, per_crate)
    update_readme(readme_path, block)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
