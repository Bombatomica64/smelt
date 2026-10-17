/**
 * In-memory stand-in for the node:sqlite database (staging diagnostic only).
 * Holds `todos` rows with the same column shape and AUTOINCREMENT id rule.
 */
import type { TodoRow } from './todos/model.js';

/** The in-memory `todos` table. */
export class MemDb {
  rows: TodoRow[] = [];
  nextId = 1;
}

/** Opens a fresh in-memory table; `path` is ignored. */
export function openDatabase(path: string): MemDb {
  void path;
  return new MemDb();
}
