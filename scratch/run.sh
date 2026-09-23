#!/usr/bin/env bash
# usage: run.sh <dir-with-input.ts>
set -u
W=/home/user/smelt/.claude/worktrees/agent-aa8e2a786664e0a39
d=$(realpath "$1"); p=$d/proj; rm -rf "$p"; mkdir -p "$p/src"; cp "$d/input.ts" "$p/src/main.ts"
cat > "$p/Smelt.toml" <<T
[project]
name = "example-app"
version = "0.1.0"
[sources]
entries = ["src/main.ts"]
[output]
target = "./dist"
crate-name = "example_app"
build = false
[runtime]
clone-strategy = "aggressive"
T
$W/target-priv/debug/smelt --manifest-path "$p/Smelt.toml" build 2>&1 | tail -5
cd "$p/dist" && CARGO_TARGET_DIR=$W/target-gen-priv cargo run -q 2>&1 | grep -vE "^warning|^ *= |^ *\||^ *-->|^$" | head -${2:-40}
