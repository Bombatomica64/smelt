#!/usr/bin/env python3
"""Gate the library regression jobs against checked-in baselines.

Three modes back the ratchets in `.github/workflows/ci.yml`. Each one blocks:
a mode that cannot read what it judges fails rather than passing.

* ``probe-gate`` — read ``smelt probe --format json`` output and fail (exit 1)
  when the number of source files with at least one blocker rises above
  ``baseline.probe_blockers``. On failure it names the largest remaining
  blocker classes (families) and, when a ``smelt rust-diagnostics`` report is
  supplied via ``--diagnostics``, folds its top diagnostic families into the
  message. All output is echoed to stdout and appended to ``$GITHUB_STEP_SUMMARY``.

* ``runtime-ratchet`` — parse a captured ``cargo test`` log. Every failing test
  must be pinned in ``baseline.runtime_known_failures`` and the passed count may
  not fall below ``baseline.runtime_passed``, so a new failure fails even when
  another test was fixed, and a killed or uncompiled suite fails on its count.

* ``compile-pin`` — count the rustc errors in a captured ``cargo test`` log and
  fail unless it equals ``baseline.test_compile_errors``. The pin is exact: a
  suite that does not build runs nothing, so a fall is a change to act on, the
  next pin being its test results.

Improvements of a ratchet pass without a baseline bump; loosening a baseline is
a deliberate, PR-reviewed edit.
"""

import argparse
import json
import os
import re
import sys


def _load_baseline(path):
    """Load the checked-in baseline JSON."""
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def _required(document, field, source):
    """Read `field` of `document`; a renamed or missing field stops the gate."""
    if field not in document:
        raise SystemExit(f"{source} has no `{field}` field; refusing to judge without it.")
    return document[field]


def _emit(text):
    """Print to stdout and append to the GitHub step summary when available."""
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(text + "\n")


def _top_families(blockers, limit=5):
    """Render the largest probe blocker classes as a human-readable list.

    ``blockers`` is the ``blockers`` array from probe JSON: each entry has
    ``category``, ``code``, ``shape``, ``occurrences``, ``files`` and
    ``example_file``. Sorted most-frequent-first by the probe already.
    """
    lines = []
    for group in blockers[:limit]:
        code = group.get("code") or group.get("category", "?")
        shape = group.get("shape", "").strip()
        occ = group.get("occurrences", 0)
        files = group.get("files", 0)
        example = group.get("example_file", "")
        lines.append(
            f"- {code} {shape} — {occ} occurrence(s) across {files} file(s); "
            f"e.g. {example}"
        )
    return lines


