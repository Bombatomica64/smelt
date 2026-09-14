// A GENERIC init interface inside a union: the arm's own type argument decides
// the field types that the per-arm read and the arm's storage struct must agree
// on.
//
// `InitLike<T extends StatusCode>` is instantiated at its default argument
// here, so the generated arm payload is `InitLike<f64>` and `status?: T` is a
// number. The record round-trip that rebuilds the arm from an erased object
// substitutes the same argument into the same field, which needs the
// substituted type to exist in the MIR type table — see the MIR pass
// `lower::passes::generic_records`. Before that pass the two renderings
// disagreed (`Option<f64>` against `Option<SmeltUnknown>`) and the generated
// crate did not compile.
//
// `statusOf` reads the generic field itself: `arg.status` off the instantiated
// arm is an `Option<f64>`, so the read must report the substituted field type
// (`emitter::types::place_ty`) rather than the declaration-time `T`, which
// erased and made the caller coerce a `SmeltUnknown` into an `f64`.
type StatusCode = 200 | 201 | 404;
type HeadersInitLike = [string, string][] | Record<string, string> | Headers;

interface InitLike<T extends StatusCode = StatusCode> {
  headers?: HeadersInitLike;
  status?: T;
}

type StatusOrInit<T extends StatusCode = StatusCode> = InitLike<T> | Response;

function headerOf(arg: StatusCode | StatusOrInit, name: string): string {
  if (typeof arg === "object" && arg.headers) {
    const copied = new Headers(arg.headers);
    return copied.get(name) ?? "none";
  }
  return "none";
}

function statusOf(arg: StatusCode | StatusOrInit): number {
  if (typeof arg === "object" && !(arg instanceof Response) && arg.status) {
    return arg.status;
  }
  return 0;
}

const recordInit: InitLike = { headers: { "x-kind": "record" }, status: 201 };
console.log(headerOf(recordInit, "x-kind"));
console.log(statusOf(recordInit));
console.log(headerOf(new Response("body", { headers: { "x-kind": "response" } }), "x-kind"));
console.log(headerOf(404, "x-kind"));
