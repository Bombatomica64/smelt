// A class instance erased with `as any` and read back as an interface keeps
// every method on its prototype, including ones that take a callback or a
// rest parameter (ts-pattern's `new MatchExpression(..) as any`).
interface Matcher<i> {
  otherwise: (handler: (value: i) => unknown) => unknown;
  when: (predicate: (value: i) => boolean, handler: (value: i) => string) => Matcher<i>;
  with: (...args: unknown[]) => number;
  each: (visit: (value: i) => void) => void;
  run: () => unknown;
}

class MatchExpression {
  value: unknown;
  hits: string[];
  constructor(value: unknown) {
    this.value = value;
    this.hits = [];
  }
  otherwise(handler: (value: unknown) => unknown): unknown {
    return handler(this.value);
  }
  when(predicate: (value: unknown) => boolean, handler: (value: unknown) => string): MatchExpression {
    if (predicate(this.value)) {
      this.hits.push(handler(this.value));
    }
    return this;
  }
  with(...args: unknown[]): number {
    return args.length;
  }
  each(visit: (value: unknown) => void): void {
    visit(this.value);
  }
  run(): unknown {
    return this.hits.join(",");
  }
}

function match<i>(value: i): Matcher<i> {
  return new MatchExpression(value) as any;
}

const m = match(4);
console.log(m.otherwise((v) => v * 10));
console.log(m.with(1, "two", [3]));
console.log(m.with());
m.each((v) => console.log("visit", v));
const chained = m.when((v) => v > 3, (v) => "big " + v).when((v) => v > 9, () => "huge");
console.log(chained.run());
