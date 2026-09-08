/**
 * HTTP routes for `/todos`, as a mountable Hono sub-app.
 *
 * Request bodies arrive as parsed JSON, which is the one genuinely dynamic
 * boundary in this app: each body is validated by hand into a concrete
 * `NewTodo`/`TodoPatch` before it reaches the repository, so no untyped value
 * travels any further.
 */
import { Hono, type Context, type Env } from 'hono';
import type { NewTodo, TodoPatch } from './model.js';
import type { TodoRepository } from './repository.js';

/**
 * The context of a handler registered on `/:id`. Naming the path in the type
 * is what makes `c.req.param('id')` a plain `string` instead of a possibly
 * absent one, so no route has to re-check what the router already matched.
 */
type IdContext = Context<Env, '/:id'>;

/** Outcome of validating a request body into a concrete shape. */
type Validated<T> = { ok: true; value: T } | { ok: false; error: string };

/**
 * Parses a `:id` path segment into a positive integer, or `undefined` when the
 * segment is not one.
 */
function parseId(raw: string): number | undefined {
  const id = Number(raw);
  return Number.isInteger(id) && id > 0 ? id : undefined;
}

/**
 * Validates a POST body: `title` must be a non-empty string, `done` is an
 * optional boolean that defaults to `false`.
 */
function validateNewTodo(body: unknown): Validated<NewTodo> {
  if (typeof body !== 'object' || body === null) {
    return { ok: false, error: 'body must be a JSON object' };
  }
  const candidate = body as { title?: unknown; done?: unknown };
  if (typeof candidate.title !== 'string' || candidate.title.trim() === '') {
    return { ok: false, error: 'title must be a non-empty string' };
  }
  if (candidate.done !== undefined && typeof candidate.done !== 'boolean') {
    return { ok: false, error: 'done must be a boolean' };
  }
  return { ok: true, value: { title: candidate.title, done: candidate.done ?? false } };
}

/**
 * Validates a PATCH body: `title` and `done` are both optional but at least
 * one of them must be present.
 */
function validateTodoPatch(body: unknown): Validated<TodoPatch> {
  if (typeof body !== 'object' || body === null) {
    return { ok: false, error: 'body must be a JSON object' };
  }
  const candidate = body as { title?: unknown; done?: unknown };
  const patch: TodoPatch = {};
  if (candidate.title !== undefined) {
    if (typeof candidate.title !== 'string' || candidate.title.trim() === '') {
      return { ok: false, error: 'title must be a non-empty string' };
    }
    patch.title = candidate.title;
  }
  if (candidate.done !== undefined) {
    if (typeof candidate.done !== 'boolean') {
      return { ok: false, error: 'done must be a boolean' };
    }
    patch.done = candidate.done;
  }
  if (patch.title === undefined && patch.done === undefined) {
    return { ok: false, error: 'patch must set title or done' };
  }
  return { ok: true, value: patch };
}

/** Builds the sub-app mounted at `/todos`. */
export function createTodosRouter(repository: TodoRepository): Hono {
  const router = new Hono();

  router.get('/', (c: Context): Response => c.json(repository.list()));

  router.get('/:id', (c: IdContext): Response => {
    const id = parseId(c.req.param('id'));
    if (id === undefined) {
      return c.json({ error: 'id must be a positive integer' }, 400);
    }
    const todo = repository.get(id);
    if (todo === undefined) {
      return c.json({ error: 'todo not found' }, 404);
    }
    return c.json(todo);
  });

  router.post('/', async (c: Context): Promise<Response> => {
    const body: unknown = await c.req.json();
    const validated = validateNewTodo(body);
    if (!validated.ok) {
      return c.json({ error: validated.error }, 400);
    }
    return c.json(repository.create(validated.value), 201);
  });

  router.patch('/:id', async (c: IdContext): Promise<Response> => {
    const id = parseId(c.req.param('id'));
    if (id === undefined) {
      return c.json({ error: 'id must be a positive integer' }, 400);
    }
    const body: unknown = await c.req.json();
    const validated = validateTodoPatch(body);
    if (!validated.ok) {
      return c.json({ error: validated.error }, 400);
    }
    const updated = repository.update(id, validated.value);
    if (updated === undefined) {
      return c.json({ error: 'todo not found' }, 404);
    }
    return c.json(updated);
  });

  router.delete('/:id', (c: IdContext): Response => {
    const id = parseId(c.req.param('id'));
    if (id === undefined) {
      return c.json({ error: 'id must be a positive integer' }, 400);
    }
    if (!repository.remove(id)) {
      return c.json({ error: 'todo not found' }, 404);
    }
    return c.body(null, 204);
  });

  return router;
}
