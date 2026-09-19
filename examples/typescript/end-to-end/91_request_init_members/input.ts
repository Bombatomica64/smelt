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
//    source's referrer. An EMPTY init (`new Request(src, {})`) is still empty,
//    so it copies. Node 22 agrees on all three, and every line below is diffed
//    against it.
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

// A `Request` input with NO init copies the whole group, referrer included.
console.log(describe(new Request(full)));

// An EMPTY init is still an empty init: it copies too.
console.log(describe(new Request(full, {})));

// A NON-EMPTY init copies the rest and resets the referrer pair.
console.log(describe(new Request(full, { method: "POST" })));

// ... unless the init supplies them itself.
console.log(describe(new Request(full, { referrer: "https://d.test/here" })));

// `clone()` carries the stored group.
console.log(describe(full.clone()));

// The defaults are not disturbed by an init that names other keys.
console.log(describe(new Request("https://a.test/three", { method: "PUT" })));
