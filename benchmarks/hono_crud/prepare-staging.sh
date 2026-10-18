#!/usr/bin/env bash
# Build the hono_crud staging tree used by bench.sh and the generated-test run.
#
# The shipped example cannot be transpiled yet (node:sqlite is declared-only,
# @hono/node-server does not lower, and `hono` from npm ships no TypeScript), so
# the staging tree:
#   - vendors Hono's TypeScript source at the pinned compat ref as a workspace
#     package (`packages/hono`, resolved through its package.json `source`);
#   - swaps db.ts/repository.ts for an in-memory store with the same columns;
#   - serves app.fetch through serve.ts, a buffered node:http shim.
# Everything else is the shipped src/ and test/ unchanged.
#
# Usage: prepare-staging.sh <out-dir>
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
out=${1:?usage: prepare-staging.sh <out-dir>}
ref=$(python3 -c "import json; print(next(l['ref'] for l in json.load(open('$repo/.github/compat/libraries.json'))['libraries'] if l['name']=='hono'))")

mkdir -p "$out"
if [[ ! -d "$out/hono-src" ]]; then
  git clone -q --filter=blob:none https://github.com/honojs/hono.git "$out/hono-src"
fi
git -C "$out/hono-src" checkout -q "$ref"

app="$out/app"
rm -rf "$app"
mkdir -p "$app/packages/hono"
cp -r "$repo/examples/typescript/hono_crud/src" "$repo/examples/typescript/hono_crud/test" \
  "$repo/examples/typescript/hono_crud/package.json" "$repo/examples/typescript/hono_crud/tsconfig.json" "$app/"
cp -r "$out/hono-src/src" "$app/packages/hono/src"
echo '{"name":"hono","source":"src/index.ts"}' > "$app/packages/hono/package.json"
cp "$here/staging/db.ts" "$here/staging/app.ts" "$here/staging/main.ts" "$here/staging/serve.ts" "$app/src/"
cp "$here/staging/repository.ts" "$app/src/todos/repository.ts"
cat > "$app/Smelt.toml" <<'TOML'
[project]
name = "hono_crud_staging"
version = "0.1.0"
[sources]
entries = ["src/main.ts"]
roots = ["src", "test"]
test-prefix = ["**/*.test.ts"]
# Hono's RPC client (`new Proxy`) and its testing helper are outside the
# profile, exactly as in .github/compat/hono/Smelt.toml; the app imports neither.
exclude = ["packages/hono/src/client/**", "packages/hono/src/helper/testing/**"]
[output]
target = "./dist-smelt"
crate-name = "hono_crud_staging"
[runtime]
clone-strategy = "aggressive"
TOML
cat > "$app/tsconfig.build.json" <<'JSON'
{ "extends": "./tsconfig.json", "compilerOptions": { "rootDir": "src", "outDir": "dist" }, "include": ["src"] }
JSON
ln -sfn "$repo/examples/typescript/hono_crud/node_modules" "$app/node_modules"
echo "staging tree ready: $app"
