/**
 * Express application wiring.
 *
 * `createApp` takes an already-open database so tests can hand it an in-memory
 * one; nothing in this module reads configuration or opens sockets.
 */
import express, {
  type Express,
  type NextFunction,
  type Request,
  type Response,
} from 'express';
import type { DatabaseSync } from 'node:sqlite';
import { TodoRepository } from './todos/repository.js';
import { createTodosRouter } from './todos/routes.js';

/** The JSON body returned for unmatched routes and unhandled errors. */
interface ErrorBody {
  error: string;
}

/** Builds the Express app backed by `db`. */
export function createApp(db: DatabaseSync): Express {
  const app = express();

  app.use(express.json());
  app.use('/todos', createTodosRouter(new TodoRepository(db)));

  app.use((_req: Request, res: Response<ErrorBody>): void => {
    res.status(404).json({ error: 'route not found' });
  });

  app.use(
    (err: unknown, _req: Request, res: Response<ErrorBody>, _next: NextFunction): void => {
      const message = err instanceof Error ? err.message : 'internal server error';
      res.status(500).json({ error: message });
    },
  );

  return app;
}
