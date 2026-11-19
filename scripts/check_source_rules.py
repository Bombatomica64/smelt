#!/usr/bin/env python3
"""Check the source rules AGENTS.md used to state in prose.

Each rule here replaced a sentence an agent had to remember. A violation names
the root-cause fix, not just the line, so the fix is the default and no prose
rule is needed. Exits 1 and lists every violation.

Rules:
  corgi-inputs    a non-`.rs` file read with `include_str!`/`include_bytes!` is
                  declared in `corgi.toml` `[extra-inputs]`
  bare-erasure    a frontend names `Type::Unknown` only inside its erasure
                  helper, so every erasure carries a reason
  agent-docs      CLAUDE.md is the symlink to AGENTS.md, and every repository
                  path AGENTS.md names exists
"""

import os
import pathlib
import posixpath
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent

INCLUDE = re.compile(r'\binclude_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')

# The one file per frontend that may turn a value into `Type::Unknown`: the
# erasure helper that records why. Everything else calls it.
ERASURE_HELPERS = {
    "crates/smelt-frontend-ts/src": "crates/smelt-frontend-ts/src/lowering/erasure.rs",
    "crates/smelt-frontend-py/src": "crates/smelt-frontend-py/src/lowering/type_helpers.rs",
}
BARE_UNKNOWN = re.compile(r"\bintern(?:_type)?\(\s*(?:smelt_hir::)?Type::Unknown\s*,?\s*\)")

# A repository path in backticks: a known top-level directory or file.
AGENT_DOC_PATH = re.compile(
    r"`((?:crates|docs|scripts|blocker-logs|skills|examples|\.github|specs)/[\w./*-]+"
    r"|[\w-]+\.(?:toml|md|json))`"
)


def is_test_source(path):
    """A source file the test builds read, which corgi never compiles."""
    parts = path.split("/")
    name = parts[-1]
    return "tests" in parts[:-1] or name == "tests.rs" or name.endswith("_tests.rs")


def glob_regex(pattern):
    """The regex for a corgi glob: `**` crosses directories, `*` does not."""
    out = []
    index = 0
    while index < len(pattern):
        if pattern.startswith("**", index):
            out.append(".*")
            index += 2
        elif pattern[index] == "*":
            out.append("[^/]*")
            index += 1
        else:
            out.append(re.escape(pattern[index]))
            index += 1
    return re.compile("".join(out) + r"\Z")


def corgi_input_violations(sources, crate_roots, extra_inputs):
    """Undeclared compile-time reads of non-`.rs` files.

    `sources` maps a repo-relative `.rs` path to its text; `crate_roots` maps a
    package name to its repo-relative directory; `extra_inputs` is corgi.toml's
    `[extra-inputs]` table (package -> globs relative to the package).
    """
    found = []
    for path, text in sorted(sources.items()):
        if is_test_source(path):
            continue
        owner = next(
            ((name, root) for name, root in crate_roots.items() if path.startswith(root + "/")),
            None,
        )
        if owner is None:
            continue
        name, root = owner
        declared = [glob_regex(glob) for glob in extra_inputs.get(name, [])]
        for match in INCLUDE.finditer(text):
            target = posixpath.normpath(posixpath.join(posixpath.dirname(path), match.group(1)))
            if target.endswith(".rs"):
                continue
            relative = posixpath.relpath(target, root)
            if not any(pattern.match(relative) for pattern in declared):
                line = text.count("\n", 0, match.start()) + 1
                found.append(
                    f"{path}:{line}: reads `{relative}` at compile time but corgi.toml "
                    f"[extra-inputs] has no glob for it in `{name}`. corgi sandboxes rustc to "
                    f"the declared inputs, so the build fails under corgi: add "
                    f'`{name} = ["{posixpath.dirname(relative) or "."}/**"]` to corgi.toml.'
                )
    return found


def bare_erasure_violations(sources):
    """A frontend naming `Type::Unknown` outside its erasure helper."""
    found = []
    for path, text in sorted(sources.items()):
        if is_test_source(path):
            continue
        for frontend, helper in ERASURE_HELPERS.items():
            if not path.startswith(frontend + "/") or path == helper:
                continue
            for match in BARE_UNKNOWN.finditer(text):
                line = text.count("\n", 0, match.start()) + 1
                found.append(
                    f"{path}:{line}: `{match.group(0)}` erases a type without saying why. "
                    f"Fix the cause: carry the real type to this site; if the value really is "
                    f"dynamic, call `erase(ErasureReason::..)` (reasons: docs/erasure-reasons.md); "
                    f"if it is only compared or already erased upstream, call "
                    f"`unknown_type(NotNewErasure::..)`."
                )
    return found


def agent_doc_violations(agents_text, symlink_target, exists):
    """CLAUDE.md must be AGENTS.md; AGENTS.md must name only paths that exist."""
    found = []
    if symlink_target != "AGENTS.md" and symlink_target != "./AGENTS.md":
        found.append(
            "CLAUDE.md is not a symlink to AGENTS.md: two copies drift, and the agent that "
            "reads the stale one follows rules the other deleted. "
            "Run `ln -sf AGENTS.md CLAUDE.md`."
        )
    for match in AGENT_DOC_PATH.finditer(agents_text):
        path = match.group(1)
        if "*" in path or "<" in path:
            continue
        if not exists(path):
            line = agents_text.count("\n", 0, match.start()) + 1
            found.append(
                f"AGENTS.md:{line}: names `{path}`, which does not exist. A rule pointing at a "
                f"missing file sends the agent hunting; update the path or delete the rule."
            )
    return found


def load_sources():
    """Every tracked `.rs` file under crates/, by repo-relative path."""
    sources = {}
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        if "/target/" in relative or "node_modules" in relative:
            continue
        sources[relative] = path.read_text(encoding="utf-8", errors="replace")
    return sources


def load_crate_roots():
    """Package name -> directory, for every crate with a Cargo.toml."""
    roots = {}
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text(encoding="utf-8")).get("package", {})
        if "name" in package:
            roots[package["name"]] = manifest.parent.relative_to(ROOT).as_posix()
    return roots


def main():
    """Run every rule over the repository."""
    sources = load_sources()
    corgi = tomllib.loads((ROOT / "corgi.toml").read_text(encoding="utf-8"))
    claude = ROOT / "CLAUDE.md"
    symlink_target = os.readlink(claude) if claude.is_symlink() else None
    violations = (
        corgi_input_violations(sources, load_crate_roots(), corgi.get("extra-inputs", {}))
        + bare_erasure_violations(sources)
        + agent_doc_violations(
            (ROOT / "AGENTS.md").read_text(encoding="utf-8"),
            symlink_target,
            lambda path: (ROOT / path).exists(),
        )
    )
    for violation in violations:
        print(violation)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
