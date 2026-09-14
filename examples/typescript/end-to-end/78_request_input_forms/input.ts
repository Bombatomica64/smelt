// `new Request(input, init)` where `input` is a `Request` or a `URL`.
//
// The spec's `RequestInfo` is `Request | string`. A REQUEST input is the "copy
// the source" form: the new request starts from the source's url, method,
// headers and body, and each init key that is PRESENT overrides its slot. So
// the input decides the defaults rather than needing a second constructor — the
// same `from_parts` builds both spellings, and the init reader is unchanged.
//
// The one place this is more than plumbing is the BODY. The spec extracts the
// source's body: the copy reads the same payload, and the source is disturbed
// at once. Measured against Node 22, `new Request(source)` leaves
// `source.bodyUsed` TRUE and the copy's false before either is read — so the
// two share the payload but NOT the used flag. Sharing the flag would leave the
// source readable until the copy was consumed; copying the payload would let
// both be read. Neither is what the spec says.
//
// A `URL` input contributes its serialization, which it already did: a
// `new URL(x)` lowers to its `href`, so such an input arrives as a string and
// needs nothing of its own. (URL member READS are a separate gap — see the
// last line — and are not what an input position asks for.)
//
// Every line below is diffed against Node.

async function run(): Promise<void> {
  const source = new Request("https://a.test/one", {
    method: "POST",
    headers: { "x-tag": "t" },
    body: "payload",
  });

  // Every slot comes from the input when the init does not say otherwise.
  const copied = new Request(source);
  console.log(copied.url, copied.method, copied.headers.get("x-tag"));

  // The source is disturbed by the construction; the copy is not, and reading
  // it reads the source's own bytes.
  console.log(source.bodyUsed, copied.bodyUsed);
  console.log(await copied.text());
  console.log(source.bodyUsed, copied.bodyUsed);

  // A present init key overrides its slot; the rest still come from the input.
  const overridden = new Request(
    new Request("https://a.test/two", { method: "PUT", headers: { "x-tag": "u" } }),
    { method: "DELETE" }
  );
  console.log(overridden.url, overridden.method, overridden.headers.get("x-tag"));

  // The init may itself be a `Request`, which was already modeled; both
  // positions being one is the shape Hono's `app.request` uses.
  const template = new Request("https://a.test/three", { method: "PATCH", headers: { a: "1" } });
  const both = new Request(template, template);
  console.log(both.url, both.method, both.headers.get("a"));

  // A body-less source copies as a body-less request.
  const bare = new Request("https://a.test/four", { method: "HEAD" });
  const bareCopy = new Request(bare);
  console.log(bareCopy.method, bareCopy.bodyUsed, bare.bodyUsed);
  console.log(bareCopy.body === null);

  // An init body wins over the source's, and then the source is still
  // disturbed by having been read from.
  const replaced = new Request(
    new Request("https://a.test/five", { method: "POST", body: "from source" }),
    { body: "from init" }
  );
  console.log(replaced.url, await replaced.text());

  // A `URL` input contributes its serialization.
  const fromUrl = new Request(new URL("https://a.test/six?q=1"), { method: "PUT" });
  console.log(fromUrl.url, fromUrl.method);

  // An ERASED input — a `RequestInfo` parameter whose arm is a run-time fact,
  // which is Hono's `app.request(input: string | Request | URL, ...)` — is
  // erased by construction, so it lives in the runtime tier
  // (`crates/smelt-codegen-rust/tests/request_runtime.rs`,
  // `an_erased_request_info_input_takes_every_arm`) rather than here: the
  // examples corpus holds a hard `avoidable == 0` invariant, and the precedent
  // for splitting a fixture that way is `74_typed_array_views`. What the tier
  // pins is the same claim this fixture makes for the concrete spellings — the
  // choice is made once on the tag, a request record recovering through the
  // class's own boundary adapter and anything else stringifying, which is what
  // JavaScript does for a non-`Request` input.
}

run();
