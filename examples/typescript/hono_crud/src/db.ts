/**
 * SQLite connection setup for the todo API.
 *
 * Uses Node's built-in `node:sqlite` driver (Node 22+) directly with plain SQL.
 * There is no ORM: every statement in this app is a prepared statement, and no
 * value is ever interpolated into a SQL string.
 */
import { DatabaseSync } from 'node:sqlite';

/** DDL for the single table this app owns. Contains no user input. */
const CREATE_TODOS_TABLE = `
  CREATE TABLE IF NOT EXISTS todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    done INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
  )
`;

/**
 * Opens (or creates) the SQLite database at `path` and makes sure the `todos`
 * table exists. Pass `":memory:"` for a throwaway database, as the tests do.
 */
export function openDatabase(path: string): DatabaseSync {
  const db = new DatabaseSync(path);
  db.exec(CREATE_TODOS_TABLE);
  return db;
}
