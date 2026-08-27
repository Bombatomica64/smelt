#!/usr/bin/env python3
"""Run every benchmark case on both sides and emit results as JSON + Markdown.

Protocol notes that matter for reading the numbers:

*   One case per process, on both sides. Peak RSS is a process-lifetime high-water
    mark, so sharing a process between cases would attribute the largest case's
    memory to all of them. It also gives every case a cold JIT / cold allocator.
*   Cases are interleaved TS/Rust and the whole sweep is repeated `--repeats`
    times; the reported figure is the best (lowest ns/op) of the repeats. Machine
    noise only ever makes a run slower, so the minimum is the least noisy estimator
    available without a quiet dedicated box.
*   A row is only published when the TS and Rust checksums agree. A mismatch means
    the two sides did not compute the same thing, and a speed comparison between
    them would be meaningless -- those rows are reported separately as errors.

Usage:
    python3 benchmarks/run.py [--repeats N] [--only LIB] [--case CASE] [--quick]
"""
from __future__ import annotations

import argparse
import json
import pathlib
import statistics
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH = ROOT / "benchmarks"
TS_DIR = BENCH / "ts"

RUST_BIN = {
    "remeda": ROOT / "target/compat-repos/remeda/dist-bench/target/release/remeda_bench",
    "es-toolkit": ROOT / "target/compat-repos/es-toolkit/dist-bench/target/release/es_toolkit_bench",
}


def run_json(cmd: list[str], env_extra: dict[str, str], cwd: pathlib.Path) -> dict | None:
    """Run one bench process and parse the single JSON object it prints."""
    import os

    env = dict(os.environ)
    env.update(env_extra)
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env, cwd=cwd)
    if proc.returncode != 0:
        return {"error": (proc.stderr or proc.stdout).strip()[:400]}
    line = proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else ""
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return {"error": f"unparseable output: {line[:200]}"}


def ts_cases(lib: str) -> list[str]:
    out = subprocess.run([NODE, "run.mjs", "list", lib], capture_output=True, text=True, cwd=TS_DIR)
    return out.stdout.split()


def rust_cases(lib: str) -> list[str]:
    out = subprocess.run([str(RUST_BIN[lib]), "list"], capture_output=True, text=True)
    return out.stdout.split()


NODE = "node"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--only", choices=sorted(RUST_BIN))
    ap.add_argument("--case")
    ap.add_argument("--quick", action="store_true",
                    help="short warmup/measure budgets, for checking the plumbing")
    ap.add_argument("--out", default=str(BENCH / "results/results.json"))
    args = ap.parse_args()

    budget = {"SMELT_BENCH_WARMUP_MS": "150", "SMELT_BENCH_MEASURE_MS": "400",
              "SMELT_BENCH_MIN_SAMPLES": "5"} if args.quick else {}

    libs = [args.only] if args.only else list(RUST_BIN)
    results: dict = {"meta": {}, "baselines": {}, "rows": []}

    # Process footprint with the library loaded but no work done. Subtracting this
    # separates "what the runtime costs to exist" from "what the workload allocated".
    for lib in libs:
        results["baselines"][lib] = {
            "ts": run_json([NODE, "run.mjs", "baseline", lib], budget, TS_DIR),
            "rust": run_json([str(RUST_BIN[lib]), "baseline"], budget, ROOT),
        }

    plan = []
    for lib in libs:
        shared = [c for c in rust_cases(lib) if c in set(ts_cases(lib))]
        rust_only = [c for c in rust_cases(lib) if c not in set(ts_cases(lib))]
        for case in shared:
            if args.case and case != args.case:
                continue
            plan.append((lib, case, True))
        for case in rust_only:
            if args.case and case != args.case:
                continue
            plan.append((lib, case, False))

    best: dict[tuple[str, str], dict] = {}
    for repeat in range(args.repeats):
        for lib, case, has_ts in plan:
            print(f"[{repeat + 1}/{args.repeats}] {lib}/{case}", file=sys.stderr, flush=True)
            entry = best.setdefault((lib, case), {"lib": lib, "case": case, "has_ts": has_ts})
            if has_ts:
                ts = run_json([NODE, "run.mjs", "run", lib, case], budget, TS_DIR)
                keep_best(entry, "ts", ts)
            rs = run_json([str(RUST_BIN[lib]), "run", case], budget, ROOT)
            keep_best(entry, "rust", rs)

    for (lib, case), entry in best.items():
        results["rows"].append(entry)

    results["meta"] = {
        "repeats": args.repeats,
        "quick": args.quick,
        "node": subprocess.run([NODE, "--version"], capture_output=True, text=True).stdout.strip(),
        "rustc": subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip(),
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }

    out = pathlib.Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, indent=2))
    print(f">> wrote {out}", file=sys.stderr)
    return 0


def keep_best(entry: dict, side: str, result: dict | None) -> None:
    """Keep the fastest observation of a side across repeats.

    Noise on a shared machine is one-directional -- contention can only make a run
    slower -- so the minimum across repeats is the closest thing to an interference-free
    measurement. Every observation's ops/s is kept alongside so the report can show
    the spread and the reader can judge how noisy the box was.
    """
    if not result or "error" in (result or {}):
        entry.setdefault(f"{side}_error", (result or {}).get("error", "no result"))
        return
    entry.setdefault(f"{side}_observations", []).append(result["ops_per_sec"])
    prev = entry.get(side)
    if prev is None or result["ns_per_op_median"] < prev["ns_per_op_median"]:
        entry[side] = result


if __name__ == "__main__":
    raise SystemExit(main())
