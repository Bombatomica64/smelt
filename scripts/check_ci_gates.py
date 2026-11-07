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


def workflow_violations(name, text):
    """Rules on one workflow's text."""
    found = []
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
