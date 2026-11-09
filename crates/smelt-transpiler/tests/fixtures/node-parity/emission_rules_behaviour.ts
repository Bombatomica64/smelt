// Option bags, module slots, IIFE typing, truthiness, generators, sort, statics and
// boxed primitives, observed end to end.

function pick(
  { leading = false, trailing = true, maxWait }: { readonly leading?: boolean; readonly trailing?: boolean; readonly maxWait?: number } = {},
): string {
  return `${leading},${trailing},${maxWait}`;
}
console.log(pick(), pick({ leading: true, maxWait: 5 }), pick({ trailing: false }));

function format(value: string): string { return value + "!"; }
export { format as formatDate };
console.log(format("ok"));

const label = "scale";
const scaled = (value: number): string => `${label}:${value * 2}`;
const useScaled = (value: number): string => applyScaled(scaled, value);
function applyScaled(f: (value: number) => string, value: number): string { return f(value); }
console.log(useScaled(3));

let annotated: number | undefined;
annotated = 5;
let initialized: number = 1;
initialized = 6;
console.log(annotated, initialized);

type Sizer = (value: string) => number;
let viaAssign: Sizer | undefined;
viaAssign = (() => { const offset = 10; return (value: string) => value.length + offset; })();
let viaOrAssign: Sizer | undefined;
viaOrAssign ||= (() => { const offset = 20; return (value: string) => value.length + offset; })();
const direct: Sizer = (() => { const offset = 30; return (value) => value.length + offset; })();
console.log(viaAssign("a"), viaOrAssign("ab"), direct("abc"));

function annotatedIife(): number {
  const value: number = (function (): number { return 3; })();
  return value;
}
console.log(annotatedIife());

interface WeekOptions { weekStartsOn?: number; }
interface FirstWeekOptions { firstWeekContainsDate?: number; }
interface LocaleOptions extends WeekOptions, FirstWeekOptions {}
function readLocale(options: LocaleOptions): number {
  return options.weekStartsOn ?? options.firstWeekContainsDate ?? 0;
}
console.log(readLocale({ weekStartsOn: 1 }), readLocale({ firstWeekContainsDate: 4 }), readLocale({}));

function consume(value: unknown): boolean { return value === null; }
console.log(consume(null as unknown as never));

function countPresent(values: string[]): Record<string, number> {
  const out = new Map<string, number>();
  for (const value of values) {
    const count = out.get(value);
    if (count === undefined) { out.set(value, 1); } else { out.set(value, count + 1); }
  }
  return Object.fromEntries(out);
}
const counted = countPresent(["a", "b", "a"]);
console.log(counted["a"], counted["b"], Object.keys(counted).length);

function isZero(amount: number): boolean { return !amount; }
function truthyNaN(): boolean { return Boolean(NaN); }
console.log(isZero(0), isZero(NaN), isZero(2), truthyNaN());

function sortKeysLike(keys: string[], compare?: (a: string, b: string) => number): string[] {
  return keys.slice().sort(compare);
}
console.log(sortKeysLike(["b", "c", "a"]), sortKeysLike(["b", "c", "a"], (a, b) => (a < b ? 1 : -1)));

function collate(a: string, b: string): number { return a.localeCompare(b); }
console.log(collate("a", "b"), collate("b", "a"), collate("a", "a"));

function* sequence(): Generator<number, string, unknown> {
  yield 1;
  yield 2;
  return "done";
}
function* delegated(): Generator<number, string, unknown> { return yield* sequence(); }
class IterableValue {
  value = 3;
  *[Symbol.iterator](): Generator<number, string, unknown> {
    yield this.value;
    return "iterable done";
  }
}
function* delegatedIterable(value: IterableValue): Generator<number, string, unknown> { return yield* value; }
function delayedThrow(message: string): Generator<number, string, unknown> {
  return (function* (): Generator<number, string, unknown> {
    yield 4;
    throw new Error(message);
  })();
}
const iterator = sequence();
const first = iterator.next();
console.log(first.value, first.done, iterator.next().value, iterator.next().value, iterator.next().done);
console.log([...delegated()], [...delegatedIterable(new IterableValue())]);
const failing = delayedThrow("late");
console.log(failing.next().value);

