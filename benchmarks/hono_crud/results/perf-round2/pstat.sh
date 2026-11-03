#!/usr/bin/env bash
# pstat.sh <port> <cmd...>: perf stat user/kernel cycles+instructions over 8s of /todos/abc load; prints per-request.
port=$1; shift
(PORT=$port exec taskset -c 2 "$@" >/dev/null 2>&1) & pid=$!
curl -s -o /dev/null --retry-connrefused --retry 100 --retry-delay 0 http://127.0.0.1:$port/nope
taskset -c 4-7,12-15 ~/scratch/smelt/bin/oha -z 3s -c 32 --no-tui --output-format quiet http://127.0.0.1:$port/todos/abc >/dev/null 2>&1
taskset -c 4-7,12-15 ~/scratch/smelt/bin/oha -z 10s -c 32 --no-tui --output-format json http://127.0.0.1:$port/todos/abc > oha.json 2>/dev/null & op=$!
sudo perf stat -x, -e cycles:u,instructions:u,cycles:k,instructions:k,cache-misses -p $pid -o pstat.txt -- timeout 8 tail -f /dev/null
wait $op; rps=$(jq '.summary.requestsPerSec' oha.json 2>/dev/null)
kill $pid; wait $pid 2>/dev/null
awk -F, -v r="$rps" '/^[0-9]/{printf "%-16s %10.0f per req\n", $3, $1/(r*8)}' pstat.txt
