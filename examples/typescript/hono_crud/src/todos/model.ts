/**
 * Todo domain types and the mapping between SQLite rows and the API shape.
 *
 * SQLite has no boolean column type, so `done` is stored as an integer and
 * converted at this single boundary; everything above it sees a real `boolean`.
 */

/** A todo as the HTTP API exposes it. */
export interface Todo {
  id: number;
  title: string;
  done: boolean;
  createdAt: string;
}

/** The fields a client may supply when creating a todo. */
export interface NewTodo {
  title: string;
  done: boolean;
}

/** The fields a client may supply when patching a todo. Both are optional. */
export interface TodoPatch {
  title?: string;
  done?: boolean;
}

/** One row of the `todos` table, exactly as the SQL columns are named. */
export type TodoRow = {
  id: number;
  title: string;
  done: number;
  created_at: string;
};

/** Converts a `todos` row into the API-facing {@link Todo} shape. */
export function rowToTodo(row: TodoRow): Todo {
  return {
    id: row.id,
    title: row.title,
    done: row.done !== 0,
    createdAt: row.created_at,
  };
}