function stopWhenSorted(items: number[]): number {
  let index = 0;
  while (index < items.length) {
    if (items[index]! >= 0) { return index; }
    index += 1;
  }
  return -1;
}
function voidReturn(items: number[]): void {
  for (const item of items) { if (item > 0) { console.log("hit", item); return; } }
  console.log("none");
}
console.log(stopWhenSorted([-1, 2]), stopWhenSorted([-1]));
voidReturn([0, 3, 4]);
voidReturn([0]);

function chooseValues(flag: boolean): unknown[] {
  let values: unknown[];
  if (flag) { values = ["ready"]; } else { values = []; }
  return values;
}
function chooseNumber(flag: boolean): number {
  let result: number;
  if (flag) { result = 1; } else { result = 2; }
  return result;
}
console.log(chooseValues(true).length, chooseValues(false).length, chooseNumber(true), chooseNumber(false));

class WithIn { in?: (value: number) => number; }
function consumeIn(context?: (value: number) => number): number { return context ? context(1) : -1; }
function runIn(options?: WithIn): number { return consumeIn(options?.in); }
const withIn = new WithIn();
withIn.in = (value) => value + 41;
console.log(runIn(), runIn(new WithIn()), runIn(withIn));

function instanceofBoolean(value: any): boolean { return value instanceof Boolean; }
function instanceofString(value: any): boolean { return value instanceof String; }
console.log(instanceofBoolean(new Boolean(true)), instanceofBoolean(true), instanceofString(new String("a")), instanceofString("a"));
console.log(Object.keys(new Boolean(true)).length, Object.keys(new Number(1)).length);

const controller = new AbortController();
const signal = controller.signal;
let fired = 0;
signal.addEventListener("abort", () => { fired += 1; });
console.log(signal.aborted, controller instanceof AbortController, signal instanceof AbortSignal);
controller.abort();
console.log(signal.aborted, fired);

const SAFE = Number.MAX_SAFE_INTEGER;
function ceiling(): number { return SAFE; }
console.log(ceiling());

function fillRange<T>(array: T[] | null | undefined, value: T, start = 0, end = array ? array.length : 0): number[] {
  start = Math.floor(start);
  end = Math.floor(end);
  if (!start) { start = 0; }
  if (!end) { end = 0; }
  return [start, end];
}
console.log(fillRange([1, 2], 0), fillRange(null, 0, 1.5, 2.5));

class Circle { tag: string = "c"; radius: number = 1; }
class Square { side: number = 2; }
function describeShape(shape: Circle | Square): number {
  if ("tag" in shape && shape.tag === "c") { return shape.radius; }
  return 0;
}
console.log(describeShape(new Circle()), describeShape(new Square()));

class Config {
  static readonly LIMIT: number = 42;
  static readonly NAME: string = "smelt";
}
console.log(Config.LIMIT, Config.NAME);

function orderRight(values: string[]): string { return values.reduceRight((acc, value, index) => acc + value + index, ""); }
function lastMinusRest(values: number[]): number { return values.reduceRight((acc, value) => acc - value); }
console.log(orderRight(["a", "b"]), lastMinusRest([1, 2, 10]));

function firstGroup(input: string): string | undefined {
  const m = /(?<result>\w+)/.exec(input);
  return m?.groups?.result;
}
console.log(firstGroup("hello world"), firstGroup("!!!"));

function joinTruthy(parts: (string | undefined)[]): string {
  let out = "";
  for (const part of parts) { if (part) { out += part; } }
  return out;
}
console.log(joinTruthy(["a", undefined, "", "b"]));

function unzipWith<T, R>(target: readonly T[][], iteratee: (...args: T[]) => R): R[] {
  return target.map((group) => iteratee(...group));
}
console.log(unzipWith([[1, 2, 3], [4, 5, 6]], (item, item2, item3) => item + item2 + item3));

function lastVal(items: number[]): number {
  let found: number = 0;
  for (const x of items) { found = x; }
  return found;
}
console.log(lastVal([1, 2]), lastVal([]));

class Sized {
  size: number;
  constructor(size = 10) { this.size = size; }
}
console.log(new Sized().size, new Sized(3).size);

function clearSlot(handle: unknown): number {
  let slot: unknown | null = null;
  slot = handle;
  slot = null;
  if (slot !== null) { return 1; }
  return 0;
}
console.log(clearSlot(1));
