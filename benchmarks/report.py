#!/usr/bin/env python3
"""Turn `benchmarks/results/results.json` into a readable Markdown report.

Reporting rules:

*   A row is published only when the TypeScript and Rust checksums agree. Rows that
    disagree are listed separately as parity failures -- if the two sides did not
    compute the same answer, comparing their speed says nothing.
*   Throughput ratio is Rust / TypeScript, so >1x means the generated Rust is faster
    and <1x means it is slower. The summary uses the geometric mean, which is the
    correct average for ratios.
*   Memory is reported twice: absolute process peak RSS, and peak minus the
    library-loaded-but-idle baseline. The first is what an operator sees; the second
    isolates what the workload itself allocated.

Usage:
    python3 benchmarks/report.py [--in results.json] [--out benchmarks/RESULTS.md]
"""
from __future__ import annotations

import argparse
import json
import math
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH = ROOT / "benchmarks"


def human_ops(v: float) -> str:
    """Format ops/s with a fixed number of significant digits, not decimals."""
    if v >= 1000:
        return f"{v:,.0f}"
    if v >= 10:
        return f"{v:.1f}"
    if v >= 1:
        return f"{v:.2f}"
    return f"{v:.3f}"


def mib(v: float | int | None) -> str:
    if v is None or v < 0:
        return "—"
    return f"{v / (1024 * 1024):.1f}"


def ratio_str(r: float) -> str:
    if r >= 1:
        return f"**{r:.2f}×**"
    return f"{r:.2f}× *({1 / r:.0f}× slower)*"


def spread(observations: list[float] | None) -> str:
    """Best/worst ops/s across repeats, as a percentage, to expose machine noise."""
    if not observations or len(observations) < 2:
        return "—"
    lo, hi = min(observations), max(observations)
    if lo <= 0:
        return "—"
    return f"{(hi / lo - 1) * 100:.0f}%"


def library_section(lib: str, rows: list[dict], baselines: dict) -> list[str]:
    out: list[str] = [f"### {lib}", ""]

    base_ts = (baselines.get(lib, {}).get("ts") or {}).get("peak_rss_bytes")
    base_rs = (baselines.get(lib, {}).get("rust") or {}).get("peak_rss_bytes")
    out += [
        "Idle footprint — the process with the library loaded and no work done:",
        "",
        "| Side | Peak RSS (MiB) |",
        "| --- | ---: |",
        f"| TypeScript (Node) | {mib(base_ts)} |",
        f"| Generated Rust | {mib(base_rs)} |",
        "",
    ]

    paired = [r for r in rows if r.get("ts") and r.get("rust")]
    matched = [r for r in paired if r["ts"]["checksum"] == r["rust"]["checksum"]]
    mismatched = [r for r in paired if r["ts"]["checksum"] != r["rust"]["checksum"]]
    rust_only = [r for r in rows if r.get("rust") and not r.get("ts")]

    out += [
        "| Case | TS ops/s | Rust ops/s | Rust / TS | TS peak RSS | Rust peak RSS | "
        "TS workload MiB | Rust workload MiB | TS noise | Rust noise |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for r in matched:
        ts, rs = r["ts"], r["rust"]
        ratio = rs["ops_per_sec"] / ts["ops_per_sec"]
        ts_delta = ts["peak_rss_bytes"] - base_ts if base_ts else None
        rs_delta = rs["peak_rss_bytes"] - base_rs if base_rs else None
        out.append(
            f"| `{r['case']}` | {human_ops(ts['ops_per_sec'])} | {human_ops(rs['ops_per_sec'])} | "
            f"{ratio_str(ratio)} | {mib(ts['peak_rss_bytes'])} | {mib(rs['peak_rss_bytes'])} | "
            f"{mib(ts_delta)} | {mib(rs_delta)} | {spread(r.get('ts_observations'))} | "
            f"{spread(r.get('rust_observations'))} |"
        )
    out.append("")

    if matched:
        ratios = [r["rust"]["ops_per_sec"] / r["ts"]["ops_per_sec"] for r in matched]
        geo = math.exp(sum(math.log(x) for x in ratios) / len(ratios))
        faster = sum(1 for x in ratios if x > 1)
        out += [
            f"**Geometric mean throughput ratio: {geo:.2f}×** "
            f"(generated Rust is faster in {faster} of {len(ratios)} cases; "
            f"range {min(ratios):.2f}×–{max(ratios):.2f}×).",
            "",
        ]

    if rust_only:
        out += ["Rust-only variants (no TypeScript twin — see the notes):", "",
                "| Case | Rust ops/s | Rust peak RSS |", "| --- | ---: | ---: |"]
        for r in rust_only:
            rs = r["rust"]
            out.append(f"| `{r['case']}` | {human_ops(rs['ops_per_sec'])} | "
                       f"{mib(rs['peak_rss_bytes'])} |")
        out.append("")

    if mismatched:
        out += ["**Parity failures** (excluded from the table above — the two sides "
                "did not compute the same result):", ""]
        for r in mismatched:
            out.append(f"- `{r['case']}`: TS checksum {r['ts']['checksum']}, "
                       f"Rust checksum {r['rust']['checksum']}")
        out.append("")

    errors = [r for r in rows if r.get("ts_error") or r.get("rust_error")]
    if errors:
        out += ["**Failures**:", ""]
        for r in errors:
            for side in ("ts", "rust"):
                if r.get(f"{side}_error"):
                    out.append(f"- `{r['case']}` ({side}): {r[f'{side}_error']}")
        out.append("")

    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--in", dest="src", default=str(BENCH / "results/results.json"))
    ap.add_argument("--out", default=str(BENCH / "RESULTS.md"))
    args = ap.parse_args()

    data = json.loads(pathlib.Path(args.src).read_text())
    meta = data["meta"]
    by_lib: dict[str, list[dict]] = {}
    for row in data["rows"]:
        by_lib.setdefault(row["lib"], []).append(row)

    lines = [
        "# TypeScript vs Smelt-generated Rust: library benchmarks",
        "",
        "_Generated by `benchmarks/report.py` from `benchmarks/results/results.json`._",
        "",
        f"- Generated: {meta['generated_at']}",
        f"- Repeats: {meta['repeats']} (each figure is the fastest of the repeats)",
        f"- Node: {meta['node']}",
        f"- rustc: {meta['rustc']}",
        "",
        "See `benchmarks/README.md` for the methodology and for how to reproduce this.",
        "",
    ]
    for lib in sorted(by_lib):
        lines += library_section(lib, by_lib[lib], data.get("baselines", {}))

    out = pathlib.Path(args.out)
    out.write_text("\n".join(lines))
    print(f">> wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
