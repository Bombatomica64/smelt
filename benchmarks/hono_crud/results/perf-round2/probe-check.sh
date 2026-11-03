#!/usr/bin/env bash
# probe.sh <binary>: fresh dir + db, run benchmarks/hono_crud/probe.sh, diff against committed output.
repo=/home/lorenzo/.t3/worktrees/smelt/perf-hono-crud-round2
wd=$HOME/scratch/smelt/r2/probe-wd; rm -rf "$wd"; mkdir -p "$wd"
(cd "$wd" && PORT=3940 exec "$1" >/dev/null 2>&1) & pid=$!
curl -s -o /dev/null --retry-connrefused --retry 100 --retry-delay 0 http://127.0.0.1:3940/__warm 
rm -f "$wd/todos.db"; kill $pid; wait $pid 2>/dev/null
(cd "$wd" && PORT=3940 exec "$1" >/dev/null 2>&1) & pid=$!
curl -s -o /dev/null --retry-connrefused --retry 100 --retry-delay 0 http://127.0.0.1:3940/__warm
"$repo/benchmarks/hono_crud/probe.sh" http://127.0.0.1:3940 > "$wd/probe.txt"
kill $pid; wait $pid 2>/dev/null
diff "$repo/benchmarks/hono_crud/results/probe-rust-shipped.txt" "$wd/probe.txt" && echo PROBE-IDENTICAL
