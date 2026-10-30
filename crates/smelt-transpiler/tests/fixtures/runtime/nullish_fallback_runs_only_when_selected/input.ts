// `a ?? b` evaluates `b` only when `a` is null or undefined (ECMA-262
// CoalesceExpression). A fallback with an observable effect must run once per
// nullish left operand and never for a present one, at an optional type and at
// an erased one. Hono's dispatch ends with `res ?? this.#notFoundHandler(c)`,
// which used to render a not-found response on every request.
let calls = 0;

function fallback(label: string): string {
  calls += 1;
  return `fallback:${label}`;
}

function pick(value: string | undefined, label: string): string {
  return value ?? fallback(label);
}

function pickUnknown(value: unknown): unknown {
  return value ?? fallback('erased');
}

console.log(pick('present', 'a'), calls);
console.log(pick(undefined, 'b'), calls);
console.log(pickUnknown(0), calls);
console.log(pickUnknown(null), calls);
console.log(pickUnknown(JSON.parse('"text"')), calls);
console.log(pickUnknown(undefined), calls);

class Counter {
  count = 0;

  next(): number {
    this.count += 1;
    return this.count;
  }
}

const counter = new Counter();
const values: (number | undefined)[] = [1, undefined];
const total = (values[0] ?? counter.next()) + (values[1] ?? counter.next());
console.log(total, counter.count);
