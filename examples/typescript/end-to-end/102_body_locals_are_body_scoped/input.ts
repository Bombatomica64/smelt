// A binding declared inside a function body is not in scope outside it —
// round 32, Agent G item 2a.
//
// Closure lowering restored only the names it bound itself: the parameters and
// the captures. Every `const` the body's own statements declared stayed bound
// in the ENCLOSING scope after the closure was finished, so a later SIBLING
// closure saw it. That is already wrong as scoping, and it corrupts the second
// body, because a `LocalId` is an index into the body that owns it: the stale
// binding named a slot of the NEW body, and the reuse check could only verify
// that the index exists.
//
// Measured on the shape below (radash's `curry.test.ts`, two `test(() => ..)`
// closures in one `describe`), the second closure lowered to
//
//   %0 user repeat: fn(Float) -> String        <- also holds `make`
//   %1 user twice:  fn(Float) -> Float         <- also holds `addFive`
//
// so `make(2)` was emitted as a call of `repeat`, and four source bindings
// shared two slots. The first closure, lowered when nothing was bound yet, was
// correct — which is why each of the two ran fine on its own and only the pair
// failed.
//
// The two bodies below are deliberately near-identical: same binding names,
// same order, same types for the shared prefix, and one extra binding in the
// second. Every line is diffed against Node.

const run = (label: string, body: () => string): string => `${label}=${body()}`;

console.log(
  run("first", () => {
    const make = (y: number) => (x: number) => x + y;
    const addFive = make(5);
    const twice = (n: number) => n * 2;
    return String(twice(addFive(make(1)(0))));
  }),
);

console.log(
  run("second", () => {
    const make = (y: number) => (x: number) => x + y;
    const addFive = make(5);
    const twice = (n: number) => n * 2;
    const repeat = (n: number) => "x".repeat(n);
    return repeat(twice(addFive(make(2)(0))));
  }),
);

// A third body whose bindings shadow NOTHING, to show the restore is a restore
// and not a purge: `run` itself is still callable after two closures have been
// lowered, and its own parameter names are untouched.
console.log(run("third", () => "plain"));
