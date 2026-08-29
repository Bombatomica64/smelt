#!/usr/bin/env python3
"""Transpile the benchmark libraries and turn each generated crate into a bench runner.

The generated crate is left byte-for-byte as `smelt build` emitted it, apart from
three added module files and a replaced `fn main`. That matters for two reasons:
the numbers must describe code Smelt actually produces, not code we touched up,
and the emitter preserves file mtimes so Cargo can reuse incremental artifacts
(see `## Generated Rust incremental builds` in AGENTS.md) -- rewriting the tree
would throw that away.

Steps per library:
  1. clone at the pinned ref (skipped if already present at that ref)
  2. copy the compat Smelt.toml, stripped of test sources, into the checkout
  3. `smelt build`
  4. copy the harness / cases / main into the generated `src/`
  5. append the module declarations and swap the generated `fn main() {}` for ours
  6. `cargo build --release`

Usage:
    python3 benchmarks/prepare.py [--only remeda|es-toolkit] [--skip-build]
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH = ROOT / "benchmarks"
WORK = ROOT / "target" / "compat-repos"

# Pinned refs. remeda is not in .github/compat/libraries.json (it has its own CI
# gate), so its ref is pinned here to match the one ci.yml uses.
REMEDA_REF = "3c80f28bb394"

LIBRARIES = {
    "remeda": {
        "repo": "remeda/remeda",
        "ref": REMEDA_REF,
        "crate": "remeda_bench",
        "cases": "smelt_bench_cases_remeda.rs",
        "entry": "packages/remeda/src/index.ts",
    },
    "es-toolkit": {
        "repo": "toss/es-toolkit",
        "ref": None,  # taken from .github/compat/libraries.json
        "crate": "es_toolkit_bench",
        "cases": "smelt_bench_cases_es_toolkit.rs",
        "entry": "src/index.ts",
    },
}


def run(cmd, **kw):
    print("$", " ".join(str(c) for c in cmd), file=sys.stderr)
    subprocess.run(cmd, check=True, **kw)


def pinned_ref(name: str) -> str:
    """Resolve a library's pinned ref, preferring the shared compat config."""
    cfg = LIBRARIES[name]
    if cfg["ref"]:
        return cfg["ref"]
    libs = json.loads((ROOT / ".github/compat/libraries.json").read_text())["libraries"]
    return next(lib["ref"] for lib in libs if lib["name"] == name)


