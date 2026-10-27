// Virtual dispatch on a directly constructed instance: a base method calling
// `this.g()` reaches the most-derived `g` without any conversion.
class Shape {
  describe(): string {
    return `${this.name()} with ${this.sides()} sides`;
  }
  name(): string {
    return "shape";
  }
  sides(): number {
    return 0;
  }
}
class Square extends Shape {
  name(): string {
    return "square";
  }
  sides(): number {
    return 4;
  }
}
class Tile extends Square {}

abstract class Counter {
  items: number[] = [];
  push(value: number): number {
    this.items.push(value);
    return this.accept(value) ? this.items.length : -1;
  }
  accept(value: number): boolean {
    return value > 0;
  }
}
class Plain extends Counter {}

type Parsed<T> = { value: T };
abstract class Parser<T> {
  run(input: string): T {
    const result = this.parse(input);
    return this.validate(result.value) ? result.value : this.fallback();
  }
  validate(value: T): boolean {
    return true;
  }
  abstract parse(input: string): Parsed<T>;
  abstract fallback(): T;
}
class YearParser extends Parser<number> {
  parse(input: string): Parsed<number> {
    return { value: Number(input) };
  }
  validate(value: number): boolean {
    return value > 1900;
  }
  fallback(): number {
    return 1970;
  }
}
class StrictYear extends YearParser {
  validate(value: number): boolean {
    return value > 2000;
  }
}

console.log(new Shape().describe(), "|", new Square().describe(), "|", new Tile().describe());
const plain = new Plain();
console.log(plain.push(3), plain.push(-1), plain.push(5));
console.log(new YearParser().run("2017"), new YearParser().run("12"), new StrictYear().run("1999"));
const parser: Parser<number> = new StrictYear();
console.log(parser.run("2001"), parser.run("1950"));

// A generic record whose callable member returns the record itself.
interface Pair<T> {
  left: T;
  right: T;
  swap(): Pair<T>;
}
function makePair<T>(left: T, right: T): Pair<T> {
  return { left, right, swap() { return makePair(right, left); } };
}
const swapped = makePair(1, 2).swap();
console.log(swapped.left, swapped.right, swapped.swap().left);

// A callback's `join` result types its return.
function apply(fn: (value: string[]) => string): string {
  return fn(["x", "y"]);
}
class Lists {
  items: string[][] = [["a", "b"], ["c"]];
  map(fn: (value: string[]) => string): string[] {
    return this.items.map((v) => fn(v));
  }
}
console.log(apply((v) => v.join("-")), new Lists().map((v) => v.join("+")).join(","));

// `Partial<T>` is its own record, and spreads of records build the struct.
type Options = { origin: string; credentials?: boolean; maxAge?: number };
const defaults: Options = { origin: "*", maxAge: 5 };
function merge(options: Partial<Options>): Options {
  return { ...defaults, ...options };
}
const merged = merge({ credentials: true });
console.log(merged.origin, merged.credentials, merged.maxAge);
const replaced = merge({ origin: "a", maxAge: 9 });
console.log(replaced.origin, replaced.credentials, replaced.maxAge);
const copy: Options = { ...defaults, origin: "b" };
console.log(copy.origin, copy.maxAge, copy.credentials);
