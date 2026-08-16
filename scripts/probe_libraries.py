#!/usr/bin/env python3
"""Drive the daily third-party "bug library" transpile probes and emit a report.

For each library listed in `.github/compat/libraries.json` (already checked out
under --work-dir, with its fixture `Smelt.toml` copied into place), this script:

  1. runs `smelt build` to attempt a whole-crate transpile;
  2. if a Rust crate is emitted, runs `smelt rust-test-report --full` and records
     how many generated `cargo test` cases pass/fail;
  3. if the build aborts at the first unsupported construct, scans every source/
     test file individually with `smelt dump-hir` to enumerate the full set of
     distinct blocker classes, categorized as missing-stdlib vs non-working Rust.

It writes a single Markdown report (the `Transpile y/n -> tests pass -> per-error
category -> where we fail` shape) to --output. Designed to run unattended in CI.

Because the report is auto-committed, the run must be able to tell "Smelt cannot
transpile this" from "this script never saw the sources": both render as
`transpile: no` with no blockers. A run that scanned zero files for any library,
or that is missing a checkout, fails without touching the report -- see
`degenerate_probes`.

Check the libraries out with `scripts/checkout_probe_libraries.py` first; the
work dir must not live under `target/`, which CI restores from the cargo cache.

Usage:
    python3 scripts/probe_libraries.py \
        --config .github/compat/libraries.json \
        --fixtures .github/compat \
        --work-dir "$RUNNER_TEMP/library-probes" \
        --output blocker-logs/library-probes.md
"""
from __future__ import annotations
import argparse, datetime as dt, json, os, re, shutil, subprocess, sys
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor

# --- error classification -------------------------------------------------

# JS/Python builtins Smelt does not model yet -> "missing stdlib".
STDLIB = {
    "Array", "Number", "String", "Boolean", "Object", "Reflect", "Math", "Map",
    "Set", "TextEncoder", "TextDecoder", "Promise", "Date", "JSON", "RegExp",
    "Symbol", "BigInt", "WeakMap", "WeakSet", "Proxy", "Intl", "ArrayBuffer",
    "DataView", "Iterator", "Buffer", "Blob", "File", "AbortController",
    "globalThis", "parseInt", "parseFloat", "isFinite", "isNaN", "Function",
}
STDLIB_PHRASES = ("TextEncoder", "TextDecoder", "instanceof")
# Single-file scanning cannot resolve cross-file symbols; treat bare unresolved
# identifier/name errors as scan noise so they do not pollute the blocker table.
NOISE_PREFIXES = ("unresolved name", "unresolved identifier", "unresolved const",
                  "references unresolved const", "<timeout>")

MSG_RE = re.compile(r'message: \\"(.*?)\\"')
SYM_RE = re.compile(r"unresolved (?:callback )?(?:identifier|class) [`']([^`']+)[`']")
ABORT_FILE_RE = re.compile(r'error: "([^"]*\.(?:ts|py))')
TEST_RESULT_RE = re.compile(r"(\d+) passed; (\d+) failed")


def classify(cls: str) -> str:
    """Map a blocker class to one of: stdlib, stdlib?, noise, rust."""
    if any(p in cls for p in STDLIB_PHRASES):
        return "stdlib"
    if cls.startswith("unresolved class"):
        return "stdlib?"  # builtin used via new/extends, e.g. `unresolved class Array`
    if any(cls.startswith(n) for n in NOISE_PREFIXES):
        return "noise"
    return "rust"


def normalize(msg: str) -> str:
    """Collapse a message to its blocker class by erasing quoted specifics."""
    s = re.sub(r"'[^']*'", "'X'", msg)
    s = re.sub(r"`[^`]*`", "`X`", s)
    return s


# --- smelt invocation -----------------------------------------------------

def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def dump_hir(smelt_bin: str, smelt_root: str, path: str):
    """Return (messages, stdlib_symbols) for one source file."""
    try:
        r = run([smelt_bin, "dump-hir", os.path.abspath(path)], timeout=45, cwd=smelt_root)
    except subprocess.TimeoutExpired:
        # Pathological type-level files (e.g. ts-pattern) can hang the frontend;
        # they are excluded as scan noise, so a short cap keeps CI fast.
        return ["<timeout>"], []
    out = r.stdout + r.stderr
    msgs = MSG_RE.findall(out)
    syms = [s for m in msgs for s in SYM_RE.findall(m) if s in STDLIB]
    return msgs, syms


