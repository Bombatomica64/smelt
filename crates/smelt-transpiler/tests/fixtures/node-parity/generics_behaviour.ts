// Generic classes, interfaces and functions: observable behaviour of the shapes the
// type-preservation tests pin, including every demotion path.

class Container<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
  set(value: T): void { this.value = value; }
}
const c = new Container<number>(3);
const aliased = c;
aliased.set(5);
console.log(c.get(), aliased.get());

class Pair<A, B> {
  first: A;
  second: B;
  constructor(first: A, second: B) { this.first = first; this.second = second; }
  getFirst(): A { return this.first; }
  getSecond(): B { return this.second; }
}
const p = new Pair<number, string>(1, "x");
console.log(p.getFirst(), p.getSecond());

function identity<T>(x: T): T { return x; }
function first<T>(xs: T[]): T { return xs[0]; }
function nestedWrap<T>(xs: T[]): T[][] { return [xs]; }
console.log(identity(3), identity("s"), first([7, 8]), nestedWrap([1, 2]).length);

function invalid<D extends Date>(value: D): boolean { return isNaN(value.getTime()); }
function flagsOf<R extends RegExp>(value: R): string { return value.flags; }
console.log(invalid(new Date(NaN)), invalid(new Date(0)), flagsOf(/a/gi));

class Animal {
  legs: number;
  constructor(legs: number) { this.legs = legs; }
  speak(): string { return "..."; }
}
class Dog extends Animal {
  speak(): string { return "woof"; }
}
function loud<A extends Animal>(a: A): A {
  console.log(a.speak(), a.legs);
  return a;
}
loud(new Animal(2));
loud(new Dog(4));
function grow<A extends Animal>(a: A): number {
  a.legs = a.legs + 1;
  return a.legs;
}
const grown = new Dog(4);
console.log(grow(grown), grown.legs);

function same<T>(a: T, b: T): boolean { return a === b; }
console.log(same(1, 1), same("a", "b"), same(grown, grown));

function difference<T>(firstArr: readonly T[], secondArr: readonly T[]): T[] {
  const secondSet = new Set(secondArr);
  return firstArr.filter(item => !secondSet.has(item));
}
console.log(difference([1, 2, 3], [2]), difference(["a"], ["a"]));

function takeWhile<F0>(xs: F0[], keep: (item: F0) => boolean): F0[] {
  return xs.filter(keep);
}
function both<T>(xs: T[], a: (item: T) => boolean, b: (item: T) => boolean): T[] {
  return xs.filter(a).filter(b);
}
console.log(takeWhile([1, 2, 3], x => x > 1), both([1, 2, 3], x => x > 1, x => x < 3));

function inner<T>(xs: T[], cb: (item: T) => boolean): T[] { return xs.filter(cb); }
function outer<T>(xs: T[], cb: (item: T) => boolean): T[] { return inner(xs, cb); }
console.log(outer(["a", "bb"], s => s.length > 1));

function attempt<T>(make: () => T): T[] { return [make()]; }
function unionBy<T, U>(arr1: T[], arr2: T[], mapper: (item: T) => U): T[] {
  const seen = new Set<U>();
  return arr1.concat(arr2).filter(item => {
    const key = mapper(item);
    if (seen.has(key)) { return false; }
    seen.add(key);
    return true;
  });
}
console.log(attempt(() => 9), unionBy([1, 2], [2, 3], (item: number) => item.toString()));

function uniqWith<T>(xs: T[], eq: (a: T, b: T) => boolean): T[] {
  const seen: T[] = [];
  for (const item of xs) {
    if (!seen.some((other) => eq(item, other))) {
      seen.push(item);
    }
  }
  return seen;
}
console.log(uniqWith([1, 2, 1, 3, 2], (a, b) => a === b));

function groupKeys<T, K extends string>(xs: T[], key: (x: T) => K): K[] {
  return xs.map(key);
}
console.log(groupKeys([1, 2], x => (x > 1 ? "big" : "small")));

