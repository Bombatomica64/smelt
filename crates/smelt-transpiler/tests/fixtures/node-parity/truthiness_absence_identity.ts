// Truthiness, nullish fallbacks, non-null forwarding, list aliasing and method
// identity, observed through values.

function keepNumber(b?: number): number {
  const d = b ?? null;
  if (d) return d + 1;
  return -1;
}
function keepFunction(b?: (count: number) => number): number {
  const backoff = b ?? null;
  if (backoff) return backoff(2);
  return -1;
}
console.log(keepNumber(), keepNumber(0), keepNumber(3));
console.log(keepFunction(), keepFunction((count: number) => count * 10));

function orDefault<TDefault = unknown>(value: any, defaultValue?: TDefault): TDefault {
  if (value === null) return defaultValue as TDefault;
  return value;
}
console.log(orDefault<number>(null) === undefined, orDefault<number>(null, 5), orDefault<number>(7, 5));

function count(keys: string[]): number {
  return keys.length;
}
function useCount(): number {
  const t: [string, string] = ["a", "b"];
  return count([] as []) + count(t);
}
console.log(useCount());

function size(obj: Record<string, number>): number {
  if (!obj) return -1;
  return Object.keys(obj).length;
}
function itemCount(items: number[]): number {
  if (!items) return -1;
  return items.length;
}
console.log(size({}), size({ a: 1 }), itemCount([]), itemCount([1, 2]));

function compact<T>(items: T[]): T[] {
  const out: T[] = [];
  for (const item of items) {
    if (item) {
      out.push(item);
    }
  }
  return out;
}
console.log(compact([0, 1, NaN, 2]).length, compact(["", "a"]).length, compact([false, true]).length);

function describeHandler(handler: () => void): string {
  if (handler) {
    handler();
    return "yes";
  }
  return "no";
}
console.log(describeHandler(() => {}));

function firstDisabled(rows: Record<string, unknown>[]): number {
  return rows.findIndex(row => !row.enabled);
}
console.log(
  firstDisabled([{ enabled: true }, { enabled: false }]),
  firstDisabled([{ enabled: 1 }, { enabled: 0 }]),
  firstDisabled([{ enabled: "x" }, { enabled: "" }]),
  firstDisabled([{ enabled: true }, {}]),
  firstDisabled([{ enabled: true }]),
);

function isBool(value: unknown): boolean {
  return typeof value === "boolean";
}
console.log(isBool(false), isBool(true), isBool(0), isBool("false"));

function countOptional(values?: string[]): number {
  return Array.isArray(values) ? values.length : 0;
}
console.log(countOptional(), countOptional([]), countOptional(["a", "b"]));

function isList(value: unknown): boolean {
  return Array.isArray(value);
}
console.log(isList([]), isList([1]), isList({}), isList("[]"), isList(null));

function span(minimum: number, maximum?: number): number {
  if (maximum == null) {
    return minimum;
  }
  return maximum - minimum;
}
function spanFrom(minimum: number, maximum?: number): number {
  return span(minimum, maximum!);
}
console.log(spanFrom(4), spanFrom(4, 9));

function dropFirst<T>(target: T[]): void {
  target.splice(0, 1);
}
function useDropFirst(): number {
  const values: unknown[] = [1, 2, 3];
  dropFirst(values);
  return values.length;
}
console.log(useDropFirst());

function undefinedHolder(): unknown {
  const holder = { a: undefined };
  return holder;
}
function nullHolder(): unknown {
  const holder = { a: null };
  return holder;
}
function entryIs(holder: unknown): string {
  const entry = (holder as any).a;
  if (entry === undefined) return "undefined";
  if (entry === null) return "null";
  return "other";
}
console.log(entryIs(undefinedHolder()), entryIs(nullHolder()));
console.log(JSON.stringify({ a: undefined }), JSON.stringify({ a: null }));

class Greeter {
  greet(): string {
    return "hi";
  }
}
class Other {
  greet(): string {
    return "other";
  }
}
function methodOf(greeter: unknown): unknown {
  return (greeter as any).greet;
}
const greeter = new Greeter();
console.log(
  methodOf(greeter) === methodOf(greeter),
  methodOf(greeter) === methodOf(new Greeter()),
  methodOf(greeter) === methodOf(new Other()),
);

class Holder {
  value = 1;
}
function readMissing(holder: Holder): unknown {
  return (holder as any)["nope"];
}
console.log(readMissing(new Holder()) === undefined, readMissing(new Holder()) === null);

function attempt(func: () => unknown): [unknown, unknown] {
  return [null, func()];
}
async function run(): Promise<unknown> {
  const [, result] = attempt(async () => 1);
  return await result;
}
run().then(value => console.log(value));
