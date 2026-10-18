// `serve` from `@hono/node-server`: a fetch handler served over `node:http`.
//
// The adapter converts each request to a fetch `Request` and writes back the
// `Response` the handler answers, directly or through a promise. Smelt models
// the call natively (`HttpCreateFetchServer` + `listen`) rather than lowering
// the adapter's source. Checked here: the method, path, query and body reach
// the handler; status, headers and body come back; a sync and an async answer
// both work; the listening callback runs; `close` lets the program exit.
// It lives outside `examples/` because the handler's answer crosses the
// adapter's erased `unknown` boundary (a `Response` or a promise of one).
import { serve } from "@hono/node-server";

const port = 39417;

const server = serve(
  {
    fetch: async (request: Request): Promise<Response> => {
      // The path and query, read off the serialized URL (`new URL(..)` member
      // reads are a separate, documented gap).
      const target = request.url.slice(request.url.indexOf("/", "http://".length));
      if (target === "/sync") {
        return new Response("sync answer", { status: 202, headers: { "x-kind": "sync" } });
      }
      const body = request.method === "POST" ? await request.text() : "";
      return new Response(`${request.method} ${target} [${body}]`, {
        status: 201,
        headers: { "content-type": "text/plain", "x-kind": "async" },
      });
    },
    port,
  },
  (): void => {
    console.log("listening");
  },
);

const base = `http://127.0.0.1:${port}`;
const posted = await fetch(`${base}/echo?q=1`, { method: "POST", body: "hello body" });
console.log(posted.status, posted.headers.get("x-kind"), posted.headers.get("content-type"));
console.log(await posted.text());

const sync = await fetch(`${base}/sync`);
console.log(sync.status, sync.headers.get("x-kind"));
console.log(await sync.text());

server.close();
console.log("closed");
