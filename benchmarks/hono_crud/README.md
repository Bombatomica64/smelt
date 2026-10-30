# hono_crud HTTP benchmark

Defined before any measurement was taken (CLAUDE.md "Audit evidence": the
benchmark and acceptance criteria come first). `bench.sh` is the harness;
`probe.sh` is the byte-for-byte route conformance probe.

## What is compared

| Variant | Runtime | App | Storage | HTTP layer |
| --- | --- | --- | --- | --- |
| `node-original` | Node 24 | `examples/typescript/hono_crud` as shipped (`npm run build && npm start`) | `node:sqlite` file `todos.db` | `@hono/node-server` |
| `node-staging` | Node 24 | staging variant (below), compiled by `tsc` | in-memory array | `serve.ts` shim over `node:http` |
| `rust-staging` | generated crate, `cargo build --release` | the same staging variant, transpiled by `smelt build` | in-memory array | the same `serve.ts` shim, lowered onto Smelt's hyper-backed `node:http` |
| `rust-shipped` | generated crate, `cargo build --release` | `examples/typescript/hono_crud` as shipped, transpiled by `smelt build` (Hono from the `[packages.hono]` pin in its `Smelt.toml`, which since perf round 1 also sets `[rust] allocator = "mimalloc"`; see `blocker-logs/hono-crud-perf-round1.md`) | `node:sqlite` file `todos.db` (rusqlite, bundled SQLite) | `@hono/node-server` `serve()`, modeled natively over the same `node:http` server |

`node-staging` vs `rust-staging` is one apples-to-apples pair: identical
TypeScript source on both sides. `node-original` vs `rust-shipped` is the other:
the shipped app, unmodified, on Node and as generated Rust, both on Hono
**4.13.7** (what `npm` installs for the example's `^4.13.0`, and what its
`Smelt.toml` pins). The two pairs do not run the same Hono: the staging tree
takes Hono from `.github/compat/libraries.json`, which is now **5.0.0-rc.1**
(see `blocker-logs/hono-crud-status.md` §11 for a leak only Hono 4 reached).

The staging variant is the shipped `src/` with two substitutions, kept in the
staging script `prepare-staging.sh`: `db.ts`/`repository.ts` use an in-memory
row array with the same column shape and AUTOINCREMENT rule, and `main.ts`
serves `app.fetch` through `serve.ts` (buffered request in, buffered response
out). Hono itself is the source pinned in `.github/compat/libraries.json`
(5.0.0-rc.1 since `4c1e9cae`, 2026-10-16; v4.13.7, `eebdf7be`, before that).

## Method

- Machine: 12-core AMD EPYC-Genoa VM, Linux 6.8. The server is pinned to ONE
  core (`taskset -c 2`): Node runs JavaScript on one thread and the generated
  server runs on a current-thread tokio runtime, so one core is the fair
  budget. The load generator (`oha` 1.16) is pinned to cores 6-11.
- Every scenario: 5 s warm-up (discarded), then 10 s at 32 keep-alive
  connections over HTTP/1.1. Fresh server process per variant; the store is
  seeded with 20 todos before the first scenario.
- Scenarios, in this order (writes last, so reads see a fixed store):
  1. `GET /todos` — list 20 rows, JSON-serialize an array.
  2. `GET /todos/1` — param route, lookup, JSON object.
  3. `GET /todos/abc` — param route, validation error, 400.
  4. `GET /nope` — router miss, `notFound` handler, 404.
  5. `PATCH /todos/1 {"done":true}` — body parse, validation, update.
  6. `POST /todos {"title":"bench"}` — body parse, validation, insert (grows
     the store; last for that reason).
- Metrics: requests/s, p50 and p99 latency from `oha --output-format json`,
  non-2xx/4xx-as-expected counts (a scenario whose status mix differs from the
  expected status is reported as broken, not as a number), and peak RSS
  (`VmHWM`) plus resident RSS after the run (`VmRSS`) from `/proc/<pid>/status`.

## Acceptance criteria for optimisation work

A change counts as a perf improvement only if, on this benchmark:

- `rust-staging` requests/s improves by at least 10 % on scenario 1 or 2 across
  three runs (median), with no scenario regressing by more than 5 %;
- the staging vitest suite still passes 9/9 on the generated crate, and
  `probe.sh` output against the generated server is unchanged;
- the SmeltUnknown report for the examples corpus keeps avoidable erasure at 0.
