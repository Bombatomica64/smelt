/**
 * In-memory TodoRepository with the same semantics as the SQL one (staging only).
 */
import type { MemDb } from '../db.js';
import { rowToTodo, type NewTodo, type Todo, type TodoPatch, type TodoRow } from './model.js';

/** CRUD operations over the in-memory `todos` table. */
export class TodoRepository {
  private readonly db: MemDb;

  constructor(db: MemDb) {
    this.db = db;
  }

  /** Returns every todo, oldest first. */
  list(): Todo[] {
    return this.db.rows.map((row: TodoRow): Todo => rowToTodo(row));
  }

  /** Returns the todo with `id`, or `undefined` when there is no such row. */
  get(id: number): Todo | undefined {
    const row = this.db.rows.find((r: TodoRow): boolean => r.id === id);
    return row === undefined ? undefined : rowToTodo(row);
  }

  /** Inserts a todo and returns it, including the assigned id. */
  create(input: NewTodo): Todo {
    const createdAt = new Date().toISOString();
    const id = this.db.nextId;
    this.db.nextId = id + 1;
    this.db.rows.push({ id, title: input.title, done: input.done ? 1 : 0, created_at: createdAt });
    return { id, title: input.title, done: input.done, createdAt };
  }

  /** Applies a partial update; `undefined` when `id` does not exist. */
  update(id: number, patch: TodoPatch): Todo | undefined {
    const current = this.get(id);
    if (current === undefined) {
      return undefined;
    }
    const title = patch.title ?? current.title;
    const done = patch.done ?? current.done;
    for (const row of this.db.rows) {
      if (row.id === id) {
        row.title = title;
        row.done = done ? 1 : 0;
      }
    }
    return { id, title, done, createdAt: current.createdAt };
  }

  /** Deletes the todo with `id` and reports whether a row was removed. */
  remove(id: number): boolean {
    const before = this.db.rows.length;
    this.db.rows = this.db.rows.filter((r: TodoRow): boolean => r.id !== id);
    return this.db.rows.length < before;
  }
}
