// Callable objects, overload slots, rest-callback adapters, erased function ABI.

interface Counter {
  (...args: number[]): number;
  reset: () => void;
}
function makeCounter(): Counter {
  let count = 0;
  const reset = () => {
    count = 0;
  };
  const counter = function (...args: number[]) {
    count = count + args.length;
    return count;
  };
  counter.reset = reset;
  return counter as Counter;
}

interface Tick {
  (): number;
  reset(): void;
}
function makeTick(): Tick {
  let count = 0;
  const counter = function (): number {
    count = count + 1;
    return count;
  };
  counter.reset = function (): void {
    count = 0;
  };
  return counter as Tick;
}

function applyTo(func: unknown, args: unknown[]): unknown {
  return (func as (...values: unknown[]) => unknown).apply(null, args);
}

function wrapCall<F extends (...args: any) => any>(fn: F): F {
  const wrapped = function (this: unknown, arg: Parameters<F>[0]): ReturnType<F> {
    return fn.call(this, arg);
  };
  return wrapped as F;
}

interface Memo {
  (value: string): string;
  cache: number;
}
function makeMemo(fn: (value: string) => string): Memo {
  const memoized = function (value: string): string {
    return fn(value);
  };
  memoized.cache = 0;
  return memoized as Memo;
}

interface Wrapped {
  (value: string): string;
  tag: number;
}
function wrap(fn: (value: string) => string): Wrapped {
  const wrapped = function (value: string): string {
    return fn(value);
  };
  wrapped.tag = 1;
  return wrapped as Wrapped;
}

interface Setter {
  (key: string, value: number): void;
  (key: string, value: string): void;
}
interface Header {
  (name: string): void;
  (name: string, value: string, append?: boolean): void;
}
class Store {
  seen: string[] = [];
  set: Setter = (key: string, value: number | string) => {
    this.seen.push(key + "=" + String(value));
  };
  header: Header = (name: string, value?: string, append?: boolean) => {
    this.seen.push(name + ":" + (value ?? "") + (append ? "+" : ""));
  };
}

function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...(args as [unknown, ...unknown[]]))
    : implementation(args[0], args[1], args.slice(2));
}

function callConcrete(
  callback: (data: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return callback(args[0], ...args.slice(1));
}

function restLengths(): void {
  const rest = callConcrete((data, ...others) => others, [1, 2, 3]) as unknown[];
  console.log(rest.length, rest[0]);
  const erased: unknown = (data: unknown, values: Array<unknown>) => values;
  console.log(((erased as (d: unknown, v: unknown[]) => unknown[])(0, [5, 6])).length);
  const late = purryOn((x) => x === "late", (d, f, ...r) => [d, f, r.length], ["late", 2]) as (d: unknown) => unknown[];
  console.log(late("d").join());
  const eager = purryOn(() => false, (d, f, r) => [d, f, (r as unknown[]).length], ["e", 2, 3, 4]) as unknown[];
  console.log(eager.join());
}

function cartesian<T>(arr1: readonly T[]): Array<[T]>;
function cartesian<T, U>(arr1: readonly T[], arr2: readonly U[]): Array<[T, U]>;
function cartesian<T>(...arrs: Array<readonly T[]>): T[][];
function cartesian<T>(...arrs: Array<readonly T[]>): T[][] {
  return arrs.map((arr) => [...arr]);
}

function sinkRows(rows: unknown[][]): number {
  return rows.map((r) => r.length).reduce((a, b) => a + b, 0);
}

function eraseInto(target: unknown[], value: unknown): void {
  target.push(value);
}

type Entry = [string, number];
class Bag {
  seen: string = "";
  take(entry: Entry): string {
    this.seen = entry[0] + entry[1];
    return this.seen;
  }
}

type Triple = [string, number[], boolean];
function buildTriple(label: string, flag: boolean): Triple | null {
  if (!flag) {
    return null;
  }
  return [label, [1, 2], flag];
}

type Holder = { index: number } & { items: unknown[] };
function record(holder: Holder, value: unknown): void {
  const { items } = holder;
  items.push(value);
}

interface Router<T> {
  name: string;
  tag: T;
  lookup(method: string): string;
}
function eraseRouter(router: Router<string>): any {
  const { name, ...rest } = router;
  return rest;
}

class Person {
  name: string = "";
  age: number = 0;
}
function firstOf<T>(items: T[], fallback: T): T {
  if (items.length > 0) {
    return items[0];
  }
  return fallback;
}

interface Pair<T> {
  left: T;
  right: T;
  same: boolean;
  swap(): Pair<T>;
}
function makeChecked<T>(left: T, right: T): Pair<T> {
  return { left, right, same: left === right, swap() { return makeChecked(right, left); } };
}

function allKeys(source: any): any[] {
  return [...Object.keys(source), ...Object.getOwnPropertySymbols(source)];
}

interface Init {
  status?: number;
  statusText?: string;
  label?: string;
}
function describeInit(init: Init): string {
  return String(init.status) + "/" + String(init.statusText) + "/" + String(init.label);
}

function maxWidth(rows: unknown[][]): number {
  return Math.max(...rows.map((r) => r.length));
}

function main(): void {
  const counter = makeCounter();
  counter();
  console.log(counter(1, 2));
  counter.reset();
  console.log(counter(5));
  const tick = makeTick();
  tick();
  console.log(tick());
  tick.reset();
  console.log(tick());
  console.log(applyTo((a: unknown, b: unknown) => String(a) + String(b), [1, 2]));
  const adder = wrapCall((x: number) => x + 10);
  console.log(adder(5));
  const memoized = makeMemo((v) => v + "!");
  console.log(["a", "b"].map((value) => memoized(value)).join());
  console.log(wrap((v) => v.toUpperCase())("q"), wrap((v) => v).tag);
  const store = new Store();
  store.set("a", 1);
  store.set("b", "two");
  store.header("x");
  store.header("y", "z", true);
  console.log(store.seen.join());
  restLengths();
  console.log(cartesian(...[[1], [2, 3]]).length);
  const pairs: Array<[number, number]> = [[1, 2]];
  console.log(sinkRows(pairs));
  const xs: number[] = [1, 2];
  const alias = xs;
  eraseInto(xs, 3);
  console.log(xs.join(), alias.length);
  console.log(new Bag().take(["a", 1]));
  console.log(buildTriple("l", true), buildTriple("l", false) === null);
  const holder: Holder = { index: 0, items: [] };
  record(holder, 1);
  record(holder, 2);
  console.log(holder.items.length);
  const router: Router<string> = { name: "r", tag: "t", lookup: (method) => "m:" + method };
  console.log(eraseRouter(router).lookup("GET"));
  const people: Person[] = [];
  console.log(firstOf(people, new Person()).age);
  console.log(makeChecked(1, 2).swap().left, makeChecked(1, 2).swap().right, makeChecked(1, 1).same);
  const sym = Symbol("s");
  console.log(allKeys({ a: 1, [sym]: 2 }).length);
  console.log(describeInit(new Response("hi", { status: 201, statusText: "made" })));
  console.log(maxWidth([[1], [1, 2]]));
}

main();
