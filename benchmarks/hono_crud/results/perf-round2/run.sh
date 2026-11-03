#!/usr/bin/env bash
# Interleaved hono_crud bench: run.sh <rounds> <out.tsv> <label=cmd>...
# Each variant: fresh workdir + todos.db, server pinned to cpu 2, bench.sh.
set -euo pipefail
rounds=$1 out=$2; shift 2
repo=/home/lorenzo/.t3/worktrees/smelt/perf-hono-crud-round2
export OHA=$HOME/scratch/smelt/bin/oha LOADGEN_CPUS=${LOADGEN_CPUS:-4-7,12-15}
port=3917
for r in $(seq 1 "$rounds"); do
  for spec in "$@"; do
    label=${spec%%=*} cmd=${spec#*=}
    wd=$HOME/scratch/smelt/r2/wd-$label; rm -rf "$wd"; mkdir -p "$wd"
    (cd "$wd" && PORT=$port exec taskset -c 2 $cmd >"$wd/server.log" 2>&1) &
    pid=$!
    curl -s -o /dev/null --retry-connrefused --retry 100 --retry-delay 0 --retry-max-time 20 "http://127.0.0.1:$port/nope"
    spid=$(pgrep -P "$pid" || echo "$pid")
    echo "load $(cut -d' ' -f1-3 /proc/loadavg) $label-r$r" >>"${out%.tsv}-load.txt"
    "$repo/benchmarks/hono_crud/bench.sh" "$label-r$r" "http://127.0.0.1:$port" "$spid" | tee -a "$out"
    kill "$pid" 2>/dev/null; pkill -P "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true
  done
done
