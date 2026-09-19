// Three rules about an OPTIONAL value of a modeled class, which together
// decided whether a `Headers` stayed a `Headers` or became a `string`.
//
// 1. The VALUE of `x ??= v` is never nullish. Either the store happened and the
//    value is `v`, or it did not and `x` was already non-nullish — so the
//    expression's type is `NonNullable<typeof x> | typeof v`, which is what
//    TypeScript gives it. `||=` and `&&=` keep the target's type, because their
//    value CAN be the original falsy one.
//
// 2. A conditional whose arms are `T` and `T | undefined` is `T | undefined`.
//    That is just TypeScript's `typeof a | typeof b`, and it is a closer answer
//    than any widening.
//
// Without 1 and 2, `cond ? res.headers : (prepared ??= new Headers())` — a
// `Headers` joined with a `Headers | undefined` — fell through to a
// string-compatibility test that accepts any class name and unified to
// `string`, so a `Headers` value was assigned into a `String` local.
//
// 3. A modeled method call on an OPTIONAL receiver is a call on the inner
//    value: `tsc` only accepts `maybe.set(k, v)` where it has already narrowed
//    `maybe`, so the optional surface is one Smelt's own flow typing did not
//    drop. Asserting presence is what the modeled property READS already do.
//
// Every line is diffed against Node.

class Bag {
  private prepared: Headers | undefined;
  private current: Headers | undefined;

  // Rule 1 + rule 2: the ternary's arms are `Headers` and `Headers`, because
  // the `??=` value is non-nullish.
  write(name: string, value: string): void {
    const headers = this.current ? this.current : (this.prepared ??= new Headers());
    headers.set(name, value);
  }

  // Rule 2 on its own: `Headers` joined with `Headers | undefined`.
  peek(name: string): string {
    const headers = this.current ? this.current : this.prepared;
    return headers ? (headers.get(name) ?? "absent") : "none";
  }

  // Rule 3: the method call happens on a receiver Smelt still types optional.
  appendTo(name: string, value: string): void {
    if (this.prepared) {
      this.prepared.append(name, value);
    }
  }

  read(name: string): string {
    return this.prepared?.get(name) ?? "absent";
  }

  adopt(headers: Headers): void {
    this.current = headers;
  }
}

const bag = new Bag();
console.log(bag.peek("x"));
bag.write("x", "1");
console.log(bag.peek("x"), bag.read("x"));
bag.appendTo("x", "2");
console.log(bag.read("x"));

// The same `??=` value read directly, so its non-nullish type is visible
// outside a ternary.
let maybe: Headers | undefined;
const ensured = (maybe ??= new Headers());
ensured.set("y", "3");
console.log(ensured.get("y") ?? "absent", maybe.get("y") ?? "absent");

// A second `??=` does NOT replace a value that is already there.
const again = (maybe ??= new Headers());
console.log(again.get("y") ?? "absent");

// `||=` keeps the target's own type, because its value can be the original.
let text: string = "";
const filled = (text ||= "fallback");
console.log(filled, text);

// A `Headers` that came from a response, so the ternary's other arm is a real
// value rather than a fresh list.
const response = new Response("body", { headers: { "x-from": "response" } });
bag.adopt(response.headers);
bag.write("x-added", "4");
console.log(bag.peek("x-from"), bag.peek("x-added"));
