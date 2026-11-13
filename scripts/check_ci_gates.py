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

# Jobs that run tool-backed tests on a runner holding the tools, per workflow.
STRICT_JOBS = {"ci.yml": ("sandbox", "test"), "coverage.yml": ("coverage",)}

# ci.yml compiles the workspace in exactly one job; every other job runs what it
# built. A workspace compile anywhere else is the waste the layout removed, and
# a `cargo nextest run` without the archive's metadata would quietly compile.
BUILD_JOB = "build"
WORKSPACE_COMPILE = re.compile(r"\bcargo (?:\+\S+ )?(?:build|check|clippy|test|nextest archive)\b")

# The selector that makes every `*_runtime` binary a runtime tier.
TIER_FILTER = "binary(/_runtime$/)"

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


def tier_violations(on_disk, ci_text):
    """Every runtime tier on disk is run: ci.yml selects them all by name pattern."""
    job = job_blocks(ci_text).get("runtime-tiers", "")
    if TIER_FILTER in job and "--run-ignored only" in job:
        return []
    return [f"ci.yml: job `runtime-tiers` no longer runs every `*_runtime` tier "
            f"(`--run-ignored only -E '{TIER_FILTER}'`); {len(on_disk)} tier(s) on disk"]


def compile_violations(jobs):
    """Workspace compiles outside ci.yml's build job, and nextest runs that would compile."""
    found = []
    for job, text in jobs.items():
        joined = text.replace("\\\n", " ")
        for line in joined.splitlines():
            command = line.split("#", 1)[0]
            if re.match(r"\s*(?:- )?name:", command) or re.search(r"\becho\b.*\bcargo\b", command):
                continue
            if job != BUILD_JOB and WORKSPACE_COMPILE.search(command) \
                    and "--manifest-path" not in command:
                found.append(f"ci.yml: job `{job}` compiles the workspace; only `{BUILD_JOB}` may")
            if "cargo nextest run" in command and "$NEXTEST_META" not in command:
                found.append(f"ci.yml: job `{job}` runs nextest without the archive ($NEXTEST_META)")
    return found


def workflow_violations(name, text):
    """Rules on one workflow's text."""
    found = pin_violations(name, text)
    for match in re.finditer(r"^\s+continue-on-error:", text, re.MULTILINE):
        step = step_id_of(text, match.start())
        if (name, step) not in ALLOWED_CONTINUE_ON_ERROR:
            found.append(f"{name}: `continue-on-error` hides step `{step}`")
    jobs = job_blocks(text)
    for job in STRICT_JOBS.get(name, ()):
        if f"{STRICT_ENV}:" not in jobs.get(job, ""):
            found.append(f"{name}: job `{job}` runs tool-backed tests without {STRICT_ENV}")
    if name != "ci.yml":
        return found
    build = jobs.get(BUILD_JOB, "")
    if "check_clippy_ratchet.py" not in build or "cargo clippy --workspace --all-targets" not in build:
        found.append("ci.yml: job `build` no longer runs the clippy ratchet over every target")
    if "cargo test --doc" not in build:
        found.append("ci.yml: job `build` no longer runs the doctests nextest skips")
    test = jobs.get("test", "")
    if "ci_test_shard.py serial" not in test or "ci_test_shard.py filter" not in test:
        found.append("ci.yml: job `test` no longer runs the process-shared binaries it excludes")
    if "cargo fmt --all -- --check" not in jobs.get("ci-gates", ""):
        found.append("ci.yml: job `ci-gates` no longer checks formatting")
    if "scripts/publish-crates.sh" not in jobs.get("package", ""):
        found.append("ci.yml: job `package` no longer dry-runs packaging")
    gate = jobs.get("es-toolkit-regression", "")
    if "runtime-ratchet" not in gate:
        found.append("ci.yml: job `es-toolkit-regression` no longer runs the runtime ratchet")
    if "compile-pin" not in jobs.get("hono", ""):
        found.append("ci.yml: job `hono` no longer pins its test build")
    found += compile_violations(jobs)
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
    with open(os.path.join(root, ".github/workflows/ci.yml"), encoding="utf-8") as ci:
        violations += tier_violations(
            [os.path.basename(path)[:-len(".rs")] for path in tiers], ci.read())
    tests = glob.glob(os.path.join(root, "crates/*/tests/**/*.rs"), recursive=True)
    for path in sorted(tests):
        if "/fixtures/" in path:
            continue
        with open(path, encoding="utf-8") as handle:
            violations += test_source_violations(os.path.relpath(path, root), handle.read())
    if not os.path.islink(os.path.join(root, "CLAUDE.md")):
        violations.append("CLAUDE.md is not a symlink to AGENTS.md, so the two can drift")
    for violation in violations:
        print(violation)
    print(f"{len(violations)} violation(s).")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
