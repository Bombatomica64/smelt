// A `node:http` echo server, served and called by the same program.
//
// Everything the module models is exercised in the order a real handler uses
// it: the request's method and url; the body read through the `EventEmitter`
// inheritance (`req.on('data')` / `req.on('end')`); the status line; a header
// set and read back; and `listen(0)` / `address()` / `close()`.
//
// Port 0 is what makes this addressable without picking a port that might
// already be taken: the OS chooses, `address()` reports it, and the URL is
// built from that. Nothing prints the port, so the output is identical on every
// run.
//
// It is one program rather than a server and a client because that is the
// stronger claim: the handler, the accept loop and the `fetch` that calls them
// all share one current-thread runtime, so a `fetch` that blocked the loop —
// or a server task that the exit drain dropped — would deadlock instead of
// printing.
import { createServer } from "node:http";

const server = createServer((req, res) => {
  let received = "";
  req.on("data", (chunk) => {
    received += chunk;
  });
  req.on("end", () => {
    // Two ways to set the same status line, so both are covered: the POST
    // takes the field-by-field path and the GET takes `writeHead`, which
    // MERGES its object over whatever `setHeader` already put there.
    if (req.method === "POST") {
      res.statusCode = 201;
      res.setHeader("content-type", "application/json");
    } else {
      res.setHeader("x-set-first", "kept");
      res.writeHead(200, { "content-type": "application/json" });
    }
    // Every field is a string on purpose. A record whose values have different
    // types has no single value type to give it, so it lowers to an erased map
    // — avoidable erasure that would be about record lowering rather than
    // about `node:http`, in the one corpus whose invariant is that there is
    // none. `getHeader` answers `string | null`, so the absent case is spelled
    // here rather than left to `JSON.stringify` (which drops an undefined
    // value entirely, and would make the two responses differ in shape as well
    // as in content).
    res.end(
      JSON.stringify({
        method: req.method,
        url: req.url,
        body: received,
        status: `${res.statusCode}`,
        sentType: res.getHeader("content-type") ?? "",
        kept: res.getHeader("x-set-first") ?? "",
      }),
    );
  });
});

server.listen(0, () => {
  console.log("listening");
});

// `address()` answers the bound port or null, so it is narrowed before use —
// the same shape as Node, where `server.address()` is `AddressInfo | null`.
const port = server.address() ?? 0;
const base = `http://127.0.0.1:${port}`;

const posted = await fetch(`${base}/echo?q=1`, {
  method: "POST",
  body: "hello body",
});
console.log(posted.status);
console.log(await posted.text());

// A second exchange proves the accept loop keeps serving after the first
// connection closes, and that a GET arrives with no `data` event at all.
const plain = await fetch(`${base}/again`);
console.log(plain.status);
console.log(await plain.text());

// After `close` the server is no longer bound, so it reports no address and
// the program is free to exit — nothing keeps the loop alive any more.
server.close();
console.log(server.address() ?? -1);
