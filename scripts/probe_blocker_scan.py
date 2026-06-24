#!/usr/bin/env python3
"""Scan every source/test file of a probed library through `smelt dump-hir`
to aggregate the distinct unsupported-construct blocker classes per library.

Used to probe third-party "bug libraries" (see blocker-logs/bug-library-probes-*.md):
when `smelt build` aborts a whole-crate transpile at the first unsupported file,
this scanner runs every file independently to enumerate the *full* set of distinct
blocker classes so the report can categorize them (missing-stdlib vs non-working Rust).

Single-file mode does not resolve cross-file imports, so bare `unresolved name`
errors for locally-imported symbols are noise; genuine blockers (missing
stdlib builtins, unlowered constructs, missing annotations) are reported
reliably. We separate the two.

Usage:
    SMELT_BIN=target/debug/smelt SMELT_ROOT=$PWD \\
        python3 scripts/probe_blocker_scan.py <library-dir> <ts|py> [source-root ...]
"""
import subprocess, sys, re, os, json
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

# `smelt dump-hir` must run from the smelt repo root (it loads node/builtin data
# relative to the working directory); override via env when invoking elsewhere.
SMELT_ROOT = os.environ.get("SMELT_ROOT", os.getcwd())
SMELT = os.environ.get("SMELT_BIN", os.path.join(SMELT_ROOT, "target/debug/smelt"))
# dump-hir prints errors as a Rust Debug string with escaped inner quotes:
#   message: \"unresolved callback identifier `Number`\"
MSG_RE = re.compile(r'message: \\"(.*?)\\"')

def scan_file(path):
    path = os.path.abspath(path)
    try:
        r = subprocess.run([SMELT, "dump-hir", path], capture_output=True, text=True,
                           timeout=60, cwd=SMELT_ROOT)
    except subprocess.TimeoutExpired:
        return path, ["<timeout>"]
    out = r.stdout + r.stderr
    msgs = MSG_RE.findall(out)
    return path, msgs

def normalize(msg):
    """Collapse a message to its blocker class, keeping stdlib symbol names."""
    # keep the symbol for unresolved/unsupported builtin references
    m = re.search(r"(unresolved (?:callback )?(?:identifier|class|name)|unsupported.*?target|instanceof target) [`']([^`']+)[`']", msg)
    s = re.sub(r"'[^']*'", "'X'", msg)
    s = re.sub(r"`[^`]*`", "`X`", s)
    return s

def categorize(msg):
    stdlib_syms = ("Array","Number","String","Boolean","Object","Reflect","Math","Map","Set",
        "TextEncoder","TextDecoder","Promise","Date","JSON","RegExp","Symbol","BigInt","Error",
        "WeakMap","WeakSet","Proxy","Intl","ArrayBuffer","DataView","Iterator")
    if re.search(r"unresolved.*?(class|identifier|callback)", msg):
        for sym in stdlib_syms:
            if re.search(r"[`']"+re.escape(sym)+r"[`']", msg):
                return "missing-stdlib"
        # module-level builtins (sys, os, etc.) or genuinely missing -> could be import noise
        return "unresolved (maybe stdlib / import noise)"
    if "instanceof" in msg or "TextEncoder" in msg or "TextDecoder" in msg:
        return "missing-stdlib"
    return "transpiler-limitation"

def list_files(root, exts):
    out = []
    for dp, dn, fn in os.walk(root):
        if "node_modules" in dp or "/dist-" in dp or "/.git" in dp:
            continue
        for f in fn:
            if any(f.endswith(e) for e in exts) and not f.endswith(".d.ts"):
                out.append(os.path.join(dp, f))
    return out

def main():
    lib_dir = sys.argv[1]
    exts = [".py"] if sys.argv[2] == "py" else [".ts"]
    # restrict to source roots passed as remaining args
    roots = [os.path.join(lib_dir, r) for r in sys.argv[3:]] or [lib_dir]
    files = []
    for r in roots:
        if os.path.isdir(r):
            files += list_files(r, exts)
    files = sorted(set(files))
    cls_counter = Counter()      # blocker class -> count of occurrences
    cls_files = {}               # blocker class -> set of files
    cat_counter = Counter()      # category -> distinct classes
    failing_files = set()
    with ThreadPoolExecutor(max_workers=8) as ex:
        for path, msgs in ex.map(scan_file, files):
            if msgs:
                failing_files.add(path)
            for m in msgs:
                c = normalize(m)
                cls_counter[c] += 1
                cls_files.setdefault(c, set()).add(path)
    result = {
        "library": os.path.basename(lib_dir.rstrip("/")),
        "files_scanned": len(files),
        "files_with_errors": len(failing_files),
        "classes": []
    }
    for cls, n in cls_counter.most_common():
        result["classes"].append({
            "class": cls,
            "occurrences": n,
            "files": len(cls_files[cls]),
            "category": categorize(cls),
        })
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
