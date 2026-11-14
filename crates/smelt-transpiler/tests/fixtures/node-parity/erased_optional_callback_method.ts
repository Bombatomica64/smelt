// A method whose callback parameter is optional (a default, or `?`) is
// callable through an erased instance: an absent argument takes the default,
// a given one is called.

function shout(value: unknown): string {
  return `no match for ${String(value)}`;
}

function refuse(value: unknown): never {
  throw new Error(`refused ${String(value)}`);
}

class Expression {
  private readonly input: number;
  private readonly matched: boolean;

  constructor(input: number, matched: boolean) {
    this.input = input;
    this.matched = matched;
  }

  finish(onMissing: (value: unknown) => string = shout): string {
    if (this.matched) return `matched ${this.input}`;
    return onMissing(this.input);
  }

  strict(onMissing = refuse): number {
    if (this.matched) return this.input;
    return onMissing(this.input);
  }

  maybe(onMissing?: (value: number) => number): number {
    return onMissing ? onMissing(this.input) : -1;
  }
}

function build(input: number, matched: boolean): any {
  return new Expression(input, matched);
}

console.log(build(1, true).finish());
console.log(build(2, false).finish());
console.log(build(3, false).finish((value: unknown) => `custom ${String(value)}`));
console.log(build(6, true).strict());
try {
  build(7, false).strict();
} catch (error) {
  console.log((error as Error).message);
}
console.log(build(4, false).maybe());
console.log(build(5, false).maybe((value: number) => value * 10));