def _diagnostic_families(diag_path, limit=5):
    """Pull the top diagnostic-family lines out of a rust-diagnostics report.

    The report groups by diagnostic, sorted by count desc, and renders a
    Markdown table whose rows begin with a leading pipe. We surface the first
    few data rows verbatim so the summary names concrete families (E0308, ...).
    """
    if not diag_path or not os.path.isfile(diag_path):
        return []
    rows = []
    with open(diag_path, encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            # Table data rows start with "|" and are not the header/separator.
            if stripped.startswith("|") and not re.match(r"^\|[\s|:-]+\|$", stripped):
                cells = [c.strip() for c in stripped.strip("|").split("|")]
                if cells and cells[0].lower() in ("count", "occurrences", ""):
                    continue
                rows.append("- " + " · ".join(c for c in cells if c))
            if len(rows) >= limit:
                break
    return rows


def probe_gate(args):
    """Compare probe blocker count to the baseline; fail on regression."""
    baseline = _load_baseline(args.baseline)
    with open(args.probe, encoding="utf-8") as handle:
        probe = json.load(handle)

    limit = int(_required(baseline, "probe_blockers", args.baseline))
    actual = int(_required(probe, "files_with_blockers", args.probe))

    _emit("## es-toolkit probe gate")
    _emit("")
    _emit(f"Files with blockers: **{actual}** (baseline {limit}).")

    if actual <= limit:
        if actual < limit:
            _emit(
                f"Improvement: {limit - actual} fewer file(s) with blockers than "
                "baseline. Consider ratcheting the baseline down."
            )
        _emit("Probe gate: **PASS**.")
        return 0

    _emit("")
    _emit(
        f"es-toolkit gate: probe blockers regressed {limit} -> {actual} "
        f"files with blockers."
    )
    families = _top_families(probe.get("blockers", []))
    if families:
        _emit("")
        _emit("Largest remaining blocker classes:")
        _emit("")
        for line in families:
            _emit(line)
    diag = _diagnostic_families(args.diagnostics)
    if diag:
        _emit("")
        _emit("Top Rust diagnostic families (see gate-diagnostics.md artifact):")
        _emit("")
        for line in diag:
            _emit(line)
    _emit("")
    _emit("Probe gate: **FAIL**.")
    return 1


# libtest summary line: "test result: ok. 12 passed; 3 failed; 0 ignored; ..."
_TEST_RESULT_RE = re.compile(
    r"test result:\s+\w+\.\s+(\d+)\s+passed;\s+(\d+)\s+failed", re.IGNORECASE
)

# libtest per-test line: "test module::name ... FAILED"
_TEST_FAILED_RE = re.compile(r"^test (\S+)(?: - .*)? \.\.\. FAILED\s*$")

# rustc diagnostic head: "error[E0308]: ..." or "error: ...". The closing
# "could not compile" and "aborting due to" lines count nothing new.
# Cargo's own summaries are not compiler errors: `could not compile`, rustc's
# `aborting due to`, and, once a test target builds and runs, `test failed, to
# rerun ...` and `N target(s) failed:`.
_RUSTC_ERROR_RE = re.compile(
    r"^error(\[E\d+\])?: (?!could not compile|aborting due to|test failed|\d+ targets? failed)"
)


# Terminal colour: the CI jobs set CARGO_TERM_COLOR=always, so a captured log
# can carry escapes inside the text the patterns above match.
_ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


def _plain_lines(log_path):
    """Yield the lines of a captured log with terminal colour removed."""
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            yield _ANSI_RE.sub("", line)


def _parse_runtime(log_path):
    """Return (passed, failed, failing names, whether any summary line exists)."""
    passed = failed = 0
    failing = set()
    summarized = False
    for line in _plain_lines(log_path):
        match = _TEST_RESULT_RE.search(line)
        if match:
            summarized = True
            passed += int(match.group(1))
            failed += int(match.group(2))
        match = _TEST_FAILED_RE.match(line)
        if match:
            failing.add(match.group(1))
    return passed, failed, failing, summarized


def runtime_ratchet(args):
    """Fail on an unpinned failing test or a fall in the passed count."""
    baseline = _load_baseline(args.baseline)
    base_passed = int(_required(baseline, "runtime_passed", args.baseline))
    known = set(_required(baseline, "runtime_known_failures", args.baseline))
    passed, failed, failing, summarized = _parse_runtime(args.log)

    _emit("## Runtime ratchet")
    _emit("")
    if not summarized:
        _emit(f"No `test result:` line in {args.log}: the suite did not finish.")
        _emit("Runtime ratchet: **FAIL**.")
        return 1

    _emit(f"Runtime: **{passed} passed / {failed} failed** "
          f"(floor {base_passed} passed, {len(known)} pinned failures).")
    new = sorted(failing - known)
    if len(failing) != failed:
        _emit(f"The log names {len(failing)} failing test(s) but its summaries "
              f"count {failed}.")
    for name in new:
        _emit(f"- new failure: `{name}`")
    fixed = sorted(known - failing)
    for name in fixed:
        _emit(f"- pinned failure now passes, remove it from the baseline: `{name}`")
    if passed < base_passed:
        _emit(f"Pass count regressed {base_passed} -> {passed}.")
    elif passed > base_passed:
        _emit(f"Improvement: {passed - base_passed} more passing test(s); "
              "raise `runtime_passed`.")

    if new or passed < base_passed or len(failing) != failed:
        _emit("Runtime ratchet: **FAIL**.")
        return 1
    _emit("Runtime ratchet: **PASS**.")
    return 0


def compile_pin(args):
    """Fail unless the rustc error count of a test build equals the pin."""
    baseline = _load_baseline(args.baseline)
    pinned = int(_required(baseline, "test_compile_errors", args.baseline))
    errors = sum(1 for line in _plain_lines(args.log) if _RUSTC_ERROR_RE.match(line))

    _emit("## Test build pin")
    _emit("")
    _emit(f"Rustc errors building the tests: **{errors}** (pinned {pinned}).")
    if errors == pinned:
        _emit("Test build pin: **PASS**.")
        return 0
    if errors < pinned:
        _emit("Fewer errors than pinned: lower `test_compile_errors`. At zero the "
              "tests run, so pin their results then.")
    _emit("Test build pin: **FAIL**.")
    return 1


def main(argv=None):
    """CLI entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode")

    p_probe = sub.add_parser("probe-gate", help="Gate probe blocker count.")
    p_probe.add_argument("--baseline", required=True)
    p_probe.add_argument("--probe", required=True,
                         help="probe JSON from `smelt probe --format json`.")
    p_probe.add_argument("--diagnostics", default=None,
                         help="Optional rust-diagnostics Markdown report.")
    p_probe.set_defaults(func=probe_gate)

    p_run = sub.add_parser("runtime-ratchet", help="Pinned-failure runtime ratchet.")
    p_run.add_argument("--baseline", required=True)
    p_run.add_argument("--log", required=True,
                       help="Captured `cargo test` output.")
    p_run.set_defaults(func=runtime_ratchet)

    p_pin = sub.add_parser("compile-pin", help="Exact pin of a test build's errors.")
    p_pin.add_argument("--baseline", required=True)
    p_pin.add_argument("--log", required=True,
                       help="Captured `cargo test` output.")
    p_pin.set_defaults(func=compile_pin)

    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 2
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
