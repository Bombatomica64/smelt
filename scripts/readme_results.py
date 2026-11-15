#!/usr/bin/env python3
"""Render the README's results tables from the files CI actually judges.

The README's "Status" section quotes three sets of numbers: the generated test
suites of real libraries, the typedness baselines and the SmeltUnknown
baselines. Hand-copied, they went stale (ts-pattern and date-fns were quoted
from pins CI had long moved past). This script renders each table from its
source of truth and writes it between ``<!-- results:NAME:begin -->`` and
``<!-- results:NAME:end -->`` markers:

* ``suites`` — the exact ``grep -F "test result: ..."`` pins in
  ``.github/workflows/ci.yml`` (remeda, radash, ts-pattern, date-fns) and the
  runtime-ratchet baselines (``blocker-logs/{es-toolkit,hono}-ci-baseline.json``).
* ``typedness`` — the ``total`` of each ``blocker-logs/typedness-baseline*.json``.
* ``unknown`` — ``files_scanned`` and ``category_totals`` of each
  ``blocker-logs/smelt-unknown-baseline*.json``.

``--write`` rewrites README.md in place; ``--check`` (and ``check_ci_gates.py``,
through :func:`stale_tables`) fails when the README differs from the render, so
a pin or baseline bump that forgets the README fails CI.
"""

import argparse
import json
import os
import re
import sys

README = "README.md"
CI = ".github/workflows/ci.yml"

# The exact-line library pins: `grep -F "test result: ok. N passed; M failed;" LIB-tests.log`.
PIN_RE = re.compile(
    r'grep -F "test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;" ([\w-]+)-tests\.log'
)

# Library (log prefix) -> the ci.yml job that pins it, in table order.
PINNED_LIBRARIES = (
    ("remeda", "remeda-regression"),
    ("radash", "radash-regression"),
    ("ts-pattern", "ts-pattern-build"),
    ("date-fns", "date-fns-build"),
)

# Runtime-ratchet libraries: (name, baseline file, ci.yml job).
RATCHETED_LIBRARIES = (
    ("es-toolkit", "blocker-logs/es-toolkit-ci-baseline.json", "es-toolkit-regression"),
    ("hono", "blocker-logs/hono-ci-baseline.json", "hono"),
)

# (corpus label, typedness baseline file).
TYPEDNESS_BASELINES = (
    ("examples", "blocker-logs/typedness-baseline.json"),
    ("es-toolkit", "blocker-logs/typedness-baseline-es-toolkit.json"),
    ("remeda", "blocker-logs/typedness-baseline-remeda.json"),
)

# (corpus label, SmeltUnknown baseline file, gate wording).
UNKNOWN_BASELINES = (
    ("examples", "blocker-logs/smelt-unknown-baseline.json", "blocking, must stay 0"),
    ("es-toolkit", "blocker-logs/smelt-unknown-baseline-es-toolkit.json", "blocking, may only fall"),
    ("remeda", "blocker-logs/smelt-unknown-baseline-remeda.json", "advisory"),
    ("hono", "blocker-logs/smelt-unknown-baseline-hono.json", "advisory"),
)

KINDS = ("values", "reads", "calls")


def _read(root, path):
    """Read a repository file as text."""
    with open(os.path.join(root, path), encoding="utf-8") as handle:
        return handle.read()


def _json(root, path):
    """Read a repository JSON file."""
    return json.loads(_read(root, path))


def _field(document, field, source):
    """`document[field]`; a renamed or missing field stops the render."""
    if field not in document:
        raise SystemExit(f"{source} has no `{field}` field; cannot render the README table.")
    return document[field]


def ci_pins(ci_text):
    """Map each pinned library's log prefix to its exact (passed, failed) pin."""
    pins = {}
    for match in PIN_RE.finditer(ci_text):
        library = match.group(3)
        if library in pins:
            raise SystemExit(f"{CI} pins `{library}` twice; cannot tell which one judges.")
        pins[library] = (int(match.group(1)), int(match.group(2)))
    return pins


