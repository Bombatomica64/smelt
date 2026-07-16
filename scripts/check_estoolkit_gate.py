#!/usr/bin/env python3
"""Gate the es-toolkit regression job against a checked-in baseline.

Two modes back the two ratchets in `.github/workflows/ci.yml`:

* ``probe-gate`` — read ``smelt probe --format json`` output and fail (exit 1)
  when the number of source files with at least one blocker rises above
  ``baseline.probe_blockers``. On failure it names the largest remaining
  blocker classes (families) and, when a ``smelt rust-diagnostics`` report is
  supplied via ``--diagnostics``, folds its top diagnostic families into the
  message. All output is echoed to stdout and appended to ``$GITHUB_STEP_SUMMARY``.

* ``runtime-ratchet`` — parse libtest ``test result:`` lines from a captured
  ``cargo test`` log, sum passed/failed across every test binary, and compare
  to ``baseline.runtime_passed`` / ``baseline.runtime_failed``. This mode is
  advisory by default (exit 0 regardless) because known async hangs make the
  suite non-deterministic; pass ``--blocking`` to make a regression fail.

The comparisons are ``<=`` / ``>=`` so improvements pass without a baseline
bump; loosening the baseline is a deliberate, PR-reviewed edit.
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

    limit = int(baseline["probe_blockers"])
    actual = int(probe.get("files_with_blockers", 0))

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


def _parse_runtime(log_path):
    """Sum passed/failed across all libtest summary lines in a captured log."""
    passed = failed = 0
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            match = _TEST_RESULT_RE.search(line)
            if match:
                passed += int(match.group(1))
                failed += int(match.group(2))
    return passed, failed


def runtime_ratchet(args):
    """Advisory (or blocking) runtime pass-count ratchet against the baseline."""
    baseline = _load_baseline(args.baseline)
    passed, failed = _parse_runtime(args.log)

    base_passed = int(baseline["runtime_passed"])
    base_failed = int(baseline["runtime_failed"])
    gate = baseline.get("runtime_gate", "advisory")
    blocking = args.blocking and gate != "advisory"

    _emit("## es-toolkit runtime ratchet"
          + ("" if blocking else " (advisory)"))
    _emit("")
    _emit(f"Runtime: **{passed} passed / {failed} failed** "
          f"(baseline {base_passed} passed / {base_failed} failed).")

    regressed = passed < base_passed
    if regressed:
        _emit("")
        _emit(
            f"es-toolkit runtime: pass count regressed {base_passed} -> {passed}."
        )
    elif passed > base_passed:
        _emit(f"Improvement: {passed - base_passed} more passing test(s).")

    if regressed and blocking:
        _emit("Runtime ratchet: **FAIL**.")
        return 1
    _emit("Runtime ratchet: **{}**.".format(
        "PASS" if not regressed else "advisory (not blocking)"))
    return 0


def _selftest():
    """Fabricated-fixture self-test; run with `--selftest`. Returns exit code."""
    import tempfile

    ok = True
    with tempfile.TemporaryDirectory() as tmp:
        base = os.path.join(tmp, "baseline.json")
        with open(base, "w", encoding="utf-8") as handle:
            json.dump({
                "probe_blockers": 9,
                "runtime_passed": 56,
                "runtime_failed": 89,
                "runtime_gate": "advisory",
            }, handle)

        # Probe within baseline -> pass.
        probe_ok = os.path.join(tmp, "probe_ok.json")
        with open(probe_ok, "w", encoding="utf-8") as handle:
            json.dump({"files_with_blockers": 9, "blockers": []}, handle)
        rc = probe_gate(argparse.Namespace(
            baseline=base, probe=probe_ok, diagnostics=None))
        ok &= (rc == 0)

        # Probe regressed -> fail, names families.
        probe_bad = os.path.join(tmp, "probe_bad.json")
        with open(probe_bad, "w", encoding="utf-8") as handle:
            json.dump({
                "files_with_blockers": 12,
                "blockers": [{
                    "category": "lowering", "code": "E-LOWER",
                    "shape": "unsupported const item expression",
                    "occurrences": 4, "files": 3,
                    "example_file": "src/predicate/isEqualWith.spec.ts",
                }],
            }, handle)
        rc = probe_gate(argparse.Namespace(
            baseline=base, probe=probe_bad, diagnostics=None))
        ok &= (rc == 1)

        # Runtime advisory regression -> exit 0 despite fewer passes.
        log = os.path.join(tmp, "test.log")
        with open(log, "w", encoding="utf-8") as handle:
            handle.write("test result: FAILED. 40 passed; 5 failed; 0 ignored\n")
            handle.write("test result: ok. 10 passed; 2 failed; 0 ignored\n")
        rc = runtime_ratchet(argparse.Namespace(
            baseline=base, log=log, blocking=False))
        ok &= (rc == 0)

        # Parsing sums across binaries.
        passed, failed = _parse_runtime(log)
        ok &= (passed == 50 and failed == 7)

    print("SELFTEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv=None):
    """CLI entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selftest", action="store_true",
                        help="Run the fabricated-fixture self-test and exit.")
    sub = parser.add_subparsers(dest="mode")

    p_probe = sub.add_parser("probe-gate", help="Gate probe blocker count.")
    p_probe.add_argument("--baseline", required=True)
    p_probe.add_argument("--probe", required=True,
                         help="probe JSON from `smelt probe --format json`.")
    p_probe.add_argument("--diagnostics", default=None,
                         help="Optional rust-diagnostics Markdown report.")
    p_probe.set_defaults(func=probe_gate)

    p_run = sub.add_parser("runtime-ratchet", help="Advisory runtime ratchet.")
    p_run.add_argument("--baseline", required=True)
    p_run.add_argument("--log", required=True,
                       help="Captured `cargo test` output.")
    p_run.add_argument("--blocking", action="store_true",
                       help="Fail on regression (only if baseline gate!=advisory).")
    p_run.set_defaults(func=runtime_ratchet)

    args = parser.parse_args(argv)
    if args.selftest:
        return _selftest()
    if not getattr(args, "func", None):
        parser.print_help()
        return 2
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
