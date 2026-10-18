#!/usr/bin/env bash
# Route conformance probe for examples/typescript/hono_crud.
#
# Drives a fixed request sequence against a freshly started server (empty todos.db)
# and prints status line, response headers (minus `Date`, which is wall-clock) and
# body for each request. Run once against Node and once against the generated axum
# server, then `diff` the two outputs: they should be byte-identical.
#
# `createdAt` is wall-clock too, so ISO timestamps are normalised to <ts>.
#
# Usage: probe.sh http://127.0.0.1:3000
set -u
base=${1:-http://127.0.0.1:3000}
req() {
  local method=$1 path=$2 body=${3-}
  echo "### $method $path ${body:+body=$body}"
  local args=(-s -i -X "$method" "$base$path")
  if [[ -n $body ]]; then args+=(-H 'content-type: application/json' --data-binary "$body"); fi
  curl "${args[@]}" | tr -d '\r' | grep -iv '^date:' \
    | sed -E 's/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.]+Z/<ts>/g'
  echo
}
req GET /todos
req POST /todos '{"title":"buy milk"}'
req POST /todos '{"title":"walk dog","done":true}'
req POST /todos '{"title":""}'
req POST /todos '{"title":"x","done":"yes"}'
req POST /todos '[1,2]'
req POST /todos 'null'
req POST /todos '{bad json'
req GET /todos
req GET /todos/1
req GET /todos/2
req GET /todos/999
req GET /todos/abc
req GET /todos/0
req GET /todos/1.5
req GET /todos/-3
req PATCH /todos/1 '{"done":true}'
req PATCH /todos/1 '{"title":"buy oat milk"}'
req PATCH /todos/1 '{}'
req PATCH /todos/1 '{"title":3}'
req PATCH /todos/1 '{"done":"no"}'
req PATCH /todos/999 '{"done":true}'
req PATCH /todos/abc '{"done":true}'
req DELETE /todos/2
req DELETE /todos/2
req DELETE /todos/abc
req GET /todos
req GET /nope
req PUT /todos/1 '{"title":"x"}'
req GET /todos/
