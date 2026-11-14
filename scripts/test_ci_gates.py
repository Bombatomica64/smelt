#!/usr/bin/env python3
"""Unit tests for the CI gate scripts. Run: python3 -m unittest discover -s scripts"""

import argparse
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import check_ci_gates
import ci_test_shard
import ci_pin
import publish_order
import check_clippy_ratchet
import check_library_gate


def write(directory, name, text):
    """Write a fixture file and return its path."""
    path = os.path.join(directory, name)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    return path


def quietly(function, *args):
    """Call `function` with stdout swallowed."""
    with contextlib.redirect_stdout(io.StringIO()):
        return function(*args)


class LibraryGate(unittest.TestCase):
    """`check_library_gate.py`."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.dir = directory.name
        self.baseline = write(self.dir, "base.json", json.dumps({
            "probe_blockers": 2,
            "runtime_passed": 3,
            "runtime_known_failures": ["m::known", "m::also_known"],
            "test_compile_errors": 2,
        }))

    def probe(self, document):
        """Run the probe gate on `document`."""
        path = write(self.dir, "probe.json", json.dumps(document))
        return quietly(check_library_gate.probe_gate, argparse.Namespace(
            baseline=self.baseline, probe=path, diagnostics=None))

    def runtime(self, log):
        """Run the runtime ratchet on a log."""
        path = write(self.dir, "test.log", log)
        return quietly(check_library_gate.runtime_ratchet, argparse.Namespace(
            baseline=self.baseline, log=path))

    def pin(self, log):
        """Run the compile pin on a log."""
        path = write(self.dir, "build.log", log)
        return quietly(check_library_gate.compile_pin, argparse.Namespace(
            baseline=self.baseline, log=path))

    def test_probe_within_and_above_the_baseline(self):
        self.assertEqual(self.probe({"files_with_blockers": 2, "blockers": []}), 0)
        self.assertEqual(self.probe({"files_with_blockers": 3, "blockers": []}), 1)

    def test_probe_stops_on_a_renamed_field(self):
        with self.assertRaises(SystemExit) as caught:
            self.probe({"blocked_files": 0, "blockers": []})
        self.assertIn("files_with_blockers", str(caught.exception))

    def test_runtime_passes_with_only_pinned_failures(self):
        log = ("test m::known ... FAILED\ntest m::also_known ... FAILED\n"
               "test result: FAILED. 3 passed; 2 failed; 0 ignored\n")
        self.assertEqual(self.runtime(log), 0)

    def test_runtime_fails_on_a_new_failure_even_with_a_fix(self):
        log = ("test m::known ... FAILED\ntest m::new ... FAILED\n"
               "test result: FAILED. 4 passed; 2 failed; 0 ignored\n")
        self.assertEqual(self.runtime(log), 1)

    def test_runtime_fails_on_a_lower_pass_count(self):
        log = "test m::known ... FAILED\ntest result: FAILED. 2 passed; 1 failed; 0 ignored\n"
        self.assertEqual(self.runtime(log), 1)

    def test_runtime_fails_on_a_suite_that_did_not_finish(self):
        self.assertEqual(self.runtime("test m::fine ... ok\n"), 1)

    def test_runtime_fails_when_summaries_count_a_failure_the_log_does_not_name(self):
        self.assertEqual(self.runtime("test result: FAILED. 3 passed; 1 failed; 0 ignored\n"), 1)

    def test_runtime_reads_coloured_logs(self):
        log = ("test m::new ... \x1b[31mFAILED\x1b[0m\n"
               "test result: \x1b[31mFAILED\x1b[0m. 3 passed; 1 failed; 0 ignored\n")
        self.assertEqual(self.runtime(log), 1)

    def test_runtime_stops_on_a_baseline_without_its_fields(self):
        self.baseline = write(self.dir, "old.json", json.dumps({"runtime_passed": 3}))
        with self.assertRaises(SystemExit):
            self.runtime("test result: ok. 3 passed; 0 failed; 0 ignored\n")

    def test_compile_pin_is_exact(self):
        two = ("\x1b[1m\x1b[31merror[E0308]\x1b[0m: mismatched\nerror: bad\n"
               "error: could not compile `x` due to 2 previous errors\n")
        self.assertEqual(self.pin(two), 0)
        self.assertEqual(self.pin("error[E0308]: mismatched\n"), 1)
        self.assertEqual(self.pin(two + "error[E0599]: no method\n"), 1)


def message(crate, code, level="warning", line=1, file_name="src/lib.rs"):
    """A cargo `compiler-message` line."""
    return json.dumps({
        "reason": "compiler-message",
        "manifest_path": f"/repo/crates/{crate}/Cargo.toml",
        "message": {
            "level": level,
            "message": "text",
            "rendered": f"{level}: {code}",
            "code": {"code": code},
            "spans": [{"is_primary": True, "file_name": file_name,
                       "line_start": line, "column_start": 1}],
        },
    })


ARTIFACT = json.dumps({"reason": "compiler-artifact"})


class ClippyRatchet(unittest.TestCase):
    """`check_clippy_ratchet.py`."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.dir = directory.name
        self.baseline = os.path.join(self.dir, "baseline.json")
        check_clippy_ratchet.write_baseline(self.baseline, {"a": {"clippy::x": 2}})

    def run_gate(self, lines, status=0, *extra):
        """Run the gate over a message stream."""
        path = write(self.dir, "clippy.json", "\n".join(lines) + "\n")
        return quietly(check_clippy_ratchet.main, [
            "--baseline", self.baseline, "--messages", path,
            "--cargo-status", str(status), *extra])

    def test_equal_counts_pass(self):
        lines = [ARTIFACT, message("a", "clippy::x", line=1), message("a", "clippy::x", line=2)]
        self.assertEqual(self.run_gate(lines), 0)

    def test_a_site_seen_from_two_targets_counts_once(self):
        site = message("a", "clippy::x", line=1)
        self.assertEqual(self.run_gate([ARTIFACT, site, site, site]), 0)

    def test_a_rise_fails(self):
        lines = [ARTIFACT] + [message("a", "clippy::x", line=n) for n in (1, 2, 3)]
        self.assertEqual(self.run_gate(lines), 1)

    def test_a_lint_new_to_a_crate_fails(self):
        self.assertEqual(self.run_gate([ARTIFACT, message("b", "clippy::x")]), 1)

    def test_an_error_fails(self):
        self.assertEqual(self.run_gate([ARTIFACT, message("a", "clippy::y", level="error")]), 1)

    def test_a_nonzero_cargo_exit_fails(self):
        self.assertEqual(self.run_gate([ARTIFACT], status=101), 1)

    def test_a_run_that_compiled_nothing_fails(self):
        self.assertEqual(self.run_gate(["not json"], status=0), 1)

    def test_update_takes_a_fall_and_refuses_a_rise(self):
        self.assertEqual(self.run_gate([ARTIFACT, message("a", "clippy::x")], 0, "--update"), 0)
        with open(self.baseline, encoding="utf-8") as handle:
            self.assertEqual(json.load(handle)["counts"], {"a": {"clippy::x": 1}})
        lines = [ARTIFACT] + [message("a", "clippy::x", line=n) for n in (1, 2)]
        self.assertEqual(self.run_gate(lines, 0, "--update"), 1)
        with open(self.baseline, encoding="utf-8") as handle:
            self.assertEqual(json.load(handle)["counts"], {"a": {"clippy::x": 1}})


