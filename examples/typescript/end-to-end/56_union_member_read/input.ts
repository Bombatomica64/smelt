// A property read whose receiver is a union reads the ARM, not a runtime
// property bag.
//
// A union lowers to a generated Rust enum, so at `u.f` the arm is a runtime
// question and each arm's read is a static one. Both used to be thrown away:
// the receiver was erased to a tagged value and the property looked up by
// name, which handed every consumer downstream a `SmeltUnknown`. Now the
// generated code matches the enum, projects the arm, and reads the member the
// way that arm's own type reads it — a struct field on an interface arm, the
// runtime accessor on a `Response` arm.
//
// The `typeof` guard is part of the same story: `typeof x === 'object'` used
// to narrow to the erased boundary, discarding the union. It now keeps the
// object-kinded arms, which is what the guard actually proves.

interface InitLike {
  headers?: Headers;
  status?: number;
}

// Two object arms, one of which is a host class whose `.headers` is an
// accessor rather than a field.
function headerOf(arg: InitLike | Response, name: string): string {
  const headers = arg.headers;
  if (headers === undefined) {
    return "none";
  }
  return headers.get(name) ?? "none";
}

const init: InitLike = { headers: new Headers([["x-a", "from-init"]]) };
console.log(headerOf(init, "x-a"));
console.log(headerOf(new Response("body", { headers: { "x-a": "from-response" } }), "x-a"));
console.log(headerOf({ status: 204 }, "x-a"));

// A union that still holds a NUMBER arm at the guard. `typeof arg === 'object'`
// now keeps only the object-kinded arms, so the read inside the guard is a
// two-arm dispatch rather than a property lookup on an erased value; the
// number arm reaches the read only in sources that narrow it away some other
// way, where it contributes `undefined` — what `(204).headers` answers in
// JavaScript.
type StatusOrInit = number | InitLike | Response;

function statusHeaderOf(arg: StatusOrInit): string {
  if (typeof arg === "object") {
    const headers = arg.headers;
    if (headers !== undefined) {
      return headers.get("x-b") ?? "none";
    }
  }
  return "not-an-object";
}

console.log(statusHeaderOf({ headers: new Headers([["x-b", "init"]]) }));
console.log(statusHeaderOf(new Response("body", { headers: { "x-b": "response" } })));
console.log(statusHeaderOf({ status: 201 }));
console.log(statusHeaderOf(204));

// The same member name, two different reads: `status` is a declared optional
// field on the interface arm and a runtime accessor on the `Response` arm
// (whose default is 200). Picking the node per arm is the reason this
// desugaring lives in the frontend — an emitter looking at a field place could
// only ever render one of the two.
function statusOf(arg: StatusOrInit): number {
  if (typeof arg === "object") {
    return arg.status ?? -1;
  }
  return arg;
}

console.log(statusOf({ status: 201 }));
console.log(statusOf(new Response("body")));
console.log(statusOf(418));
