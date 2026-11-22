// A method declared in an inline type literal is a member of the type, like a
// property: `x.get()` calls the value's own `get`, never a dictionary lookup.

interface Cell {
  v: number;
  get(): number;
}

function makeCell(start: number): Cell {
  let v = start;
  return { v, get(): number { return v * 10; } };
}

// A cast to an inline literal with a method, optional and present.
const held: Cell | undefined = makeCell(3);
const asLiteral = held as { v: number; get(): number } | undefined;
console.log(asLiteral?.v, asLiteral?.get());
const absent = undefined as { v: number; get(): number } | undefined;
console.log(absent?.v, absent?.get());

// A parameter typed by an inline literal whose members share one value type.
function total(counter: { size: number; next(): number }): number {
  return counter.size + counter.next() + counter.next();
}
let n = 0;
console.log(total({ size: 100, next: () => ++n }));

// Methods with parameters and an optional method.
function apply(ops: { add(a: number, b: number): number; label?(): string }): string {
  const sum = ops.add(2, 3);
  return (ops.label ? ops.label() : "none") + "=" + sum;
}
console.log(apply({ add: (a, b) => a + b }));
console.log(apply({ add: (a, b) => a * b, label: () => "mul" }));
