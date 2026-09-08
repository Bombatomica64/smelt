// An ambient init dictionary is a real struct, not an erased record.
//
// `ResponseInit` and `RequestInit` are lib.d.ts DICTIONARIES: bags of optional
// keys with no methods and no identity. Modeling them as opaque classes made a
// value of one arrive as an erased record, so every key was read back through a
// checked cast — and the cast tried to recover a `Headers` from a plain record
// literal and answered an EMPTY header list. A silent wrong value, and, where
// the parameter was optional, a `SmeltUnknown` that the `Headers` conversion
// refused outright.
//
// They are declared as interfaces with the spec's optional typed keys instead,
// so a parameter typed by one is a generated struct: `status: Option<f64>`,
// `statusText: Option<String>`, `headers: Option<HeadersInit>`. An object
// literal at the call site converts into it through the ordinary
// record-to-struct adapter, and the fields read with their own types.
//
// `headers` keeps the UNION WHATWG gives it (`Headers`, a pair list, or a
// record) rather than being narrowed to the `Headers` arm alone, which is what
// lets all three spellings reach the constructor's per-arm conversion.
//
// A source declaration of the same name still wins: a program with its own
// `interface ResponseInit` keeps it, and `globalThis.ResponseInit` is how such
// a program asks for the ambient one — which is exactly why the forwarding
// helper below spells it that way.

// The shape Hono forwards through: an OPTIONAL ambient init parameter passed
// straight to the constructor.
// (The body is spelled `string | undefined` rather than `BodyInit` because
// `BodyInit`'s other arms are host classes Smelt does not model, so the union
// erases — a boundary of its own, covered by an emission test rather than here,
// since this corpus holds zero avoidable erasure.)
const createResponseInstance = (
  body?: string,
  init?: globalThis.ResponseInit,
): Response => new Response(body, init);

// The init is bound to an annotated const rather than written inline at this
// call: an object literal passed straight to a CONST-BOUND ARROW is not reached
// by the parameter's type hint (it is built as a record and converted), which
// is a hint-propagation gap of its own, unrelated to the init modeling and
// recorded in `blocker-logs/standards-ambient-init-dictionaries.md`. Every
// other call below writes its literal inline, where the hint does arrive.
const createdInit: globalThis.ResponseInit = {
  status: 201,
  statusText: "Created",
  headers: { "x-a": "1" },
};
const withInit = createResponseInstance("hi", createdInit);
console.log(withInit.status);
console.log(withInit.statusText);
console.log(withInit.headers.get("x-a") ?? "none");

// No init at all: the constructor's own defaults, and an absent body is the
// empty body rather than a blocker.
const bare = createResponseInstance();
console.log(bare.status);
console.log(bare.headers.get("x-a") ?? "none");

// The same init type, with the `headers` key spelled as each of its arms.
function statusOf(init: globalThis.ResponseInit): string {
  const response = new Response("body", init);
  return `${response.status} ${response.headers.get("content-type") ?? "none"}`;
}

console.log(statusOf({ status: 202, headers: { "content-type": "text/plain" } }));
const pairs: [string, string][] = [["content-type", "text/html"]];
console.log(statusOf({ status: 203, headers: pairs }));
console.log(statusOf({ status: 206, headers: new Headers([["content-type", "application/json"]]) }));

// A `RequestInit` reads its own keys the same way.
const request = new Request("http://example.test/x", {
  method: "POST",
  headers: { "x-b": "2" },
  body: "payload",
});
console.log(request.method);
console.log(request.headers.get("x-b") ?? "none");
