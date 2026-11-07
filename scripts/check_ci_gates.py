#!/usr/bin/env python3
"""Check that the CI workflows and test helpers still have teeth.

Static rules over the repository; each one names a way a gate has gone quiet
here before. Exits 1 and lists every violation.
"""

import glob
import os
import re
import sys

# The only `continue-on-error` allowed. The compat report records the step's
# outcome as data in COMPAT.md, so its failure is read rather than hidden.
ALLOWED_CONTINUE_ON_ERROR = {("compat.yml", "generated-tests")}

STRICT_ENV = "SMELT_REQUIRE_SANDBOX"

# Jobs that run tool-backed tests on a runner holding the tools.
STRICT_JOBS = ("sandbox", "test", "coverage")

# Versions and commits that belong in .github/compat/libraries.json.
LITERAL_PINS = (
    re.compile(r"typescript@\d[\w.]*"),
    re.compile(r"node-version: ['\"]?\d[\w.]*"),
    re.compile(r"\b[0-9a-f]{40}\b"),
)

# A probe for a tool followed by a bare return: a skip nobody can see or forbid.
SILENT_SKIP = re.compile(
    r"(?:discovered_\w+\(\)|_environment\(\w+\)\?|\.availability\(\)[^{;]*"
    r"|\.is_file\(\)|\.is_err\(\)|node_runs_typescript\(\)|python_runs_corpus\(\)"
    r"|npm_available\(\))"
    r"[^{;]*(?:else\s*)?\{\s*return(?: Ok\(\(\)\))?;\s*\}"
)


def job_blocks(text):
    """Split a workflow into {job id: text}, by two-space-indented job keys."""
    start = text.find("\njobs:\n")
    if start < 0:
        return {}
    body = text[start:]
    matches = list(re.finditer(r"^  ([A-Za-z0-9_-]+):\n", body, re.MULTILINE))
    blocks = {}
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        blocks[match.group(1)] = body[match.start():end]
    return blocks


def step_id_of(text, offset):
    """The `id:` of the workflow step holding `offset`, or its `name:`, or ''."""
    head = text[:offset]
    start = head.rfind("\n      - ")
    step = text[start:text.find("\n      - ", offset) if "\n      - " in text[offset:] else len(text)]
    match = re.search(r"^\s+id: (\S+)", step, re.MULTILINE) or re.search(
        r"name: (.+)$", step, re.MULTILINE)
    return match.group(1) if match else ""


def pin_violations(name, text):
    """A version or commit spelled in a workflow instead of libraries.json."""
    found = []
    for pattern in LITERAL_PINS:
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            found.append(f"{name}:{line}: `{match.group(0)}` belongs in libraries.json")
    if "dtolnay/rust-toolchain@stable" in text:
        found.append(f"{name}: installs `stable`; use ./.github/actions/setup-rust")
    return found


def tier_violations(on_disk, matrix_text, ci_text):
    """Every runtime tier on disk is run exactly once, and every name exists."""
    jobs = matrix_text.split("\njobs:\n", 1)[-1]
    uncommented = "\n".join(line for line in jobs.splitlines() if not line.lstrip().startswith("#"))
    matrix = re.findall(r"\b([a-z0-9_]+_runtime)\b", uncommented)
    named = matrix + re.findall(r"--test ([a-z0-9_]+_runtime)\b", ci_text)
    found = [f"runtime tier `{tier}` is run by no job" for tier in sorted(set(on_disk) - set(named))]
    found += [f"runtime tier `{tier}` has no test file" for tier in sorted(set(named) - set(on_disk))]
    found += [f"runtime tier `{tier}` is named twice" for tier in sorted({
        tier for tier in named if named.count(tier) > 1})]
    return found


def workflow_violations(name, text):
    """Rules on one workflow's text."""
    found = pin_violations(name, text)
    for match in re.finditer(r"^\s+continue-on-error:", text, re.MULTILINE):
        step = step_id_of(text, match.start())
        if (name, step) not in ALLOWED_CONTINUE_ON_ERROR:
            found.append(f"{name}: `continue-on-error` hides step `{step}`")
    if name != "ci.yml":
        return found
    jobs = job_blocks(text)
    for job in STRICT_JOBS:
        if f"{STRICT_ENV}:" not in jobs.get(job, ""):
            found.append(f"ci.yml: job `{job}` runs tool-backed tests without {STRICT_ENV}")
    clippy = jobs.get("clippy", "")
    if "check_clippy_ratchet.py" not in clippy or "cargo clippy --workspace --all-targets" not in clippy:
        found.append("ci.yml: job `clippy` no longer runs the clippy ratchet over every target")
    if "scripts/publish-crates.sh" not in jobs.get("package", ""):
        found.append("ci.yml: job `package` no longer dry-runs packaging")
    gate = jobs.get("es-toolkit-regression", "")
    if "runtime-ratchet" not in gate:
        found.append("ci.yml: job `es-toolkit-regression` no longer runs the runtime ratchet")
    if "compile-pin" not in jobs.get("hono", ""):
        found.append("ci.yml: job `hono` no longer pins its test build")
    return found


def test_source_violations(path, text):
    """Rules on one Rust test source."""
    found = []
    for match in SILENT_SKIP.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        found.append(f"{path}:{line}: a tool probe returns without `missing_prerequisite`")
    return found


def main():
    """Check the repository the script lives in; return the exit status."""
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    violations = []
    for path in sorted(glob.glob(os.path.join(root, ".github/workflows/*.yml"))):
        with open(path, encoding="utf-8") as handle:
            violations += workflow_violations(os.path.basename(path), handle.read())
    for path in sorted(glob.glob(os.path.join(root, ".github/actions/*/action.yml"))):
        with open(path, encoding="utf-8") as handle:
            violations += pin_violations(os.path.relpath(path, root), handle.read())
    tiers = glob.glob(os.path.join(root, "crates/smelt-codegen-rust/tests/*_runtime.rs"))
    with open(os.path.join(root, ".github/workflows/runtime-tiers.yml"), encoding="utf-8") as matrix, \
            open(os.path.join(root, ".github/workflows/ci.yml"), encoding="utf-8") as ci:
        violations += tier_violations(
            [os.path.basename(path)[:-len(".rs")] for path in tiers], matrix.read(), ci.read())
    tests = glob.glob(os.path.join(root, "crates/*/tests/**/*.rs"), recursive=True)
    for path in sorted(tests):
        if "/fixtures/" in path:
            continue
        with open(path, encoding="utf-8") as handle:
            violations += test_source_violations(os.path.relpath(path, root), handle.read())
    for violation in violations:
        print(violation)
    print(f"{len(violations)} violation(s).")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
