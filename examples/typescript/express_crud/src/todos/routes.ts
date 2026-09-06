/**
 * HTTP routes for `/todos`.
 *
 * Request bodies arrive as parsed JSON, which is the one genuinely dynamic
 * boundary in this app: each body is validated by hand into a concrete
 * `NewTodo`/`TodoPatch` before it reaches the repository, so no untyped value
 * travels any further.
 */
import { Router, type Request, type Response } from 'express';
import type { NewTodo, Todo, TodoPatch } from './model.js';
import type { TodoRepository } from './repository.js';

/** The JSON body returned for every failed request. */
interface ErrorBody {
  error: string;
}

/** Route parameters for the `/:id` routes. */
interface IdParams {
  id: string;
}

/** Routes that take no route parameters. */
type NoParams = Record<string, never>;

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

/** Builds the router mounted at `/todos`. */
export function createTodosRouter(repository: TodoRepository): Router {
  const router = Router();

  router.get('/', (_req: Request<NoParams, Todo[]>, res: Response<Todo[]>): void => {
    res.json(repository.list());
  });

  router.get(
    '/:id',
    (req: Request<IdParams, Todo | ErrorBody>, res: Response<Todo | ErrorBody>): void => {
      const id = parseId(req.params.id);
      if (id === undefined) {
        res.status(400).json({ error: 'id must be a positive integer' });
        return;
      }
      const todo = repository.get(id);
      if (todo === undefined) {
        res.status(404).json({ error: 'todo not found' });
        return;
      }
      res.json(todo);
    },
  );

  router.post(
    '/',
    (req: Request<NoParams, Todo | ErrorBody, unknown>, res: Response<Todo | ErrorBody>): void => {
      const validated = validateNewTodo(req.body);
      if (!validated.ok) {
        res.status(400).json({ error: validated.error });
        return;
      }
      res.status(201).json(repository.create(validated.value));
    },
  );

  router.patch(
    '/:id',
    (req: Request<IdParams, Todo | ErrorBody, unknown>, res: Response<Todo | ErrorBody>): void => {
      const id = parseId(req.params.id);
      if (id === undefined) {
        res.status(400).json({ error: 'id must be a positive integer' });
        return;
      }
      const validated = validateTodoPatch(req.body);
      if (!validated.ok) {
        res.status(400).json({ error: validated.error });
        return;
      }
      const updated = repository.update(id, validated.value);
      if (updated === undefined) {
        res.status(404).json({ error: 'todo not found' });
        return;
      }
      res.json(updated);
    },
  );

  router.delete(
    '/:id',
    (req: Request<IdParams, ErrorBody>, res: Response<ErrorBody>): void => {
      const id = parseId(req.params.id);
      if (id === undefined) {
        res.status(400).json({ error: 'id must be a positive integer' });
        return;
      }
      if (!repository.remove(id)) {
        res.status(404).json({ error: 'todo not found' });
        return;
      }
      res.status(204).end();
    },
  );

  return router;
}