def suites_table(root):
    """The generated-test-suites table."""
    pins = ci_pins(_read(root, CI))
    rows = [
        "| Library | Result | Gate | Source |",
        "| --- | --- | --- | --- |",
    ]
    ordered = []
    for library, job in PINNED_LIBRARIES:
        if library not in pins:
            raise SystemExit(f"{CI} no longer pins `{library}` with `grep -F \"test result: ...\"`.")
        passed, failed = pins[library]
        ordered.append((library, f"exactly {passed} passed, {failed} failed (pinned)",
                        "blocking pin", f"`{CI}` job `{job}`"))
    for library, path, job in RATCHETED_LIBRARIES:
        base = _json(root, path)
        passed = int(_field(base, "runtime_passed", path))
        known = len(_field(base, "runtime_known_failures", path))
        total = int(_field(base, "runtime_total", path))
        result = (f"{total} tests; at least {passed} passed; "
                  f"{known} named tests allowed to fail")
        if "test_compile_errors" in base:
            result += f"; test target builds with {int(base['test_compile_errors'])} rustc errors"
        ordered.append((library, result, "blocking ratchet",
                        f"[`{path}`]({path}), `{CI}` job `{job}`"))
    order = ("remeda", "es-toolkit", "hono", "radash", "ts-pattern", "date-fns")
    ordered.sort(key=lambda row: order.index(row[0]) if row[0] in order else len(order))
    rows += [f"| {name} | {result} | {gate} | {source} |" for name, result, gate, source in ordered]
    return "\n".join(rows)


def _share(part, whole):
    """`part / whole` as a one-decimal percentage."""
    return f"{100 * part / whole:.1f}%" if whole else "n/a"


def typedness_table(root):
    """The typedness table: one row per corpus and kind."""
    rows = [
        "| Corpus (baseline file) | Kind | Concrete | Generic | Erased | Erased share |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for label, path in TYPEDNESS_BASELINES:
        base = _json(root, path)
        total = _field(base, "total", path)
        programs = len(_field(base, "programs", path))
        for index, kind in enumerate(KINDS):
            counts = _field(total, kind, path)
            concrete, generic, erased = (int(_field(counts, key, path))
                                         for key in ("concrete", "generic", "erased"))
            corpus = (f"{label}, {programs} programs ([`{os.path.basename(path)}`]({path}))"
                      if index == 0 else "")
            rows.append(f"| {corpus} | {kind} | {concrete} | {generic} | {erased} | "
                        f"{_share(erased, concrete + generic + erased)} |")
    return "\n".join(rows)


def unknown_table(root):
    """The SmeltUnknown table: one row per corpus."""
    rows = [
        "| Corpus (baseline file) | Files | Runtime prelude | Legitimate boundary | Avoidable erasure | Gate |",
        "| --- | ---: | ---: | ---: | ---: | --- |",
    ]
    for label, path, gate in UNKNOWN_BASELINES:
        base = _json(root, path)
        files = int(_field(base, "files_scanned", path))
        totals = _field(base, "category_totals", path)
        prelude, boundary, avoidable = (
            int(totals.get(key, 0))
            for key in ("runtime-prelude", "legitimate-boundary", "avoidable-erasure"))
        rows.append(f"| {label} ([`{os.path.basename(path)}`]({path})) | {files} | "
                    f"{prelude} | {boundary} | {avoidable} | {gate} |")
    return "\n".join(rows)


TABLES = {
    "suites": suites_table,
    "typedness": typedness_table,
    "unknown": unknown_table,
}


def _region(name):
    """The regex matching table `name`'s marked region, markers included."""
    return re.compile(
        rf"(<!-- results:{name}:begin -->\n)(.*?)(<!-- results:{name}:end -->)", re.DOTALL)


def render(root, readme_text):
    """`readme_text` with every marked table re-rendered; a missing marker stops."""
    for name, build in TABLES.items():
        region = _region(name)
        if not region.search(readme_text):
            raise SystemExit(f"{README} has no `<!-- results:{name}:begin/end -->` region.")
        table = build(root)
        readme_text = region.sub(lambda match, t=table: match.group(1) + t + "\n" + match.group(3),
                                 readme_text, count=1)
    return readme_text


def stale_tables(root):
    """Names of the README tables whose text differs from the render."""
    text = _read(root, README)
    rendered = render(root, text)
    return [name for name in TABLES
            if _region(name).search(text).group(2) != _region(name).search(rendered).group(2)]


def main(argv=None):
    """CLI entry point: `--check` (default) or `--write`."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="rewrite README.md in place")
    args = parser.parse_args(argv)
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    if args.write:
        text = _read(root, README)
        rendered = render(root, text)
        if rendered != text:
            with open(os.path.join(root, README), "w", encoding="utf-8") as handle:
                handle.write(rendered)
        return 0
    stale = stale_tables(root)
    for name in stale:
        print(f"{README}: the `{name}` table is stale; run `python3 scripts/readme_results.py --write`")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
