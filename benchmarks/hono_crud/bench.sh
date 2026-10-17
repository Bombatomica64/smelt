#!/usr/bin/env bash
# HTTP benchmark for a running hono_crud server; see README.md for the method.
#
# Usage: bench.sh <label> <base-url> <server-pid> [duration] [connections]
# Prints one TSV row per scenario:
#   label scenario rps p50_ms p99_ms status_mix vmhwm_kib vmrss_kib
# Requires `oha` (OHA env var or on PATH) and `jq`.
set -euo pipefail
label=$1 base=$2 pid=$3 dur=${4:-10s} conns=${5:-32}
oha=${OHA:-oha}
pin=${LOADGEN_CPUS:-6-11}

for i in $(seq 1 20); do
  curl -s -o /dev/null -X POST -H 'content-type: application/json' \
    --data-binary "{\"title\":\"seed $i\"}" "$base/todos"
done

run() {
  local name=$1 expect=$2; shift 2
  taskset -c "$pin" "$oha" -z 5s -c "$conns" --no-tui --output-format quiet "$@" >/dev/null 2>&1 || true
  local json
  json=$(taskset -c "$pin" "$oha" -z "$dur" -c "$conns" --no-tui --output-format json "$@")
  local rps p50 p99 mix
  rps=$(jq -r '.summary.requestsPerSec | floor' <<<"$json")
  p50=$(jq -r '.latencyPercentiles.p50 * 1000 | . * 1000 | round / 1000' <<<"$json")
  p99=$(jq -r '.latencyPercentiles.p99 * 1000 | . * 1000 | round / 1000' <<<"$json")
  mix=$(jq -r '.statusCodeDistribution | to_entries | map("\(.key):\(.value)") | join(",")' <<<"$json")
  if [[ "$mix" != "$expect:"* || "$mix" == *","* ]]; then mix="BROKEN($mix,expected $expect)"; fi
  local hwm rss
  hwm=$(awk '/VmHWM/{print $2}' "/proc/$pid/status")
  rss=$(awk '/VmRSS/{print $2}' "/proc/$pid/status")
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$label" "$name" "$rps" "$p50" "$p99" "$mix" "$hwm" "$rss"
}

run "GET /todos"      200 "$base/todos"
run "GET /todos/1"    200 "$base/todos/1"
run "GET /todos/abc"  400 "$base/todos/abc"
run "GET /nope"       404 "$base/nope"
run "PATCH /todos/1"  200 -m PATCH -T application/json -d '{"done":true}' "$base/todos/1"
run "POST /todos"     201 -m POST -T application/json -d '{"title":"bench"}' "$base/todos"