def list_source_files(root: str, lang: str):
    exts = (".py",) if lang == "py" else (".ts",)
    out = []
    for dp, _dn, fn in os.walk(root):
        if any(x in dp for x in ("node_modules", "/dist-", "/.git", "__pycache__")):
            continue
        for f in fn:
            if f.endswith(exts) and not f.endswith(".d.ts"):
                out.append(os.path.join(dp, f))
    return out


def scan_library(smelt_bin, smelt_root, lib_dir, roots, lang):
    """Enumerate distinct blocker classes across every file in the library."""
    files = []
    for r in roots:
        p = os.path.join(lib_dir, r)
        if os.path.isdir(p):
            files += list_source_files(p, lang)
    files = sorted(set(files))
    cls_occ, cls_files, syms = Counter(), defaultdict(set), Counter()
    fail_files = set()
    with ThreadPoolExecutor(max_workers=min(8, (os.cpu_count() or 2))) as ex:
        results = ex.map(lambda f: (f, *dump_hir(smelt_bin, smelt_root, f)), files)
        for path, msgs, file_syms in results:
            if msgs:
                fail_files.add(path)
            for m in msgs:
                c = normalize(m)
                cls_occ[c] += 1
                cls_files[c].add(path)
            for s in file_syms:
                syms[s] += 1
    classes = [
        {"class": c, "occurrences": n, "files": len(cls_files[c]), "category": classify(c)}
        for c, n in cls_occ.most_common()
    ]
    return {
        "files_scanned": len(files),
        "files_with_errors": len(fail_files),
        "classes": classes,
        "stdlib_symbols": dict(syms.most_common()),
    }


def probe_library(smelt_bin, smelt_root, lib, work_dir, fixtures_dir):
    """Build one library and return its probe result dict."""
    name, lang, roots = lib["name"], lib["lang"], lib["roots"]
    lib_dir = os.path.join(work_dir, name)
    fixture = os.path.join(fixtures_dir, name, "Smelt.toml")
    shutil.copy(fixture, os.path.join(lib_dir, "Smelt.toml"))
    manifest = os.path.join(lib_dir, "Smelt.toml")

    build = run([smelt_bin, "build", "--manifest-path", manifest], cwd=smelt_root)
    build_out = build.stdout + build.stderr

    # output.target / output.crate-name from fixture (default dist-smelt)
    target = "dist-smelt"
    m = re.search(r'target\s*=\s*"\.?/?([^"]+)"', open(fixture).read())
    if m:
        target = m.group(1)
    cargo_manifest = os.path.join(lib_dir, target, "Cargo.toml")

    res = {"name": name, "repo": lib["repo"], "ref": lib["ref"], "lang": lang, "roots": roots}

    if os.path.isfile(cargo_manifest):
        res["transpile"] = True
        report_path = os.path.join(lib_dir, "_test_report.md")
        tr = run([smelt_bin, "rust-test-report",
                  "--build-manifest", manifest,
                  "--cargo-manifest", cargo_manifest,
                  "--full", "--suppress-warnings",
                  "--output", report_path], cwd=smelt_root)
        report = open(report_path).read() if os.path.isfile(report_path) else (tr.stdout + tr.stderr)
        mr = TEST_RESULT_RE.search(report)
        if mr:
            res["passed"], res["failed"] = int(mr.group(1)), int(mr.group(2))
        res["test_report"] = report
    else:
        res["transpile"] = False
        ab = ABORT_FILE_RE.search(build_out)
        if ab:
            af = ab.group(1).replace(lib_dir + "/", "")
            af = re.sub(r"^\.?/+", "", af).replace("/./", "/")
            res["abort_file"] = af
        else:
            res["abort_file"] = "(unknown)"
        res.update(scan_library(smelt_bin, smelt_root, lib_dir, roots, lang))
    return res