function pick<T>(x: T, cb: (v: T) => boolean = () => true): T {
  return cb(x) ? x : x;
}
console.log(pick(1), pick("kept", s => s.length > 100));

function keepCallback<T>(x: T, cb: (v: T) => boolean): (v: T) => boolean {
  return cb;
}
const positive = keepCallback<number>(0, (v: number) => v > 0);
console.log(positive(1), positive(-1));

function makeAlways<T>(x: T): (v: T) => boolean {
  return (v: T) => true;
}
console.log(makeAlways("x")("y"));

function run<T>(items: T[], cb: (v: number) => number): T | number {
  const touched: number = cb(1);
  if (touched < 0) {
    return items[0];
  }
  const out: T = items[0];
  return out;
}
console.log(run([1, 2, 3], (v: number) => v + 1), run(["a"], (v: number) => -v));

function count<T>(items: T[]): number | string {
  return items.length;
}
console.log(count([1, 2, 3]));

function viaList<T>(items: T[], cbs: ((v: T) => T)[]): T {
  const out: T = cbs[0](items[0]);
  return out;
}
function viaRest<T>(items: T[], ...cbs: ((v: T) => T)[]): T {
  const out: T = cbs[0](items[0]);
  return out;
}
function packs(cbp0: (v: number) => number): number {
  const items0: number[] = [1, 2, 3];
  return viaList(items0, [cbp0]) + viaRest(items0, cbp0);
}
console.log(packs(v => v * 10));

class Cache<T> {
  data: Map<T, string> = new Map();
  get(key: T): string | undefined { return this.data.get(key); }
  set(key: T, value: string): void { this.data.set(key, value); }
}
const cache = new Cache<number>();
cache.set(1, "one");
console.log(cache.get(1), cache.get(2));

class Chain<T> {
  value: T;
  rewind: () => Chain<T>;
  constructor(value: T) {
    this.value = value;
    this.rewind = () => { this.value = value; return this; };
  }
}
const chain = new Chain<number>(1);
chain.value = 2;
console.log(chain.rewind().value);

class Cell<T> {
  value: T;
  constructor(value: T) { this.value = value; }
}
class Wrap<A> {
  left: Cell<A>;
  constructor(left: Cell<A>) { this.left = left; }
}
console.log(new Wrap<number>(new Cell<number>(7)).left.value);

class Tagged<Tag> {
  label: string;
  constructor(label: string) { this.label = label; }
  read(): string { return this.label; }
}
console.log(new Tagged<number>("tag").read());

class Sink<T> {
  count: number;
  constructor() { this.count = 0; }
  accept(value: T): number { this.count = this.count + 1; return this.count; }
}
const sink = new Sink<string>();
sink.accept("a");
console.log(sink.accept("b"));

// Erased paths: inspected, spread, keyed and mutated generics.
function copyEntries<T>(value: T): unknown[] {
  const copied: unknown[] = [];
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      copied[index] = item;
    }
  }
  return copied;
}
console.log(copyEntries([1, "b"]), copyEntries(3));

function firstChar<S extends string>(value: S): string { return value[0]; }
console.log(firstChar("abc"));

function keyedEntries(key: string): unknown {
  const values = new Map<unknown, unknown>([[key, "ready"]]);
  return Object.fromEntries(values);
}
console.log(keyedEntries("k"));

type IterableContainer<T> = readonly T[];
const numberComparator = (a: number, b: number): number => a - b;
function sortedSpread<T extends IterableContainer<number>>(data: T): unknown {
  return [...data].sort(numberComparator);
}
console.log(sortedSpread([3, 1, 2]));

function concatImplementation<T1, T2>(arr1: T1, arr2: T2): unknown[] {
  return [...arr1 as unknown[], ...arr2 as unknown[]];
}
console.log(concatImplementation([1], ["two"]));

