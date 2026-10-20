// The stored `RequestInit` members: `cache`, `credentials`, `integrity`,
// `keepalive`, `mode`, `redirect`, `referrer` and `referrerPolicy`.
//
// These eight differ from `method`/`headers`/`body` in that nothing reads them
// to build the request's transport identity: each is a scalar the constructor
// stores and a read-only getter answers unchanged. So they are one group in the
// generated Rust — `SmeltRequestInit` — with the spec's defaults, an override
// per init key, and a copy on `clone()`.
//
// Two rules are worth the fixture rather than the code comment:
//
//  * a `Request` INPUT copies the whole group, which is the spec's "copy the
//    source" form;
//  * but a NON-EMPTY init puts `referrer` and `referrerPolicy` back to their
//    defaults, because a request being re-initialized must not inherit the
//    source's referrer.
//
// VERSION DEPENDENCE. Which inits count as empty differs between Node
// releases. By the WHATWG text, and in Node 22 (undici 6), an absent or empty
// init (`new Request(src)`, `new Request(src, {})`) is empty, so it copies the
// referrer pair. Node 24 (undici 7) converts every init through a dictionary
// that defaults `priority` to 'auto', so the converted init is NEVER empty and
// those two lines reset the pair to `about:client|` as well. `expected.stdout`
// is Node 24's output, the version CI pins for the oracle check that diffs
// every line below against `node`.
//
// A spelled `referrer` is stored as its URL SERIALIZATION, the same rule the
// request URL itself goes through: `"https://c.test"` reads back as
// `"https://c.test/"`.

function describe(request: Request): string {
  return [
    request.cache,
    request.credentials,
    request.integrity,
    `${request.keepalive}`,
    request.mode,
    request.redirect,
    request.referrer,
    request.referrerPolicy,
  ].join("|");
}

// The spec defaults for a request built from a URL string.
const plain = new Request("https://a.test/one");
console.log(describe(plain));

// Every member set explicitly.
const full = new Request("https://a.test/two", {
  cache: "no-store",
  credentials: "include",
  integrity: "sha256-abc",
  keepalive: true,
  mode: "same-origin",
  redirect: "manual",
  referrer: "https://c.test",
  referrerPolicy: "no-referrer",
});
console.log(describe(full));

// Read one member at a time, so the getters are exercised outside the join.
console.log(full.cache, full.credentials, full.integrity);
console.log(full.keepalive, full.mode, full.redirect);
console.log(full.referrer, full.referrerPolicy);

// A `Request` input with NO init copies the whole group; Node 24 resets the
// referrer pair (see the version note above).
console.log(describe(new Request(full)));

// An EMPTY init: the same as no init (the pair resets under Node 24).
console.log(describe(new Request(full, {})));

// A NON-EMPTY init copies the rest and resets the referrer pair.
console.log(describe(new Request(full, { method: "POST" })));

// ... unless the init supplies them itself.
console.log(describe(new Request(full, { referrer: "https://d.test/here" })));

// `clone()` carries the stored group.
console.log(describe(full.clone()));

// The defaults are not disturbed by an init that names other keys.
console.log(describe(new Request("https://a.test/three", { method: "PUT" })));
