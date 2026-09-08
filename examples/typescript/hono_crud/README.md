# hono_crud

A small, deliberately ordinary Hono 4 todo API in strict TypeScript. It is a
Smelt showcase input: the source a competent engineer would actually write for a
CRUD service on a modern `fetch`-based framework, which Smelt should transpile
into a working axum server.

Persistence is Node's built-in `node:sqlite` driver (Node 22+) used directly with
plain SQL — no ORM and no extra database dependency. Every statement is a prepared
statement; no value is interpolated into SQL.

## Layout

| File | Contents |
| --- | --- |
| `src/main.ts` | Entry point: opens `todos.db`, builds the app, serves it on `PORT` (default `3000`) via `@hono/node-server`. |
| `src/app.ts` | `createApp(db)` — the root `Hono` instance, the `/todos` sub-app, `notFound` and `onError`. |
| `src/db.ts` | `openDatabase(path)` — opens SQLite and creates the `todos` table. |
| `src/todos/model.ts` | `Todo`, `NewTodo`, `TodoPatch`, and the `rowToTodo` row mapper. |
| `src/todos/repository.ts` | `TodoRepository` — `list`, `get`, `create`, `update`, `remove`. |
| `src/todos/routes.ts` | The `/todos` sub-app plus hand-written body validation. |
| `test/todos.test.ts` | vitest driving `app.request(...)` against an in-memory database. |

## Routes

| Method | Path | Success | Errors |
| --- | --- | --- | --- |
| GET | `/todos` | `200` `Todo[]` | — |
| GET | `/todos/:id` | `200` `Todo` | `400` bad id, `404` missing |
| POST | `/todos` | `201` `Todo` | `400` invalid body |
| PATCH | `/todos/:id` | `200` `Todo` | `400` bad id or body, `404` missing |
| DELETE | `/todos/:id` | `204` | `400` bad id, `404` missing |

Every error response is `{ "error": string }`.

## Run and test

```sh
npm install
npm run typecheck   # tsc --noEmit over src + test
npm test            # vitest, in-memory database, no socket bound
npm run build       # tsc -> dist/
npm start           # node dist/main.js, writes ./todos.db
```

## Transpiling with Smelt

`Smelt.toml` declares `src/main.ts` as the single entry point and emits an axum
crate named `hono_crud` into `./dist-smelt`:

```sh
cargo run --bin smelt -- --manifest-path examples/typescript/hono_crud/Smelt.toml build
```

## Why Hono

The interesting part for Smelt is not the app but the dependency. Most Node web
frameworks ship as JavaScript with separate type declarations, so a transpiler
can only read their types and model the runtime. Hono is written in TypeScript
and publishes its own sources, so Smelt transpiles the framework itself —
generics, routers, `Context`, the whole tree — as ordinary input alongside this
app. That makes `hono_crud` the smallest honest end-to-end test of "transpile
the library too" rather than "model the library". The campaign notes for that
work live in `blocker-logs/hono-campaign-plan.md` on the Hono integration branch.
