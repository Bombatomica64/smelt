#!/usr/bin/env python3
"""Check out the third-party "bug libraries" probed by the daily library-probes job.

Each library in `.github/compat/libraries.json` is cloned into --work-dir at its
pinned ref. A checkout is reused only when it is *verifiably* the pinned ref with
a populated worktree; anything else is wiped and re-cloned.

That verification is the whole point of this script. The workflow previously
checked out into `target/library-probes` and skipped the clone whenever
`<dest>/.git` existed. `target/` is restored by `Swatinem/rust-cache`, which
brought the `.git` directories back but not the source trees -- so from
2026-08-07 every probe run skipped all 12 clones, scanned 0 files, and
auto-committed a report claiming every library failed to transpile. Checking for
`.git` alone cannot tell a real checkout from that husk.

Usage:
    python3 scripts/checkout_probe_libraries.py \
        --config .github/compat/libraries.json \
        --work-dir "$RUNNER_TEMP/library-probes"
"""
from __future__ import annotations
import argparse, json, os, shutil, subprocess, sys


def head_sha(dest: str) -> str | None:
    """Return the checked-out commit of `dest`, or None if it is not a repository."""
    r = subprocess.run(["git", "-C", dest, "rev-parse", "HEAD"],
                       capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def worktree_populated(dest: str, roots: list[str]) -> bool:
    """Report whether every configured source root exists and holds at least one file."""
    for root in roots:
        p = os.path.join(dest, root)
        if not os.path.isdir(p):
            return False
        if not any(files for _dp, _dn, files in os.walk(p)):
            return False
    return True


def is_usable(dest: str, lib: dict) -> bool:
    """Report whether `dest` is already the pinned ref with a populated worktree."""
    if not os.path.isdir(os.path.join(dest, ".git")):
        return False
    if head_sha(dest) != lib["ref"]:
        return False
    return worktree_populated(dest, lib["roots"])


def checkout(dest: str, lib: dict) -> None:
    """Clone `lib` into `dest` at its pinned ref, replacing whatever was there."""
    shutil.rmtree(dest, ignore_errors=True)
    url = f"https://github.com/{lib['repo']}.git"
    subprocess.run(["git", "clone", "--no-tags", "--filter=blob:none", url, dest], check=True)
    subprocess.run(["git", "-C", dest, "checkout", lib["ref"]], check=True)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--config", default=".github/compat/libraries.json")
    ap.add_argument("--work-dir", default=".library-probes")
    args = ap.parse_args()

    os.makedirs(args.work_dir, exist_ok=True)
    config = json.load(open(args.config))
    failed = []
    for lib in config["libraries"]:
        dest = os.path.join(args.work_dir, lib["name"])
        if is_usable(dest, lib):
            print(f">> {lib['name']}: reusing checkout at {lib['ref'][:12]}", file=sys.stderr)
            continue
        print(f">> {lib['name']}: cloning {lib['repo']} @ {lib['ref'][:12]}", file=sys.stderr)
        checkout(dest, lib)
        # Re-verify: a clone that "succeeded" but left an empty or wrong-ref
        # worktree must not reach the probe, which would read it as a total
        # transpile failure and publish that as a result.
        if not is_usable(dest, lib):
            failed.append(lib["name"])
            print(f"!! {lib['name']}: checkout is not at {lib['ref']} with populated "
                  f"roots {lib['roots']} after cloning", file=sys.stderr)

    if failed:
        print(f"!! unusable checkouts: {', '.join(failed)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
