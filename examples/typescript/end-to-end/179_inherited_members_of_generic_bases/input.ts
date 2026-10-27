// Methods inherited from a generic base are copied into each subclass, where
// the base's type parameters stand for the subclass's arguments.
type Box1<T> = { value: T };

class Base<T> {
  items: T[] = [];

  push(value: T): number {
    this.items.push(value);
    const wrapped: Box1<T> = { value };
    return this.check(wrapped) ? this.items.length : -1;
  }

  // `undefined` compared against a `T` stays a literal once `T` is concrete.
  check(box: Box1<T>): boolean {
    return box.value !== undefined;
  }

  // Named like `Array.prototype.map`: a subclass still calls this method.
  map(fn: (value: T) => string): string[] {
    return this.items.map((v) => fn(v));
  }
}

// Both classes name their parameter `T`; `Base`'s `T` is `Middle`'s `T[]`.
class Middle<T> extends Base<T[]> {
  total(): number {
    return this.items.length;
  }
}

class Leaf extends Middle<string> {}

class Nums extends Base<number> {}

type ParseResult<T> = { value: T };

abstract class Parser<T> {
  run(input: string): T {
    const result = this.parse(input);
    return result.value;
  }

  abstract parse(input: string): ParseResult<T>;
}

class YearParser extends Parser<number> {
  parse(input: string): ParseResult<number> {
    return { value: Number(input) + 1 };
  }
}

const leaf = new Leaf();
console.log(leaf.push(["a", "b"]), leaf.push(["c"]), leaf.total());
const joined: string[] = leaf.map((v) => `${v.length}:${v[0]}`);
console.log(joined.join(","));

const nums = new Nums();
nums.push(3);
nums.push(4);
console.log(nums.map((n) => `${n * 2}`).join(" "));

const parser: Parser<number> = new YearParser();
console.log(parser.run("2016"));