GOOD_CI = """on: push
jobs:
  build:
    steps:
      - run: cargo nextest archive --workspace --archive-file t.tar.zst
      - run: cargo clippy --workspace --all-targets > c.json; python3 scripts/check_clippy_ratchet.py
      - run: cargo test --doc --workspace
  sandbox:
    env: {SMELT_REQUIRE_SANDBOX: "1"}
  test:
    env: {SMELT_REQUIRE_SANDBOX: "1"}
    steps:
      - run: cargo nextest run $NEXTEST_META -E "$(python3 scripts/ci_test_shard.py filter)"
      - run: python3 scripts/ci_test_shard.py serial --shard 1 --total 4
  runtime-tiers:
    steps:
      - run: cargo nextest run $NEXTEST_META --run-ignored only -E 'binary(/_runtime$/)'
  package:
    steps:
      - run: scripts/publish-crates.sh
  es-toolkit-regression:
    steps:
      - name: Generated tests compile (cargo test --no-run)
        run: |
          RUSTFLAGS=-Awarnings cargo test --no-run \\
            --manifest-path dist/Cargo.toml
          echo "cargo test exit status"
      - run: python3 scripts/check_library_gate.py runtime-ratchet
  hono:
    steps:
      - run: python3 scripts/check_library_gate.py compile-pin
"""

GOOD_COVERAGE = """on: push
jobs:
  coverage:
    env: {SMELT_REQUIRE_SANDBOX: "1"}
"""


def crate(name, *dependencies, publish=None, dev=()):
    """A `cargo metadata` package."""
    return {
        "name": name,
        "publish": publish,
        "dependencies": [{"name": dep, "kind": None} for dep in dependencies]
        + [{"name": dep, "kind": "dev"} for dep in dev],
    }


