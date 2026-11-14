// Four shapes from ts-pattern's library code:
// - a record field typed as a 3-tuple, recovered from an erased value;
// - a generic union whose arm is a generic class (`Some<T> | Nothing`);
// - a closure nested in a closure, both reading a variable the outer function
//   assigns on several paths (`select()`'s `pattern`);
// - `new` of a class declared later in the file, with no type arguments and a
//   parameter only a `never`-typed argument instantiates.
interface Located {
  path: [string, string, string];
  at: [number, string, boolean];
}

class Some<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
class Nothing {
  readonly empty = true;
}
type Maybe<T> = Some<T> | Nothing;

function wrap<T>(value: T, present: boolean): Maybe<T> {
  return present ? new Some(value) : new Nothing();
}

function describe(maybe: Maybe<number>): string {
  return maybe instanceof Some ? `some ${maybe.value}` : "nothing";
}

function selector(...args: unknown[]): () => (value: unknown) => string {
  const key = typeof args[0] === "string" ? args[0] : undefined;
  let pattern: unknown;
  if (args.length === 2) {
    pattern = args[1];
  } else if (typeof args[0] === "string") {
    pattern = undefined;
  } else {
    pattern = args[0];
  }
  return () => {
    const label = (value: unknown) => `${key ?? "anon"}:${String(pattern)}:${String(value)}`;
    return (value: unknown) => label(value);
  };
}

// An escaping closure that mutates its creator's second local. Closure
// captures name their creator's locals by per-function index, so this one
// must not decide how `selector`'s nested closures store theirs.
function counter(start: number): () => number {
  let count = start;
  return () => {
    count += 1;
    return count;
  };
}

type State<V> = { done: true; value: V } | { done: false; value: undefined };
const idle: State<never> = { done: false, value: undefined };

function start<I>(input: I): string {
  const machine = new Machine(input, idle) as any;
  return machine.describe();
}

class Machine<I, O> {
  private input: I;
  private state: State<O>;
  constructor(input: I, state: State<O>) {
    this.input = input;
    this.state = state;
  }
  describe(): string {
    return `done=${this.state.done}`;
  }
}

function main(): void {
  const raw: unknown = { path: ["a", "b", "c"], at: [3, "x", true] };
  const located = raw as Located;
  console.log(located.path.join("/"), located.at[0] + 1, located.at[1], located.at[2]);
  console.log(describe(wrap(4, true)), describe(wrap(5, false)), describe(new Some(6)));
  const next = counter(10);
  next();
  console.log(next());
  console.log(selector("k", 7)()(1), selector(9)()(2), selector("only")()(3));
  console.log(start(5), start("go"));
}

main();
