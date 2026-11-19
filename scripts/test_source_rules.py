#!/usr/bin/env python3
"""Unit tests for check_source_rules.py. Run: python3 -m unittest discover -s scripts"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import check_source_rules as rules

ROOTS = {"smelt-specialize": "crates/smelt-specialize"}


class CorgiInputs(unittest.TestCase):
    """`corgi-inputs`."""

    def check(self, path, text, declared):
        """The violations for one source file."""
        return rules.corgi_input_violations({path: text}, ROOTS, {"smelt-specialize": declared})

    def test_a_declared_glob_covers_the_file(self):
        text = 'const G: &str = include_str!("../guest/node.js");'
        self.assertEqual(self.check("crates/smelt-specialize/src/node.rs", text, ["guest/**"]), [])

    def test_an_undeclared_file_is_reported_with_the_fix(self):
        text = 'const G: &str = include_str!("../guest/node.js");'
        found = self.check("crates/smelt-specialize/src/node.rs", text, [])
        self.assertEqual(len(found), 1)
        self.assertIn("node.rs:1", found[0])
        self.assertIn('smelt-specialize = ["guest/**"]', found[0])

    def test_a_rust_file_is_a_tracked_input_already(self):
        text = 'const S: &str = include_str!("runtime/value.rs");'
        self.assertEqual(self.check("crates/smelt-specialize/src/lib.rs", text, []), [])

    def test_a_test_only_read_is_not_a_compile_input(self):
        text = 'let s = include_str!("../../tests/fixtures/a.ts");'
        for path in (
            "crates/smelt-specialize/src/tests/a_tests.rs",
            "crates/smelt-specialize/tests/a.rs",
            "crates/smelt-specialize/src/a_tests.rs",
        ):
            self.assertEqual(self.check(path, text, []), [], path)

    def test_a_single_star_does_not_cross_directories(self):
        text = 'const G: &str = include_str!("../guest/deep/node.js");'
        self.assertEqual(len(self.check("crates/smelt-specialize/src/n.rs", text, ["guest/*"])), 1)


class BareErasure(unittest.TestCase):
    """`bare-erasure`."""

    TS = "crates/smelt-frontend-ts/src/lowering/stmt/a.rs"

    def test_a_bare_intern_is_reported_with_the_fix(self):
        text = "let t = self.ctx.krate.types.intern(Type::Unknown);"
        found = rules.bare_erasure_violations({self.TS: text})
        self.assertEqual(len(found), 1)
        self.assertIn("erase(ErasureReason::..)", found[0])

    def test_a_formatted_multiline_intern_is_still_found(self):
        text = "types.intern(\n    Type::Unknown,\n)"
        self.assertEqual(len(rules.bare_erasure_violations({self.TS: text})), 1)

    def test_the_python_spelling_is_found(self):
        path = "crates/smelt-frontend-py/src/lowering/call.rs"
        self.assertEqual(
            len(rules.bare_erasure_violations({path: "self.intern_type(Type::Unknown)"})), 1)

    def test_the_erasure_helper_may_name_it(self):
        for path in rules.ERASURE_HELPERS.values():
            self.assertEqual(
                rules.bare_erasure_violations({path: "self.intern_type(Type::Unknown)"}), [])

    def test_tests_and_other_crates_are_not_judged(self):
        text = "types.intern(Type::Unknown)"
        self.assertEqual(rules.bare_erasure_violations({
            "crates/smelt-frontend-ts/src/tests/a_tests.rs": text,
            "crates/smelt-mir/src/lower/mod.rs": text,
        }), [])


class AgentDocs(unittest.TestCase):
    """`agent-docs`."""

    def test_a_real_path_and_the_symlink_pass(self):
        found = rules.agent_doc_violations("see `docs/a.md`", "AGENTS.md", lambda path: True)
        self.assertEqual(found, [])

    def test_a_missing_path_is_reported_with_its_line(self):
        found = rules.agent_doc_violations("x\nsee `docs/gone.md`", "AGENTS.md", lambda p: False)
        self.assertEqual(len(found), 1)
        self.assertIn("AGENTS.md:2", found[0])

    def test_globs_and_placeholders_are_not_paths(self):
        text = "`blocker-logs/<name>.md` and `crates/*/src`"
        self.assertEqual(rules.agent_doc_violations(text, "AGENTS.md", lambda p: False), [])

    def test_a_copied_claude_md_is_reported(self):
        found = rules.agent_doc_violations("", None, lambda path: True)
        self.assertEqual(len(found), 1)
        self.assertIn("ln -sf AGENTS.md CLAUDE.md", found[0])


if __name__ == "__main__":
    unittest.main()
