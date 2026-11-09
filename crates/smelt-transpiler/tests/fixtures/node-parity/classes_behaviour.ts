// Classes: index signatures, erased projection, statics, typed arrays, keyed members
// and inheritance, observed end to end.

class StringBag {
  [key: string]: string;
}
class MixedBag {
  size: number = 2;
  [key: string]: unknown;
}
function readBag(bag: StringBag, key: string): string | undefined { return bag[key]; }
function writeBag(bag: StringBag, key: string, value: string): void { bag[key] = value; }
const bag = new StringBag();
writeBag(bag, "a", "x");
console.log(readBag(bag, "a"), readBag(bag, "missing"), new MixedBag().size);

class PrivateBox {
  private value: string = "hidden";
  shown: string = "visible";
  read(): string { return this.value; }
}
class GetterBox {
  shown: number = 1;
  get value(): number { return this.shown + 1; }
}
function keyCount(item: unknown): number { return Object.keys(item as object).length; }
console.log(keyCount(new PrivateBox()) >= 1, keyCount(new GetterBox()), new GetterBox().value, new PrivateBox().read());

console.log(["a"].map(() => Object.entries({ x: 1 })), [[["k", 2]] as [string, number][]].map(Object.fromEntries));

function makeError(message: string): unknown { return new Error(message); }
const err = makeError("boom") as Error;
console.log(err instanceof Error, err.message, Object.keys(err).length);

function make(count: number): number {
  const view = new Uint8Array(count);
  view[0] = 257;
  return view[0] + view.length + new Uint16Array(count).byteLength;
}
console.log(make(3));
const big = new BigUint64Array(3);
console.log(big.length, big.byteLength);

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
}
class Point3 extends Point {}
const point = new Point3(1, 2);
console.log(point.x, point.y);

const tag = Symbol();
class Holder {
  [tag]: number = 41;
}
function readTag(h: Holder): number { return h[tag] + 1; }
console.log(readTag(new Holder()));

class Body {
  json(): string { return "json-body"; }
  text(): string { return "text-body"; }
}
function readBody(body: Body, key: "json" | "text"): string { return body[key](); }
console.log(readBody(new Body(), "json"), readBody(new Body(), "text"));

class Parser {
  set(date: number, value: number): number { date = date + value; return date; }
}
class Reassign {
  value: number;
  constructor(value: number) { value = value + 1; this.value = value; }
}
console.log(new Parser().set(1, 2), new Reassign(1).value);

class OptionalPoint {
  x: number;
  y?: number;
  constructor(x: number, y?: number) { this.x = x; this.y = y; }
}
console.log(new OptionalPoint(1, 2).y ?? 0, new OptionalPoint(3).y ?? 0);

class MathUtils {
  static square(value: number): number { return value * value; }
}
console.log(MathUtils.square(4));

function pad<S extends string>(value: S): string { return value.padStart(3, "0"); }
function firstChar<S extends string>(value: S): string { return value.charAt(0); }
function repeated(value: string, count: number): string { return value.repeat(count); }
function startsAt(value: string, needle: string, position?: number): boolean { return value.startsWith(needle, position); }
console.log(pad("7"), firstChar("ab"), repeated("xy", 2), startsAt("abc", "b", 1), startsAt("abc", "b"));
console.log(["a"].map(s => s.repeat(2)));

const falsey = [0, null];
const empties = [[], {}].concat(falsey.slice(1) as never[]);
console.log(empties.length);

class Base {
  message: string;
  constructor(message: string) { this.message = message; }
}
class Child extends Base {
  extra: number;
  constructor(message: string) { super(message + "!"); this.extra = 1; }
}
const child = new Child("hi");
console.log(child.message, child.extra);

class A {
  log: string[];
  constructor() { this.log = ["A"]; }
}
class B extends A {
  constructor() { super(); this.log.push("B"); }
}
class C extends B {
  constructor() { super(); this.log.push("C"); }
}
console.log(new C().log);
