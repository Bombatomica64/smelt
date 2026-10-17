/**
 * Minimal `serve()` over `node:http` (staging diagnostic): what
 * `@hono/node-server` does for a buffered request/response, without http2 or
 * streaming. Runs unchanged on Node and through Smelt.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';

/** A fetch-style request handler (`app.fetch`). */
type FetchHandler = (request: Request) => Response | Promise<Response>;

/** Serve options: the handler and the TCP port. */
export interface ServeOptions {
  fetch: FetchHandler;
  port: number;
}

/** Converts one buffered node request into a `Request`, runs `handler`, writes the `Response`. */
async function handle(
  req: IncomingMessage,
  res: ServerResponse,
  body: string,
  handler: FetchHandler,
): Promise<void> {
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (typeof value === 'string') {
      headers.set(name, value);
    }
  }
  const method = req.method ?? 'GET';
  const url = `http://localhost${req.url ?? '/'}`;
  const request =
    method === 'GET' || method === 'HEAD'
      ? new Request(url, { method, headers })
      : new Request(url, { method, headers, body });
  const response = await handler(request);
  const text = await response.text();
  // Only the header this app sets is copied. Iterating a `Headers` value does
  // not lower correctly yet: `headers.forEach` gets Array's `(value, index)`
  // callback signature and `for...of` erases the value into a non-iterable
  // record (blocker-logs/hono-crud-status.md, family M).
  const contentType = response.headers.get('content-type');
  if (contentType !== null) {
    res.setHeader('content-type', contentType);
  }
  res.statusCode = response.status;
  res.end(text);
}

/**
 * Starts an HTTP server on `options.port` that answers with `options.fetch`.
 * Reports readiness with a log line rather than an `onListen` callback: Smelt
 * passes a callback parameter by reference and cannot yet move it into the
 * listen continuation (blocker-logs family N).
 */
export function serve(options: ServeOptions): void {
  const server = createServer((req: IncomingMessage, res: ServerResponse): void => {
    let received = '';
    req.on('data', (chunk: string): void => {
      received += chunk;
    });
    req.on('end', (): void => {
      void handle(req, res, received, options.fetch);
    });
  });
  server.listen(options.port, (): void => {
    console.log(`hono_crud listening on http://localhost:${options.port}`);
  });
}
