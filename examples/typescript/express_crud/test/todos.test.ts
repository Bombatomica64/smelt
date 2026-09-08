/**
 * End-to-end route tests: a fresh in-memory database per test file, driven
 * through the real Express app with supertest.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import request from 'supertest';
import type { Express } from 'express';
import { createApp } from '../src/app.js';
import { openDatabase } from '../src/db.js';

describe('todos API', () => {
  let app: Express;

  beforeEach((): void => {
    app = createApp(openDatabase(':memory:'));
  });

  it('starts empty', async (): Promise<void> => {
    const response = await request(app).get('/todos');
    expect(response.status).toBe(200);
    expect(response.body).toEqual([]);
  });

  it('creates a todo', async (): Promise<void> => {
    const response = await request(app).post('/todos').send({ title: 'buy milk' });
    expect(response.status).toBe(201);
    expect(response.body.id).toBe(1);
    expect(response.body.title).toBe('buy milk');
    expect(response.body.done).toBe(false);
    expect(typeof response.body.createdAt).toBe('string');
  });

  it('reads a todo back', async (): Promise<void> => {
    const created = await request(app).post('/todos').send({ title: 'walk dog' });
    const response = await request(app).get(`/todos/${created.body.id}`);
    expect(response.status).toBe(200);
    expect(response.body.title).toBe('walk dog');
  });

  it('lists created todos', async (): Promise<void> => {
    await request(app).post('/todos').send({ title: 'first' });
    await request(app).post('/todos').send({ title: 'second', done: true });
    const response = await request(app).get('/todos');
    expect(response.status).toBe(200);
    expect(response.body).toHaveLength(2);
    expect(response.body[1].done).toBe(true);
  });

  it('patches a todo', async (): Promise<void> => {
    const created = await request(app).post('/todos').send({ title: 'draft' });
    const response = await request(app)
      .patch(`/todos/${created.body.id}`)
      .send({ done: true });
    expect(response.status).toBe(200);
    expect(response.body.title).toBe('draft');
    expect(response.body.done).toBe(true);
  });

  it('deletes a todo', async (): Promise<void> => {
    const created = await request(app).post('/todos').send({ title: 'temporary' });
    const deleted = await request(app).delete(`/todos/${created.body.id}`);
    expect(deleted.status).toBe(204);
    const after = await request(app).get(`/todos/${created.body.id}`);
    expect(after.status).toBe(404);
  });

  it('rejects a blank title with 400', async (): Promise<void> => {
    const response = await request(app).post('/todos').send({ title: '   ' });
    expect(response.status).toBe(400);
    expect(response.body.error).toBe('title must be a non-empty string');
  });

  it('returns 404 for a missing todo', async (): Promise<void> => {
    const response = await request(app).get('/todos/999');
    expect(response.status).toBe(404);
    expect(response.body.error).toBe('todo not found');
  });
});
