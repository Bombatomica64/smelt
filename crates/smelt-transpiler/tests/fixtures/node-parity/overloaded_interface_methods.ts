// A method an interface declares with several call signatures is one
// function that accepts every overload's arguments (ts-pattern's `Match.with`).
type Matcher<T> = {
  with(pattern: T, handler: (value: T) => string): Matcher<T>;
  with(first: T, second: T, handler: (value: T) => string): Matcher<T>;
  with(pattern: T, guard: (value: T) => boolean, handler: (value: T) => string): Matcher<T>;
  run(): string;
};

class Chain<T> {
  private result: string | undefined;
  private readonly input: T;
  constructor(input: T) {
    this.input = input;
  }
  with(...args: any[]): Chain<T> {
    if (this.result !== undefined) return this;
    const handler = args[args.length - 1];
    const patterns: T[] = [args[0]];
    let guard: ((value: T) => boolean) | undefined;
    if (args.length === 3 && typeof args[1] === "function") guard = args[1];
    else if (args.length > 2) patterns.push(...args.slice(1, args.length - 1));
    if (patterns.some((pattern) => pattern === this.input) && (guard ? guard(this.input) : true)) {
      this.result = handler(this.input);
    }
    return this;
  }
  run(): string {
    return this.result ?? "none";
  }
}

const match = <T>(input: T): Matcher<T> => new Chain(input) as unknown as Matcher<T>;

const describe = (n: number) =>
  match(n)
    .with(1, () => "one")
    .with(2, 3, (value) => `two-or-three:${value}`)
    .with(4, (value) => value > 3, () => "four")
    .run();

console.log(describe(1), describe(2), describe(3), describe(4), describe(5));