def checkout(name: str) -> pathlib.Path:
    """Clone `name` at its pinned ref, reusing an existing checkout when it matches."""
    cfg = LIBRARIES[name]
    ref = pinned_ref(name)
    dest = WORK / name
    if (dest / ".git").is_dir():
        head = subprocess.run(
            ["git", "-C", str(dest), "rev-parse", "HEAD"], capture_output=True, text=True
        ).stdout.strip()
        if head.startswith(ref) or ref.startswith(head[:12]):
            print(f">> {name}: reusing checkout at {head[:12]}", file=sys.stderr)
            return dest
    shutil.rmtree(dest, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(["git", "clone", "--no-tags", "--filter=blob:none",
         f"https://github.com/{cfg['repo']}.git", str(dest)])
    run(["git", "-C", str(dest), "checkout", ref])
    return dest


def write_manifest(name: str, repo: pathlib.Path) -> pathlib.Path:
    """Write the bench Smelt.toml: the compat fixture minus tests, emitting dist-bench."""
    cfg = LIBRARIES[name]
    src = (ROOT / ".github/compat" / name / "Smelt.toml").read_text()
    # Test sources roughly double the generated crate and its compile time, and the
    # benchmark never calls them.
    src = re.sub(r"^test-prefix = .*$", "", src, flags=re.M)
    src = src.replace('target = "./dist-smelt"', 'target = "./dist-bench"')
    src = re.sub(r"^crate-name = .*$", f'crate-name = "{cfg["crate"]}"', src, flags=re.M)
    (BENCH / "smelt").mkdir(parents=True, exist_ok=True)
    # The benchmark measures a program built for throughput, so it opts into the
    # allocator such a program would pick. The regression manifests deliberately
    # do not: an allocator cannot change what the generated code computes, and
    # making the gate crates build C would slow CI for no signal.
    src = src.replace("[rust]", '[rust]\nallocator = "mimalloc"', 1)
    (BENCH / "smelt" / f"{name}.Smelt.toml").write_text(src)
    manifest = repo / "Smelt.bench.toml"
    manifest.write_text(src)
    return manifest


def snake(name: str) -> str:
    """`toCamelCase` -> `to_camel_case`, matching the emitter's own renaming."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


# Library export -> generated module file, for every export the benchmarks call.
# The generated *function* name is discovered, not listed, because the emitter
# appends a disambiguating index (`chunk_15`, `uniq_64`) that shifts whenever the
# module graph changes. Pinning those literals in the case files would make the
# benchmarks silently un-buildable after any regeneration.
ENTRY_EXPORTS = {
    "remeda": ["chunk", "unique", "uniqueBy", "groupBy", "sortBy", "sumBy",
               "difference", "intersection", "zip", "isDeepEqual", "clone",
               "toCamelCase", "toKebabCase", "flat", "partition", "countBy"],
    "es-toolkit": ["chunk", "uniq", "uniqBy", "groupBy", "sortBy", "sumBy",
                   "difference", "intersection", "zip", "isEqual", "cloneDeep",
                   "camelCase", "kebabCase", "flatten", "partition", "countBy"],
}

FN_RE = re.compile(r"^pub\(crate\) fn ([A-Za-z_][A-Za-z0-9_]*)(.*)$", re.M)


def resolve_entry(src: pathlib.Path, export: str) -> str:
    """Find the generated Rust function that implements library export `export`.

    The emitter writes each module to `src/<export>.rs` and may rename the function
    to snake_case and/or append `_<index>`. It also emits a nullary
    `fn <export>() -> ()` module-initializer stub in the same file, which is never
    the entry point -- that exact shape is what gets skipped below. (Matching on
    "has parameters" with a regex does not work: a generic parameter list can itself
    contain parentheses and angle brackets, as in
    `fn sum_by<T: ..., F0: Fn(T, f64) -> f64 + ?Sized>(...)`.)
    """
    module = src / f"{export}.rs"
    if not module.is_file():
        raise SystemExit(f"no generated module {module} for export {export!r}")
    wanted = {export, snake(export)}
    for fn, rest in FN_RE.findall(module.read_text()):
        base = re.sub(r"_\d+$", "", fn)
        if base in wanted and not rest.startswith("() -> ()"):
            return fn
    raise SystemExit(f"no entry function for export {export!r} in {module}")


def write_entry_aliases(name: str, src: pathlib.Path) -> None:
    """Emit `smelt_bench_entry.rs`: stable `entry_*` names for the case files."""
    lines = [
        "//! Auto-generated by `benchmarks/prepare.py`. Do not edit.",
        "//!",
        "//! Maps each library export the benchmarks call to the function name the",
        "//! emitter actually produced for it, so the case files can use stable",
        "//! `entry_*` names across regenerations.",
        "",
        "#![allow(unused_imports)]",
        "",
    ]
    for export in ENTRY_EXPORTS[name]:
        fn = resolve_entry(src, export)
        lines.append(f"pub(crate) use super::{fn} as entry_{snake(export)};")
    (src / "smelt_bench_entry.rs").write_text("\n".join(lines) + "\n")


def inject(name: str, dist: pathlib.Path) -> None:
    """Copy the bench modules in and give the generated crate a real `main`."""
    cfg = LIBRARIES[name]
    src = dist / "src"
    shutil.copy(BENCH / "rust/smelt_bench_harness.rs", src / "smelt_bench_harness.rs")
    shutil.copy(BENCH / "rust" / cfg["cases"], src / "smelt_bench_cases.rs")
    write_entry_aliases(name, src)

    main_rs = src / "main.rs"
    text = main_rs.read_text()
    if "mod smelt_bench_harness" in text:
        # Already injected (re-run without regenerating); strip the old tail first so
        # this stays idempotent.
        text = text[: text.index("// --- smelt bench injection ---")]
    # The generated crate root ends with an empty `fn main() {}`; replace exactly that.
    assert text.rstrip().endswith("fn main() {}"), "generated main.rs shape changed"
    text = text.rstrip()[: -len("fn main() {}")]

    tail = ["// --- smelt bench injection ---",
            '#[path = "smelt_bench_entry.rs"]',
            "mod smelt_bench_entry;",
            '#[path = "smelt_bench_harness.rs"]',
            "mod smelt_bench_harness;",
            '#[path = "smelt_bench_cases.rs"]',
            "mod smelt_bench_cases;",
            "",
            (BENCH / "rust/smelt_bench_main.rs").read_text()]
    main_rs.write_text(text + "\n" + "\n".join(tail))


def bundle_typescript(name: str, repo: pathlib.Path) -> None:
    """Bundle the library's own TypeScript source to a single ESM file for Node.

    The benchmark deliberately measures the library's *source* at the pinned ref, the
    same input Smelt was given, rather than its published npm build -- otherwise the
    two sides would not be running the same program. Bun is used only as the bundler;
    the benchmark itself always runs on Node.

    The bundles are derived third-party code, so they are generated here and gitignored
    rather than vendored into the repository.
    """
    cfg = LIBRARIES[name]
    out = BENCH / "ts/vendor" / f"{name}.mjs"
    out.parent.mkdir(parents=True, exist_ok=True)
    if shutil.which("bun") is None:
        print(f"!! bun not found; leaving {out} as-is", file=sys.stderr)
        return
    run(["bun", "build", str(repo / cfg["entry"]),
         "--target=node", "--format=esm", "--outfile", str(out)])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", choices=sorted(LIBRARIES))
    ap.add_argument("--skip-build", action="store_true",
                    help="stop after transpiling + injecting; do not run cargo")
    args = ap.parse_args()

    # ALWAYS rebuild, never `if not smelt.exists()`. A stale binary here is the
    # worst possible failure mode for this script: transpiling with codegen from
    # some earlier commit still produces a bench crate that builds and runs and
    # agrees on every checksum, so a before/after comparison silently measures
    # the same old codegen twice and reports "no change" for a fix that was
    # never in the binary. Cargo makes the no-op case cheap; a wrong number is
    # not cheap. (Measured: this masked the entire #218 fix set, whose numbers
    # had to be retracted.)
    smelt = ROOT / "target/release/smelt"
    run(["cargo", "build", "--release", "-p", "smelt-transpiler", "--bin", "smelt"], cwd=ROOT)

    names = [args.only] if args.only else list(LIBRARIES)
    for name in names:
        repo = checkout(name)
        bundle_typescript(name, repo)
        manifest = write_manifest(name, repo)
        run([str(smelt), "--manifest-path", str(manifest), "build"], cwd=ROOT)
        dist = repo / "dist-bench"
        inject(name, dist)
        if not args.skip_build:
            # Touch every generated source before building. The Rust emitter
            # deliberately preserves generated-file mtimes (see CLAUDE.md) so that
            # Cargo can reuse incremental artifacts for large generated crates —
            # but Cargo decides what to rebuild FROM mtimes, so switching between
            # two trees that emit different bytes can leave the previous tree's
            # binary in place. That is not hypothetical: a branch-vs-main
            # comparison silently measured the branch's binary twice, because the
            # main-side regeneration wrote different source and Cargo rebuilt
            # nothing. Same failure mode as the stale `target/release/smelt`
            # above, one level down, and just as quiet — the numbers agree to the
            # instruction, which reads like a null result rather than a bug.
            for generated in (dist / "src").rglob("*.rs"):
                generated.touch()
            run(["cargo", "build", "--release"], cwd=dist)
        print(f">> {name}: bench binary at {dist}/target/release/{LIBRARIES[name]['crate']}",
              file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
