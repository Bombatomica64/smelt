/**
 * Hono application wiring.
 *
 * `createApp` takes an already-open database so tests can hand it an in-memory
 * one; nothing in this module reads configuration or opens sockets. The value
 * it returns is a plain `Hono` instance, so tests can drive it through
 * `app.request(...)` without binding a socket at all.
 */
import { Hono, type Context } from 'hono';
import type { DatabaseSync } from 'node:sqlite';
import { TodoRepository } from './todos/repository.js';
import { createTodosRouter } from './todos/routes.js';

/** Builds the Hono app backed by `db`. */
export function createApp(db: DatabaseSync): Hono {
  const app = new Hono();

  app.route('/todos', createTodosRouter(new TodoRepository(db)));

  app.notFound((c: Context): Response => c.json({ error: 'route not found' }, 404));

  app.onError((err: unknown, c: Context): Response => {
    const message = err instanceof Error ? err.message : 'internal server error';
    return c.json({ error: message }, 500);
  });

  return app;
}
