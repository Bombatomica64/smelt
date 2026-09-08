// `new Headers(init)` where `init`'s ARM is decided at runtime.
//
// `HeadersInit` is a union in WHATWG's own IDL — `Headers`, a
// `[name, value]` pair list, or a `Record<string, string>` — so source that
// keeps the union rather than committing to one spelling (Hono's
// `ResponseHeadersInit`, or any `headers?:` init key) hands the constructor a
// value whose arm only the runtime knows, while every arm's conversion is
// statically known. A generated union is a tagged enum, so the arm IS
// recoverable: the emitter matches it and runs that arm's own conversion. The
// alternative — erasing the union to a tagged runtime value and re-inspecting
// it — would throw away the static type to re-derive what the enum already
// says.
//
// The absent arm is modeled too: `new Headers(undefined)` is the empty header
// list, the same answer the no-argument spelling gives, so an `Optional` init
// needs no blocker.

type HeaderPairs = [string, string][];
type HeadersInitUnion = HeaderPairs | Record<string, string> | Headers;

// One conversion per arm, chosen by the tag.
function contentTypeOf(init: HeadersInitUnion): string {
  return new Headers(init).get("content-type") ?? "none";
}

// The pair-list arm's own spelling is a TUPLE list, so the argument carries the
// annotation: a bare `[["a", "b"]]` literal is `string[][]` to TypeScript, and
// injecting THAT into the tuple arm is a separate coercion that still erases —
// recorded in `blocker-logs/standards-hono-headers-init-resolved.md`.
const htmlPairs: HeaderPairs = [["content-type", "text/html"]];

console.log(contentTypeOf({ "Content-Type": "text/plain" }));
console.log(contentTypeOf(htmlPairs));
console.log(contentTypeOf(new Headers([["Content-Type", "application/json"]])));

// The same union, reached through an optional init key. `headers` is absent in
// the second call, so the constructor answers an empty header list.
interface InitLike {
  headers?: HeadersInitUnion;
}

function firstHeader(init: InitLike): string {
  const copied = new Headers(init.headers);
  return copied.get("a") ?? "none";
}

console.log(firstHeader({ headers: { a: "1", b: "2" } }));
console.log(firstHeader({}));

// A copy is a distinct object: writing through the copy must not touch the
// source, which is what the spec's fill algorithm over the source's iteration
// order gives.
const source = new Headers([["x-shared", "original"]]);
const copy = new Headers(source);
copy.set("x-shared", "changed");
console.log(source.get("x-shared"));
console.log(copy.get("x-shared"));

// A pair list keeps insertion order per name: two values under one name join
// with ", " on read, exactly as `append` does.
const twoValues: HeaderPairs = [
  ["content-type", "text/plain"],
  ["content-type", "text/css"],
];
console.log(contentTypeOf(twoValues));