# --- report rendering -----------------------------------------------------

CAT_LABEL = {
    "stdlib": "missing-stdlib",
    "stdlib?": "missing-stdlib (builtin class)",
    "rust": "non-working Rust",
}


def visible_classes(res, limit=14):
    rows = [c for c in res.get("classes", []) if classify(c["class"]) != "noise"]
    return rows[:limit], rows


def render(results, date_str):
    L = []
    w = L.append
    w("# Bug-library transpile probes (TypeScript + Python)")
    w("")
    w(f"_Generated {date_str} by the `library-probes` workflow "
      "(`scripts/probe_libraries.py`)._")
    w("")
    w("Each library is checked out at a pinned ref (see "
      "`.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, "
      "and run through `smelt build`. If a crate is emitted, its generated `cargo test` "
      "suite is run and counted. Otherwise every source/test file is scanned individually "
      "with `smelt dump-hir` to enumerate the full set of distinct blocker classes "
      "(single-file mode cannot resolve cross-file imports, so bare `unresolved "
      "name/identifier` errors are excluded as scan noise).")
    w("")
    w("**Error categories:**")
    w("- **missing-stdlib** — a JS/Python builtin Smelt does not model yet "
      "(`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).")
    w("- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the "
      "construct into Rust that compiles (missing return-type annotations, `try`/`except`, "
      "decorators, callback methods not lowered into closures, non-primitive exported "
      "`const`, ...).")
    w("")

    # summary
    w("## Summary")
    w("")
    w("| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |")
    w("| --- | --- | --- | --- | --- | ---: | --- |")
    for r in results:
        lang = r["lang"].upper()
        if r["transpile"]:
            tests = (f"{r['passed']} / {r['failed']}" if "passed" in r
                     else "transpiled (counts unparsed)")
            row = (f"| [{r['name']}](https://github.com/{r['repo']}) | {lang} | **yes** | "
                   f"{tests} | — | — | — |")
        else:
            _, rows = visible_classes(r)
            nstd = sum(1 for c in rows if classify(c["class"]) in ("stdlib", "stdlib?"))
            nrust = sum(1 for c in rows if classify(c["class"]) == "rust")
            dom = "non-working Rust" if nrust >= nstd else "missing-stdlib"
            row = (f"| [{r['name']}](https://github.com/{r['repo']}) | {lang} | **no** | "
                   f"n/a | `{r['abort_file']}` | {len(rows)} | {dom} ({nrust}r/{nstd}s) |")
        w(row)
    w("")

    # per-library detail
    for r in results:
        w(f"## {r['name']}")
        w("")
        w(f"- Source: `{r['repo']}` @ `{r['ref'][:12]}`")
        if r["transpile"]:
            w("- Transpile: **yes** — Rust crate emitted")
            if "passed" in r:
                w(f"- Generated `cargo test`: **{r['passed']} passed / {r['failed']} failed**")
            else:
                w("- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed")
            w("")
            continue
        w(f"- Transpile: **no** — `smelt build` aborts at `{r['abort_file']}`")
        w("- Tests passing: **n/a** (no Rust crate emitted)")
        w(f"- Files scanned: {r['files_scanned']} · with blockers: {r['files_with_errors']}")
        w("")
        rows_md, _ = visible_classes(r)
        w("| Occurrences | Files | Category | Blocker class |")
        w("| ---: | ---: | --- | --- |")
        for c in rows_md:
            label = CAT_LABEL[classify(c["class"])]
            w(f"| {c['occurrences']} | {c['files']} | {label} | "
              f"{c['class'][:90].replace('|', chr(92) + '|')} |")
        w("")

    # cross-cutting: highest-leverage lowering gaps
    libcount, occ = defaultdict(set), Counter()
    for r in results:
        if r["transpile"]:
            continue
        for c in r.get("classes", []):
            if classify(c["class"]) == "rust":
                libcount[c["class"]].add(r["name"])
                occ[c["class"]] += c["occurrences"]
    ranked = sorted(libcount.items(), key=lambda kv: (len(kv[1]), occ[kv[0]]), reverse=True)
    multi = [(c, libs) for c, libs in ranked if len(libs) >= 2]
    if multi:
        w("## Highest-leverage transpiler gaps (non-working Rust)")
        w("")
        w("Lowering gaps blocking more than one probed library; fixing these unlocks the most surface.")
        w("")
        w("| Libraries hit | Total occ. | Blocker class |")
        w("| ---: | ---: | --- |")
        for c, libs in multi:
            safe = c[:100].replace("|", "\\|")
            w(f"| {len(libs)} ({', '.join(sorted(libs))}) | {occ[c]} | {safe} |")
        w("")

    # cross-cutting: missing stdlib builtins
    sym_rows = [(r["name"], r["stdlib_symbols"]) for r in results
                if not r["transpile"] and r.get("stdlib_symbols")]
    if sym_rows:
        w("## Missing stdlib builtins observed")
        w("")
        w("Builtins referenced in `new`/`extends`/callback position that Smelt does not resolve.")
        w("")
        w("| Library | Builtins (occurrences) |")
        w("| --- | --- |")
        for name, syms in sym_rows:
            s = ", ".join(f"`{k}`×{v}" for k, v in syms.items())
            w(f"| {name} | {s} |")
        w("")
    return "\n".join(L) + "\n"


