// `(typeof obj)[keyof typeof obj]`: the const-object-as-closed-value-set idiom.
//
// A type query asks for the type of a VALUE binding, so the answer is the type
// the frontend already resolved that binding to. While the query answered
// `Unknown`, an alias written this way erased — and because an erased member
// makes a WHOLE union non-concrete (`union_member_is_concrete`), one such alias
// inside a union erased the union entirely. That is how Hono's five-arm
// `ResponseHeadersInit` reached `new Headers(init)` as `SmeltUnknown`: one arm
// was `Record<'Content-Type', BaseMime>`, and `BaseMime` is exactly this
// spelling. See `blocker-logs/standards-generic-arm-and-typeof-indexed-alias.md`.
//
// The alias is declared BEFORE the const it queries, as Hono's is: module
// bindings are collected in a prepass, so the annotation sees the binding
// regardless of source order.
type Mime = (typeof mimes)[keyof typeof mimes];

const mimes = {
  json: "application/json",
  text: "text/plain",
} as const;

type HeadersInitLike =
  | [string, string][]
  | Record<"Content-Type", Mime>
  | Record<string, string>
  | Headers;

// Every arm here is concrete, so the read dispatches on the arm rather than
// erasing: that is the whole point of resolving the alias.
function headerOf(init: HeadersInitLike, name: string): string {
  const copied = new Headers(init);
  return copied.get(name) ?? "none";
}

const declared: Mime = mimes.json;
console.log(headerOf({ "Content-Type": declared }, "content-type"));
console.log(headerOf([["x-kind", "pairs"]], "x-kind"));
console.log(headerOf({ "x-kind": "record" }, "x-kind"));
console.log(headerOf(new Headers([["x-kind", "headers"]]), "x-kind"));
console.log(headerOf({ "Content-Type": mimes.text }, "x-missing"));