class PublishOrder(unittest.TestCase):
    """`publish_order.py`."""

    def test_every_crate_follows_its_path_dependencies(self):
        order, blocked = publish_order.plan([
            crate("top", "mid", "serde"), crate("mid", "base"), crate("base")])
        self.assertEqual(order, ["base", "mid", "top"])
        self.assertEqual(blocked, {})

    def test_a_dev_dependency_does_not_order_or_block(self):
        order, blocked = publish_order.plan([crate("a", dev=["b"]), crate("b", "a")])
        self.assertEqual(order, ["a", "b"])
        self.assertEqual(blocked, {})

    def test_a_crate_on_an_unpublishable_one_is_blocked_and_so_are_its_users(self):
        order, blocked = publish_order.plan([
            crate("private", publish=[]), crate("mid", "private"), crate("top", "mid"), crate("free")])
        self.assertEqual(order, ["free"])
        self.assertEqual(blocked, {"mid": "private", "top": "mid"})

    def test_a_cycle_stops(self):
        with self.assertRaises(SystemExit):
            publish_order.plan([crate("a", "b"), crate("b", "a")])


class CiPin(unittest.TestCase):
    """`ci_pin.py`."""

    CONFIG = {"tools": {"node": "1.2.3"},
              "libraries": [{"name": "lib", "repo": "o/lib", "ref": "abc"}]}

    def test_reads_each_kind(self):
        self.assertEqual(ci_pin.pin(self.CONFIG, "tool", "node"), "1.2.3")
        self.assertEqual(ci_pin.pin(self.CONFIG, "ref", "lib"), "abc")
        self.assertEqual(ci_pin.pin(self.CONFIG, "repo", "lib"), "o/lib")

    def test_an_unknown_name_is_an_error(self):
        with self.assertRaises(KeyError):
            ci_pin.pin(self.CONFIG, "ref", "missing")

    def test_the_real_file_holds_what_ci_asks_for(self):
        with open(ci_pin.CONFIG, encoding="utf-8") as handle:
            config = json.load(handle)
        for tool in ("node", "typescript"):
            self.assertTrue(ci_pin.pin(config, "tool", tool))
        for library in ("remeda", "radash", "hono", "es-toolkit"):
            self.assertRegex(ci_pin.pin(config, "ref", library), r"^[0-9a-f]{40}$")


