/**
 * End-to-end route tests: a fresh in-memory database per test, driven through
 * the real Hono app with `app.request(...)`, so no socket is ever bound.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import type { Hono } from 'hono';
import { createApp } from '../src/app.js';
import { openDatabase } from '../src/db.js';
import type { Todo } from '../src/todos/model.js';

/** The JSON body every failed request returns. */
interface ErrorBody {
  error: string;
}

/** Base URL for the synthetic `Request` objects the tests build. */
const BASE = 'http://localhost';

/** Builds a JSON request for `method path` with `body` as its payload. */
function jsonRequest(method: string, path: string, body: unknown): Request {
  return new Request(`${BASE}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

describe('todos API', () => {
  let app: Hono;

  beforeEach((): void => {
    app = createApp(openDatabase(':memory:'));
  });

  /** POSTs a todo and returns it, failing the test if creation did not work. */
  async function createTodo(body: unknown): Promise<Todo> {
    const response = await app.request(jsonRequest('POST', '/todos', body));
    expect(response.status).toBe(201);
    return (await response.json()) as Todo;
  }

  it('starts empty', async (): Promise<void> => {
    const response = await app.request(new Request(`${BASE}/todos`));
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual([]);
  });

  it('creates a todo', async (): Promise<void> => {
    const todo = await createTodo({ title: 'buy milk' });
    expect(todo.id).toBe(1);
    expect(todo.title).toBe('buy milk');
    expect(todo.done).toBe(false);
    expect(typeof todo.createdAt).toBe('string');
  });

  it('reads a todo back', async (): Promise<void> => {
    const created = await createTodo({ title: 'walk dog' });
    const response = await app.request(new Request(`${BASE}/todos/${created.id}`));
    expect(response.status).toBe(200);
    expect(((await response.json()) as Todo).title).toBe('walk dog');
  });

  it('lists created todos', async (): Promise<void> => {
    await createTodo({ title: 'first' });
    await createTodo({ title: 'second', done: true });
    const response = await app.request(new Request(`${BASE}/todos`));
    expect(response.status).toBe(200);
    const todos = (await response.json()) as Todo[];
    expect(todos).toHaveLength(2);
    expect(todos[1]?.done).toBe(true);
  });

  it('patches a todo', async (): Promise<void> => {
    const created = await createTodo({ title: 'draft' });
    const response = await app.request(
      jsonRequest('PATCH', `/todos/${created.id}`, { done: true }),
    );
    expect(response.status).toBe(200);
    const patched = (await response.json()) as Todo;
    expect(patched.title).toBe('draft');
    expect(patched.done).toBe(true);
  });

  it('deletes a todo', async (): Promise<void> => {
    const created = await createTodo({ title: 'temporary' });
    const deleted = await app.request(
      new Request(`${BASE}/todos/${created.id}`, { method: 'DELETE' }),
    );
    expect(deleted.status).toBe(204);
    const after = await app.request(new Request(`${BASE}/todos/${created.id}`));
    expect(after.status).toBe(404);
  });

  it('rejects a blank title with 400', async (): Promise<void> => {
    const response = await app.request(jsonRequest('POST', '/todos', { title: '   ' }));
    expect(response.status).toBe(400);
    expect(((await response.json()) as ErrorBody).error).toBe('title must be a non-empty string');
  });

  it('returns 404 for a missing todo', async (): Promise<void> => {
    const response = await app.request(new Request(`${BASE}/todos/999`));
    expect(response.status).toBe(404);
    expect(((await response.json()) as ErrorBody).error).toBe('todo not found');
  });

  it('returns 404 for an unmatched route', async (): Promise<void> => {
    const response = await app.request(new Request(`${BASE}/nope`));
    expect(response.status).toBe(404);
    expect(((await response.json()) as ErrorBody).error).toBe('route not found');
  });
});
