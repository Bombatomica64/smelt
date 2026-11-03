#!/usr/bin/env bash
# Paired A/B on one scenario: ab.sh <pairs> <expect> <path> [oha args...]
# Env A, B = server commands (B is the challenger). Both servers stay up (pinned to cpu 2,
# fresh db, 20 seeded rows); 10 s runs alternate A,B,A,B...; prints per-pair rps and B/A.
set -euo pipefail
pairs=$1 expect=$2 path=$3; shift 3
oha=$HOME/scratch/smelt/bin/oha pin=${LOADGEN_CPUS:-4-7,12-15} dur=${DUR:-8s}
start() { # label cmd port
  local wd=$HOME/scratch/smelt/r2/ab-$1; rm -rf "$wd"; mkdir -p "$wd"
  (cd "$wd" && PORT=$3 exec taskset -c 2 $2 >"$wd/server.log" 2>&1) &
  echo $! >"$wd/pid"
  curl -s -o /dev/null --retry-connrefused --retry 100 --retry-delay 0 --retry-max-time 20 "http://127.0.0.1:$3/nope"
  for i in $(seq 1 20); do curl -s -o /dev/null -X POST -H 'content-type: application/json' --data-binary "{\"title\":\"seed $i\"}" "http://127.0.0.1:$3/todos"; done
  taskset -c "$pin" "$oha" -z 4s -c 32 --no-tui --output-format quiet "$@" "http://127.0.0.1:$3$path" >/dev/null 2>&1 || true
}
one() { # port -> rps
  local json; json=$(taskset -c "$pin" "$oha" -z "$dur" -c 32 --no-tui --output-format json "${args[@]}" "http://127.0.0.1:$1$path")
  local mix; mix=$(jq -r '.statusCodeDistribution | keys | join(",")' <<<"$json")
  [[ $mix == "$expect" ]] || { echo "BROKEN $mix" >&2; }
  jq -r '.summary.requestsPerSec | floor' <<<"$json"
}
args=("$@")
start a "$A" 3921 "$@"; start b "$B" 3922 "$@"
trap 'kill $(cat ~/scratch/smelt/r2/ab-a/pid ~/scratch/smelt/r2/ab-b/pid) 2>/dev/null || true' EXIT
for p in $(seq 1 "$pairs"); do
  if (( p % 2 )); then a=$(one 3921); b=$(one 3922); else b=$(one 3922); a=$(one 3921); fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$path" "$a" "$b" "$(awk -v a=$a -v b=$b 'BEGIN{printf "%.3f", b/a}')" "$(cut -d' ' -f1 /proc/loadavg)"
done
