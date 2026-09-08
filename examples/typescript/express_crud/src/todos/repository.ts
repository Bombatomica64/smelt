/**
 * Data access for todos.
 *
 * Every query is a prepared statement with bound parameters. The only place
 * untyped data enters the app is the row objects `node:sqlite` returns, which
 * are asserted to {@link TodoRow} once here and mapped to `Todo` immediately.
 */
import type { DatabaseSync } from 'node:sqlite';
import { rowToTodo, type NewTodo, type Todo, type TodoPatch, type TodoRow } from './model.js';

/** CRUD operations over the `todos` table. */
export class TodoRepository {
  private readonly db: DatabaseSync;

  constructor(db: DatabaseSync) {
    this.db = db;
  }

  /** Returns every todo, oldest first. */
  list(): Todo[] {
    const statement = this.db.prepare('SELECT id, title, done, created_at FROM todos ORDER BY id');
    const rows = statement.all() as TodoRow[];
    return rows.map((row: TodoRow): Todo => rowToTodo(row));
  }

  /** Returns the todo with `id`, or `undefined` when there is no such row. */
  get(id: number): Todo | undefined {
    const statement = this.db.prepare('SELECT id, title, done, created_at FROM todos WHERE id = ?');
    const row = statement.get(id) as TodoRow | undefined;
    return row === undefined ? undefined : rowToTodo(row);
  }

  /** Inserts a todo and returns it, including the id SQLite assigned. */
  create(input: NewTodo): Todo {
    const createdAt = new Date().toISOString();
    const statement = this.db.prepare(
      'INSERT INTO todos (title, done, created_at) VALUES (?, ?, ?)',
    );
    const result = statement.run(input.title, input.done ? 1 : 0, createdAt);
    return {
      id: Number(result.lastInsertRowid),
      title: input.title,
      done: input.done,
      createdAt,
    };
  }

  /**
   * Applies a partial update and returns the stored todo, or `undefined` when
   * `id` does not exist. Fields absent from `patch` keep their current value.
   */
  update(id: number, patch: TodoPatch): Todo | undefined {
    const current = this.get(id);
    if (current === undefined) {
      return undefined;
    }
    const title = patch.title ?? current.title;
    const done = patch.done ?? current.done;
    const statement = this.db.prepare('UPDATE todos SET title = ?, done = ? WHERE id = ?');
    statement.run(title, done ? 1 : 0, id);
    return { id, title, done, createdAt: current.createdAt };
  }

  /** Deletes the todo with `id` and reports whether a row was removed. */
  remove(id: number): boolean {
    const statement = this.db.prepare('DELETE FROM todos WHERE id = ?');
    const result = statement.run(id);
    return Number(result.changes) > 0;
  }
}
