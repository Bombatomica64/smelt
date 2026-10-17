/** Staging entry point: in-memory store, served by the `node:http` shim. */
import { serve } from './serve.js';
import { createApp } from './app.js';
import { openDatabase } from './db.js';

const port = Number(process.env.PORT ?? 3000);
const app = createApp(openDatabase(':memory:'));

serve({ fetch: app.fetch, port });
