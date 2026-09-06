# express_crud

A small, deliberately ordinary Express 5 todo API in strict TypeScript. It is the
Smelt v1 showcase input: the source a competent engineer would actually write for
a CRUD service, which Smelt should transpile into a working axum server.

Persistence is Node's built-in `node:sqlite` driver (Node 22+) used directly with
plain SQL — no ORM and no extra database dependency. Every statement is a prepared
statement; no value is interpolated into SQL.

## Layout

| File | Contents |
| --- | --- |
| `src/main.ts` | Entry point: opens `todos.db`, builds the app, listens on `PORT` (default `3000`). |
| `src/app.ts` | `createApp(db)` — `express.json()`, the `/todos` router, a 404 handler and an error handler. |
| `src/db.ts` | `openDatabase(path)` — opens SQLite and creates the `todos` table. |
| `src/todos/model.ts` | `Todo`, `NewTodo`, `TodoPatch`, and the `rowToTodo` row mapper. |
| `src/todos/repository.ts` | `TodoRepository` — `list`, `get`, `create`, `update`, `remove`. |
| `src/todos/routes.ts` | The `/todos` router plus hand-written body validation. |
| `test/todos.test.ts` | vitest + supertest against an in-memory database. |

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
npm test            # vitest + supertest, in-memory database
npm run build       # tsc -> dist/
npm start           # node dist/main.js, writes ./todos.db
```

## Transpiling with Smelt

`Smelt.toml` declares `src/main.ts` as the single entry point and emits an axum
crate named `express_crud` into `./dist-smelt`:

```sh
cargo run --bin smelt -- --manifest-path examples/typescript/express_crud/Smelt.toml build
```
