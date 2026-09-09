/**
 * Process entry point: opens the database, builds the app, starts serving it.
 *
 * Hono itself is runtime-agnostic, so the Node adapter is the only place in the
 * app that knows it is running on Node.
 */
import { serve } from '@hono/node-server';
import { createApp } from './app.js';
import { openDatabase } from './db.js';

const port = Number(process.env.PORT ?? 3000);
const db = openDatabase('todos.db');
const app = createApp(db);

serve({ fetch: app.fetch, port }, (): void => {
  console.log(`hono_crud listening on http://localhost:${port}`);
});
