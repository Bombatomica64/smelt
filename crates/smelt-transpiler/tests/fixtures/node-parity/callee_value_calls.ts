// A callee that is a computed value (a conditional, comma or logical
// expression) is called with `this` undefined, as JavaScript calls any value
// that is not a reference; a parenthesized member keeps its receiver.

function attempt(label: string, run: () => unknown): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, (error as Error).name, (error as Error).message);
  }
}

const a: any = [1, 2];
const b: any = [3];
const flag: boolean = Date.now() > 0;
const none: any = undefined;
const inc = (x: number): number => x + 1;
const dbl = (x: number): number => x * 2;
let steps = 0;

// Member values chosen by an expression lose their receiver.
attempt("conditional members", () => (flag ? a.map : b.map)((x: number) => x));
attempt("comma member", () => (0, a.map)((x: number) => x));
attempt("logical member", () => (none || a.map)((x: number) => x));
attempt("nullish member", () => (none ?? a.map)((x: number) => x));

// A parenthesized member is still a reference.
attempt("parenthesized member", () => (a.map)((x: number) => x * 5));

// Typed functions chosen by an expression, and the comma's effects.
attempt("conditional functions", () => (flag ? inc : dbl)(10));
attempt("comma function", () => [(steps++, dbl)(4), steps]);
attempt("comma value", () => (steps++, steps++, steps));

// An explicit receiver still works, and the plain call's `undefined` holds
// inside a method call too.
attempt("conditional call method", () => (flag ? a.map : b.map).call(b, (x: number) => x - 1));
const holder: any = {
  run(this: any) {
    return (0, a.map)((x: number) => x);
  },
};
attempt("comma inside method", () => holder.run());