function indexPresent(arr: number[], i: number): boolean { return Object.hasOwn(arr, i); }
function keyPresent(obj?: Record<string, number>, key?: string): boolean {
  return obj != null && key != null && Object.hasOwn(obj, key);
}
function genericKey<T extends object>(object: T, key: string): boolean {
  return Object.hasOwn(object, key);
}
console.log(indexPresent([1, 2], 1), indexPresent([1, 2], 2));
console.log(keyPresent({ a: 1 }, "a"), keyPresent({ a: 1 }, "b"), keyPresent(undefined, "a"));
console.log(genericKey({ a: 1 }, "a"), genericKey({ a: 1 }, "b"));

function keysOf<T>(object: T): string[] {
  const out: string[] = [];
  for (const key in object) {
    out.push(key);
  }
  return out;
}
console.log(keysOf({ x: 1, y: 2 }));

function eraseInto(target: unknown[], value: unknown): void { target.push(value); }
function pushTyped<T>(arr: T[], value: T): T[] {
  eraseInto(arr, value);
  return arr;
}
const pushedNums: number[] = [1, 2, 3];
pushTyped(pushedNums, 4);
console.log(pushedNums, pushTyped<string>([""], "a"));

function dropFirst<T>(arr: T[]): T[] { return arr.splice(0, 1); }
const droppedFrom: number[] = [1, 2, 3];
console.log(dropFirst(droppedFrom), droppedFrom);

class Holder<T> {
  value: T;
  constructor(value: T) { this.value = value; }
}
function takeHeld<T>(arr: T[]): Holder<T> { return new Holder(arr.splice(0, 1)[0]); }
const heldFrom: number[] = [4, 5];
const held = takeHeld(heldFrom);
console.log(held.value + heldFrom.length);

class AppBase<E = string> {
  prefix: string = "";
  handle: (path: string) => string;
  constructor(label: string) {
    this.handle = (path: string) => label + this.prefix + path;
  }
}
class App<E = string> extends AppBase<E> {
  constructor() {
    super("app");
    this.prefix = "/api";
  }
}
console.log(new App<number>().handle("/x"));

class BaseBox<T> {
  value: T;
  constructor(value: T) { this.value = value; }
}
class StringBox extends BaseBox<string> {}
console.log(new StringBox("value").value);

function mapArr<T, U>(arr: T[], fn: (item: T) => U): U[] {
  const out: U[] = [];
  for (const item of arr) { out.push(fn(item)); }
  return out;
}
console.log(mapArr([1, 2, 3], (x) => x * 2));

function pickFirst<T, K extends string>(arr: T[], key: (item: T) => K, fallback: T): T {
  if (arr.length > 0) { return arr[0]; }
  return fallback;
}
console.log(pickFirst([1, 2], (n) => (n > 1 ? "big" : "small"), 0), pickFirst([] as number[], () => "k", 9));

function scan<T, K extends string>(arr: T[], key: (item: T) => K): T[] {
  const out: T[] = [];
  arr.forEach((item) => { if (typeof item === "string") { out.push(item); } });
  return out;
}
console.log(scan(["a", "b"], (s) => "k"), scan([1], () => "k"));

function collectRows<T, K extends string>(rows: T[][], key: (vals: T[]) => K): K[] {
  const out: K[] = [];
  for (let i = 0; i < rows.length; i++) {
    const group = new Array(rows.length);
    for (let j = 0; j < rows.length; j++) { group[j] = rows[j][i]; }
    out.push(key(group));
  }
  return out;
}
console.log(collectRows([[1, 2]], (v) => (v.length > 0 ? "some" : "none")));

interface Picker {
  <K extends string>(key: K): K;
}
class PickerHolder {
  pick: Picker = <K extends string>(key: K): K => key;
  direct: <K extends string>(key: K) => K = <K extends string>(key: K): K => key;
}
const pickerHolder = new PickerHolder();
console.log(pickerHolder.pick("a") + pickerHolder.direct("b"));

function repeat<T>(x: T, n: number): T { return n > 0 ? repeat(x, n - 1) : x; }
console.log(repeat("end", 3));
