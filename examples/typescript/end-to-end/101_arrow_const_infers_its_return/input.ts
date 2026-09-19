// An arrow constant's return type is INFERRED from its body.
//
// A `const` with no type annotation takes its initializer's type, and an arrow
// with neither a return annotation nor a contextual signature infers its return
// type from what its body evaluates to. Smelt interned the binding's signature
// before lowering the body and left the return type as the erased placeholder,
// so the binding said `(string) => unknown` while the closure it holds is
// `(string) => Wrapper`.
//
// The binding's type is what every CONSUMER reads, which is where the loss
// showed: `promise.then(decorate)` is `Promise<U>` for the callback's own `U`,
// so an erased binding made a continuation that resolves a `Wrapper` claim to
// resolve `unknown` — Hono's `c.html(...)`, whose `.then(res)` continuation
// returns a `Response`, is exactly that shape.

class Wrapper {
  readonly label: string;

  constructor(label: string) {
    this.label = label;
  }
}

async function boxed(text: Promise<string>): Promise<string> {
  // No return annotation and no contextual signature: the return type is the
  // body's.
  const decorate = (value: string) => new Wrapper(`[${value}]`);
  const box = await text.then(decorate);
  return box.label;
}

function direct(value: string): string {
  const twice = (input: string) => new Wrapper(input + input);
  return twice(value).label;
}

console.log(await boxed(Promise.resolve("a")));
console.log(direct("b"));
