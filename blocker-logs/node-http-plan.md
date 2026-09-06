# `node:http` on hyper 1 — the decisions, before the code

Not started. This is the design note the next round should start from, written
after `node:events` landed because the two are coupled in a way the original
item description did not anticipate. `node:http` is still `Declared` in
`host_modules.rs` (`HTTP_REASON`), so every use reports an honest blocker today.

## Why this did not land alongside EventEmitter

The item as written is "node:http on hyper 1 with the current-thread
`LocalSet`/`spawn_local` runtime". Working through what the echo fixture
actually needs turns that into four separate pieces, three of which are each
about the size of the whole `EventEmitter` item:

1. three new modeled classes through the nine-site recipe (`Server`,
   `IncomingMessage`, `ServerResponse`);
2. `IncomingMessage` and `ServerResponse` are **EventEmitters** in Node, so the
   emitter dispatch has to accept them as receivers — see below;
3. a new backend dependency (hyper 1 + hyper-util + http-body-util) and the
   `#[tokio::main]` flavor change, which touches every async program, not just
   the ones that serve;
4. the fixture and its runtime tier, which compile hyper inside a generated
   crate.

Landing 1 and 3 without 2 would flip `node:http` to `Modeled` while a plain
`req.on('data', ..)` still failed — a registry that claims a surface it does not
serve, which is the exact false green this tier exists to remove. So it is one
commit or none.

## The decisions already made

**Runtime flavor: `#[tokio::main(flavor = "current_thread")]` plus a
`LocalSet`, with `spawn_local` for each connection.** Not a workaround. Every
value Smelt generates is `Rc`-based, so a request handler's captured state is
not `Send` and cannot cross a work-stealing runtime's threads. A single-threaded
event loop is also what the source language actually has: a JS port that
silently gained parallel handler execution would be a different program. The
current emitter writes a bare `#[tokio::main]` in
`emitter/core.rs` (~line 160); the flavor belongs there, applied to every async
`main`, not only to serving ones — two runtime shapes for the same language is
the kind of special case that gets refused.

**`req.headers` is a plain object, not a `Headers`.** Node's `IncomingMessage`
exposes a lowercased string map, and `Headers` is the fetch type. Modeling it as
`Headers` would hand callers `get`/`has` methods the source does not have.

**`server.listen(port, cb)` answers the server**, so `createServer(..).listen(..)`
chains, and `listen(0)` must expose the bound port through `server.address()` —
the fixture round-trips on port 0, so `address()` is not optional extra surface,
it is what makes the test addressable.

## The coupling to EventEmitter

`IncomingMessage` extends `EventEmitter`, and a real echo handler is written
through it:

```ts
createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => { body += chunk; });
  req.on('end', () => { res.writeHead(200, {}); res.end(body); });
});
```

`SmeltEventEmitter` is already the right shape for this — an insertion-ordered
listener list behind an `Rc<RefCell<..>>` with snapshot `emit` semantics — so the
work is not a second implementation but making the emitter operations dispatch
on an `IncomingMessage` receiver, and having the hyper glue drive `data`/`end`
through the same `emit` path a source `emit` uses. Doing it any other way (a
private listener list on `IncomingMessage`) would mean two emitters with two
sets of ordering bugs.

That is the piece to design first, because it decides whether the modeled
classes carry an emitter by composition (a `SmeltEventEmitter` field, with the
`on`/`once`/`off` dispatch delegating to it) or whether the frontend grows a
notion of "extends a modeled class". Composition looks right and is smaller, but
it needs the receiver test in `dispatch_event_emitter_method` to widen from "is
the emitter class" to "has an emitter", which is a real design change rather
than a line.
