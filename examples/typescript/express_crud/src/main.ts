/**
 * Process entry point: opens the database, builds the app, starts listening.
 */
import { createApp } from './app.js';
import { openDatabase } from './db.js';

const port = Number(process.env.PORT ?? 3000);
const db = openDatabase('todos.db');
const app = createApp(db);

app.listen(port, (): void => {
  console.log(`express_crud listening on http://localhost:${port}`);
});