# --- degenerate-run detection ---------------------------------------------

def degenerate_probes(results, missing, expected):
    """Describe every way this run failed to actually probe anything.

    A probe that never saw the library's sources looks exactly like a library
    Smelt cannot transpile at all: `transpile: no`, zero blocker classes, abort
    file `(unknown)`. Committing that is worse than committing nothing, because
    it overwrites real measurements with a plausible-looking regression. This
    returns human-readable reasons; an empty list means the run is trustworthy.
    """
    problems = []
    if missing:
        problems.append(f"{len(missing)} of {expected} libraries were not checked out: "
                        f"{', '.join(missing)}")
    if not results:
        problems.append("no library was probed at all")
    for r in results:
        # A transpiled library emits a crate instead of a file scan, so
        # `files_scanned` is only meaningful on the abort path.
        if not r.get("transpile") and r.get("files_scanned", 0) == 0:
            problems.append(f"{r['name']}: aborted at {r.get('abort_file', '(unknown)')} and "
                            f"scanned 0 source files -- the checkout is empty or its "
                            f"`roots` {r['roots']} are wrong")
    return problems


# --- main -----------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--config", default=".github/compat/libraries.json")
    ap.add_argument("--fixtures", default=".github/compat")
    ap.add_argument("--work-dir", default=".library-probes")
    ap.add_argument("--output", default="blocker-logs/library-probes.md")
    ap.add_argument("--smelt-bin", default=os.environ.get("SMELT_BIN", "target/debug/smelt"))
    ap.add_argument("--smelt-root", default=os.environ.get("SMELT_ROOT", os.getcwd()))
    ap.add_argument("--date", default=dt.date.today().isoformat())
    ap.add_argument("--allow-degenerate", action="store_true",
                    help="write the report even when libraries were missing or scanned "
                         "zero files (default: fail without touching the report)")
    args = ap.parse_args()

    smelt_bin = os.path.abspath(args.smelt_bin)
    config = json.load(open(args.config))
    libraries = config["libraries"]
    results, missing = [], []
    for lib in libraries:
        lib_dir = os.path.join(args.work_dir, lib["name"])
        if not os.path.isdir(lib_dir):
            missing.append(lib["name"])
            print(f"!! {lib['name']}: not checked out at {lib_dir}", file=sys.stderr)
            continue
        print(f">> probing {lib['name']} ...", file=sys.stderr)
        results.append(probe_library(smelt_bin, args.smelt_root, lib, args.work_dir, args.fixtures))

    problems = degenerate_probes(results, missing, len(libraries))
    if problems and not args.allow_degenerate:
        for p in problems:
            print(f"!! {p}", file=sys.stderr)
        print("!! refusing to write a report from a degenerate probe run; the previous "
              "report is left in place. Pass --allow-degenerate to override.", file=sys.stderr)
        return 1

    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    open(args.output, "w").write(render(results, args.date))
    print(f"wrote {args.output} ({len(results)} libraries)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