class CiGates(unittest.TestCase):
    """`check_ci_gates.py`."""

    def test_the_good_workflow_passes(self):
        self.assertEqual(check_ci_gates.workflow_violations("ci.yml", GOOD_CI), [])

    def test_continue_on_error_is_refused(self):
        text = GOOD_CI.replace("      - run: python3 scripts/check_library_gate.py compile-pin",
                               "      - name: Quiet\n        continue-on-error: true\n"
                               "        run: python3 scripts/check_library_gate.py compile-pin")
        found = check_ci_gates.workflow_violations("ci.yml", text)
        self.assertEqual(len(found), 1, found)
        self.assertIn("Quiet", found[0])

    def test_the_allowed_continue_on_error_is_exactly_that_step(self):
        steps = ("jobs:\n  compat:\n    steps:\n      - name: Run\n        id: {}\n"
                 "        continue-on-error: true\n")
        self.assertEqual(
            check_ci_gates.workflow_violations("compat.yml", steps.format("generated-tests")), [])
        self.assertEqual(
            len(check_ci_gates.workflow_violations("compat.yml", steps.format("other"))), 1)

    def test_removing_strict_mode_from_a_job_is_refused(self):
        text = GOOD_CI.replace('  test:\n    env: {SMELT_REQUIRE_SANDBOX: "1"}\n', "  test:\n    env: {}\n")
        self.assertEqual(len(check_ci_gates.workflow_violations("ci.yml", text)), 1)
        self.assertEqual(check_ci_gates.workflow_violations("coverage.yml", GOOD_COVERAGE), [])
        self.assertEqual(len(check_ci_gates.workflow_violations(
            "coverage.yml", GOOD_COVERAGE.replace('{SMELT_REQUIRE_SANDBOX: "1"}', "{}"))), 1)

    def test_removing_a_gate_step_is_refused(self):
        for gate in ("check_clippy_ratchet.py", "publish-crates.sh", "runtime-ratchet", "compile-pin",
                     "cargo test --doc", "ci_test_shard.py serial"):
            found = check_ci_gates.workflow_violations("ci.yml", GOOD_CI.replace(gate, "true"))
            self.assertEqual(len(found), 1, (gate, found))

    def test_a_second_workspace_compile_is_refused(self):
        for command in ("cargo build --bin smelt", "cargo test -p smelt-codegen-rust",
                        "cargo +nightly clippy --workspace", "cargo check"):
            text = GOOD_CI.replace("      - run: scripts/publish-crates.sh",
                                   f"      - run: {command}\n      - run: scripts/publish-crates.sh")
            found = check_ci_gates.workflow_violations("ci.yml", text)
            self.assertEqual(len(found), 1, (command, found))
            self.assertIn("job `package` compiles the workspace", found[0])

    def test_a_nextest_run_outside_the_archive_is_refused(self):
        text = GOOD_CI.replace("--run-ignored only", "").replace("run $NEXTEST_META", "run", 1)
        found = check_ci_gates.workflow_violations("ci.yml", text)
        self.assertEqual(len(found), 1, found)
        self.assertIn("without the archive", found[0])

    def test_literal_pins_are_refused(self):
        for text in ("run: npm install --global typescript@5.9.3\n",
                     "        node-version: '24.21.0'\n",
                     "run: git checkout 3c80f28bb394edbf89f1fc9978571dec8ed20edc\n",
                     "uses: dtolnay/rust-toolchain@stable\n"):
            self.assertEqual(len(check_ci_gates.pin_violations("x.yml", text)), 1, text)
        self.assertEqual(check_ci_gates.pin_violations(
            "x.yml", "node-version: ${{ steps.pins.outputs.node }}\n"), [])

    def test_every_tier_runs_by_pattern(self):
        tiers = check_ci_gates.tier_violations
        self.assertEqual(tiers(["a_runtime", "new_runtime"], GOOD_CI), [])
        for broken in (GOOD_CI.replace("binary(/_runtime$/)", "binary(a_runtime)"),
                       GOOD_CI.replace("--run-ignored only", ""),
                       GOOD_CI.replace("  runtime-tiers:", "  tiers:")):
            self.assertIn("no longer runs every", tiers(["a_runtime"], broken)[0])

    def test_silent_skips_are_found(self):
        for source in (
            "    let Some(python) = discovered_python() else {\n        return Ok(());\n    };\n",
            "    if !node_runs_typescript() {\n        return Ok(());\n    }\n",
            '    if Command::new("tsc").output().is_err() {\n        return;\n    }\n',
            "    if backend.availability() != BackendAvailability::Available {\n        return Ok(());\n    }\n",
        ):
            self.assertEqual(len(check_ci_gates.test_source_violations("t.rs", source)), 1, source)

    def test_a_routed_skip_passes(self):
        source = ('    if !node_runs_typescript() {\n'
                  '        return missing_prerequisite("t", "node");\n    }\n')
        self.assertEqual(check_ci_gates.test_source_violations("t.rs", source), [])


class TestShards(unittest.TestCase):
    """`ci_test_shard.py`."""

    def test_example_slices_cover_every_example_once(self):
        names = [f"{index:03}_x" for index in range(169)]
        for total in (1, 2, 3, 4, 7):
            slices = [ci_test_shard.example_slice(names, shard, total) for shard in range(1, total + 1)]
            self.assertEqual(sorted(name for part in slices for name in part), names)
            self.assertTrue(all(slices), total)

    def test_every_process_binary_has_exactly_one_shard(self):
        for total in (1, 2, 4):
            assigned = [binary for shard in range(1, total + 1)
                        for binary in ci_test_shard.assigned_binaries(shard, total)]
            self.assertEqual(sorted(assigned), sorted(ci_test_shard.PROCESS_BINARIES))

    def test_the_filter_excludes_exactly_the_process_binaries(self):
        found = ci_test_shard.nextest_filter()
        self.assertTrue(found.startswith("not ("))
        for binary in ci_test_shard.PROCESS_BINARIES:
            self.assertIn(f"binary_id({binary})", found)

    def test_a_missing_binary_of_an_archived_package_stops(self):
        archived = {"smelt-transpiler::other_tests": "/x"}
        with self.assertRaises(SystemExit):
            quietly(ci_test_shard.present, "smelt-transpiler::cross_language_matrix_tests", archived)
        self.assertFalse(quietly(ci_test_shard.present, "smelt-codegen-rust::compile_scaling", archived))

    def test_the_shard_with_the_other_end_to_end_tests_gets_fewer_examples(self):
        names = [f"{index:03}_x" for index in range(169)]
        sizes = [len(ci_test_shard.example_slice(names, shard, 4)) for shard in range(1, 5)]
        self.assertLess(sizes[0], min(sizes[1:]))

    def test_the_real_corpus_is_sliced(self):
        names = ci_test_shard.example_names()
        self.assertGreater(len(names), 100)
        self.assertIn("01_number", names)


if __name__ == "__main__":
    unittest.main()
