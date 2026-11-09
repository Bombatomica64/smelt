// Try and catch in every region shape: loops, closures, if arms, and adapters.

function flaky(n: number): number {
  if (n < 3) {
    throw new Error("nope");
  }
  return n;
}

function attempt(limit: number): number {
  for (let i = 0; i <= limit; i++) {
    try {
      return flaky(i);
    } catch (err) {
      // retry with the next index
    }
  }
  return -1;
}
console.log("loop retry", attempt(5), attempt(1));

function outer(n: number): number {
  const inner = (limit: number): number => {
    for (let i = 0; i <= limit; i++) {
      try {
        return flaky(i);
      } catch (err) {}
    }
    return -1;
  };
  return inner(n);
}
console.log("closure retry", outer(5), outer(0));

function thrower(x: number): string {
  if (x > 0) {
    throw new Error("boom");
  }
  return "ok";
}
function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return "caught";
  }
}
console.log("adapted thrower", guard(thrower, 1), guard(thrower, 0));

function guarded(func: () => any, recover: (err: any) => any): any {
  const isPromise = (result: any): result is Promise<any> => result instanceof Promise;
  try {
    const result = func();
    return isPromise(result) ? result.catch(recover) : result;
  } catch (err) {
    return recover(err);
  }
}
console.log(
  "if arm",
  guarded(
    () => 1,
    () => 2,
  ),
  guarded(
    () => {
      throw new Error("x");
    },
    () => 3,
  ),
);

function mapTry(values: string[]): Array<string | undefined> {
  return values.map((value) => {
    try {
      if (value === "bad") {
        throw new Error(value);
      }
      return value;
    } catch (e) {
      return undefined;
    }
  });
}
console.log("map try", JSON.stringify(mapTry(["a", "bad", "c"])));

try {
  throw "boom";
} catch (err: any) {
  console.log("string throw", err);
}

const defaults: Record<string, unknown> = { weekStartsOn: 3 };
interface Options {
  weekStartsOn?: number;
}
function read(options?: Options): number {
  return (options?.weekStartsOn ?? defaults.weekStartsOn ?? 0) as number;
}
console.log("coalesce", read(), read({}), read({ weekStartsOn: 1 }), read({ weekStartsOn: 0 }));

function pick(arr: number[], index: number): number | undefined {
  return arr[index];
}
console.log("element read", pick([1, 2], 1), pick([1, 2], 5), pick([1, 2], -1), pick([], -1));

type Locale = {
  formatDistance: (token: string) => string;
};
const localizeDistance = (token: string) => {
  if (token !== "x") {
    throw new Error("bad token " + token);
  }
  return "ok";
};
const locale: Locale = { formatDistance: localizeDistance };
console.log("function field", locale.formatDistance("x"));
try {
  locale.formatDistance("y");
} catch (error) {
  console.log("function field throw", (error as Error).message);
}
