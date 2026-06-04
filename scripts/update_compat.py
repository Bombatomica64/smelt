#!/usr/bin/env python3
"""Update COMPAT.md with one external compatibility run result."""

from __future__ import annotations

import argparse
import datetime as dt
import re
from pathlib import Path


START = "<!-- compat-results:start -->"
END = "<!-- compat-results:end -->"
HEADER = "| Repo | Ref | Status | Failing tests | Result | Updated | Run |\n"
SEPARATOR = "| --- | --- | --- | ---: | --- | --- | --- |\n"


def main() -> None:
    """Parse arguments and update the compatibility results table."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--compat", required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--run-url", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--status", required=True)
    args = parser.parse_args()

    compat_path = Path(args.compat)
    report_path = Path(args.report)
    compat = compat_path.read_text() if compat_path.exists() else default_compat()
    report = report_path.read_text() if report_path.exists() else ""

    row = result_row(
        name=args.name,
        url=args.url,
        rev=args.ref,
        run_url=args.run_url,
        status=args.status,
        report=report,
    )
    compat_path.write_text(update_table(compat, args.name, row))


def default_compat() -> str:
    """Return a new compatibility report document."""
    return (
        "# Compatibility\n\n"
        "This file is updated by the manual `Compatibility` GitHub Actions workflow.\n\n"
        f"{START}\n{HEADER}{SEPARATOR}{END}\n"
    )


def result_row(
    *, name: str, url: str, rev: str, run_url: str, status: str, report: str
) -> str:
    """Build one Markdown table row from a generated Rust test report."""
    normalized_status = compat_status(status, report)
    failing_tests = extract_value(report, r"- Failing tests: `([^`]+)`") or "unknown"
    result = extract_value(report, r"- Result: `([^`]+)`") or "no report"
    updated = dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat()
    short_ref = rev[:12]
    return (
        f"| [{escape_cell(name)}]({url}) | `{short_ref}` | `{escape_cell(normalized_status)}` | "
        f"{escape_cell(failing_tests)} | {escape_cell(result)} | {updated} | [run]({run_url}) |"
    )


def compat_status(step_status: str, report: str) -> str:
    """Return the compatibility status shown in COMPAT.md."""
    if "- Status: `passed`" in report:
        return "passed"
    if "- Status: `failed`" in report:
        return "failed"
    return step_status


def extract_value(text: str, pattern: str) -> str | None:
    """Extract the first regex capture from text."""
    match = re.search(pattern, text)
    return match.group(1) if match else None


def update_table(document: str, name: str, row: str) -> str:
    """Replace or append one repository row inside the compat results block."""
    if START not in document or END not in document:
        document = document.rstrip() + f"\n\n{START}\n{HEADER}{SEPARATOR}{END}\n"

    before, rest = document.split(START, 1)
    table, after = rest.split(END, 1)
    rows = [
        line
        for line in table.splitlines()
        if line.strip()
        and line != HEADER.rstrip()
        and line != SEPARATOR.rstrip()
        and not line.startswith(f"| [{name}](")
    ]
    rows.append(row)
    rows.sort(key=lambda line: line.split("|", 3)[1].casefold())
    block = START + "\n" + HEADER + SEPARATOR + "\n".join(rows) + "\n" + END
    return before.rstrip() + "\n" + block + after


def escape_cell(value: str) -> str:
    """Escape Markdown table separators inside a cell."""
    return value.replace("|", "\\|")


if __name__ == "__main__":
    main()
