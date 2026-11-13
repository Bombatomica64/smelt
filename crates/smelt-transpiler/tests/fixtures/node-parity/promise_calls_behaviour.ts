// Promises, erased calls and the object runtime they reach.

// Array mutators.
const popped: number[] = [1, 2];
popped.pop();
console.log(popped.pop(), popped.pop());
const shifted: string[] = ["a", "b"];
shifted.shift();
console.log(shifted.shift(), shifted.shift());

// Erased slice dispatches on the runtime value.
function cloneSlice(obj: unknown): unknown {
  return (obj as any).slice(0);
}
const erasedArray = [1, 2];
const sliced = cloneSlice(erasedArray) as number[];
sliced.push(3);
console.log(erasedArray.length, sliced.length, cloneSlice("abc"));

// Erased iteration over a Map.
function toEntries(x: unknown): unknown[] {
  const out: unknown[] = [];
  for (const e of x as any) {
    out.push(e);
  }
  return out;
}
console.log(toEntries(new Map([["k", 1]])).length, toEntries(new Set([1, 2])).length, toEntries([1]).length);

// Array.isArray.
const typedList: number[] = [1, 2, 3];
console.log(Array.isArray(typedList), Array.isArray(1));
const isArrayRef = Array.isArray;
console.log(isArrayRef([1]), isArrayRef("value"));
function isArrayValue(value?: any): boolean {
  return Array.isArray(value);
}
console.log(isArrayValue([1]), isArrayValue(1), isArrayValue());

// Object projections.
const mapping: Record<string, number> = { a: 1, b: 2 };
console.log(Object.keys(mapping).join(","), Object.values(mapping).join(","), Object.entries(mapping).length);
const rebuilt = Object.fromEntries([["a", 1], ["b", 2]]);
console.log(rebuilt.a + rebuilt.b);

// Object.assign.
const assignSource: Record<string, number> = { a: 1 };
const merged = Object.assign({}, assignSource, { b: 2 });
console.log(merged.a, merged.b);
function mergeInto(source: Record<string, number>): Record<string, number> {
  const result: Record<string, number> = {};
  Object.assign(result, source);
  return result;
}
console.log(mergeInto({ x: 9 }).x);

// Object.hasOwn and hasOwnProperty.
console.log(Object.hasOwn(mapping, "a"), mapping.hasOwnProperty("z"));
function findKey<Value, Obj extends { [key in string | number]: Value }>(
  object: Obj,
  predicate: (value: Value) => boolean,
): string | undefined {
  for (const key in object) {
    if (Object.prototype.hasOwnProperty.call(object, key) && predicate(object[key])) {
      return key;
    }
  }
  return undefined;
}
console.log(findKey({ a: 1, b: 2 }, (v: number) => v === 2));

// The prototype chain ends at null.
function protoDepth(value: object): number {
  let depth = 0;
  let proto = Object.getPrototypeOf(value);
  while (Object.getPrototypeOf(proto) !== null) {
    proto = Object.getPrototypeOf(proto);
    depth = depth + 1;
  }
  return depth;
}
console.log(protoDepth({}), protoDepth([]));

// Fall-through and explicit returns at an erased return type.
function fallsThrough(value: unknown): unknown {
  if (typeof value === "number") {
    return value;
  }
}
function returnsNull(value: unknown): unknown {
  if (typeof value === "number") {
    return value;
  }
  return null;
}
console.log(fallsThrough("s") === undefined, returnsNull("s") === null);

// Boxing and valueOf.
function unboxed(value: unknown): unknown {
  const boxed: any = Object(value);
  return boxed.valueOf();
}
console.log(unboxed(1), unboxed("s"), unboxed(true));

// Erased field round trips.
function erasedField(): unknown {
  const obj: any = {};
  obj.someProp = 1;
  return obj.someProp;
}
function erasedKeys(): string[] {
  const obj: any = {};
  obj.someProp = 1;
  return Object.keys(obj);
}
console.log(erasedField(), erasedKeys().join(","));

// Object.create allocates.
function shallowClone(obj: object): object {
  const prototype = Object.getPrototypeOf(obj);
  return Object.assign(Object.create(prototype), obj);
}
const originalRecord = { a: 1 };
const cloneRecord = shallowClone(originalRecord) as { a: number };
cloneRecord.a = 2;
console.log(originalRecord.a, cloneRecord.a);

// Calls and control flow.
function addTwo(a: number, b: number): number {
  return a + b;
}
console.log(addTwo(2, 3));
function maxOf(a: number, b: number): number {
  if (a > b) {
    return a;
  }
  return b;
}
console.log(maxOf(2, 3), maxOf(3, 2));
function label(status: "pending" | "approved" | "rejected"): string {
  switch (status) {
    case "pending":
      return "Waiting";
    case "approved":
      return "Approved";
    case "rejected":
      return "Rejected";
  }
}
console.log(label("approved"), label("pending"));
function pick(kind: string, n: number): number {
  let total = 0;
  switch (kind) {
    case "loop": {
      while (n > 0) {
        total += n;
        n--;
      }
      break;
    }
    case "double":
      return n * 2;
    default:
      total = n;
  }
  return total;
}
console.log(pick("loop", 3), pick("double", 3), pick("other", 3));
function trimEndChars(str: string, chars: string | string[]): string {
  let endIndex = str.length;
  switch (typeof chars) {
    case "string": {
      if (chars.length !== 1) {
        throw new Error("bad");
      }
      while (endIndex > 0 && str[endIndex - 1] === chars) {
        endIndex--;
      }
      break;
    }
    case "object": {
      while (endIndex > 0 && chars.includes(str[endIndex - 1])) {
        endIndex--;
      }
    }
  }
  return str.substring(0, endIndex);
}
console.log(trimEndChars("baaa", "a"), trimEndChars("bcd", ["d", "c"]));
try {
  trimEndChars("x", "ab");
} catch (error) {
  console.log((error as Error).message);
}
function pathExists(object: any, path: PropertyKey | PropertyKey[]): boolean {
  let resolvedPath;
  if (Array.isArray(path)) {
    resolvedPath = path;
  } else if (typeof path === "string") {
    resolvedPath = [path];
  } else {
    resolvedPath = [path];
  }
  let current = object;
  for (let i = 0; i < resolvedPath.length; i++) {
    const key = resolvedPath[i];
    if (current == null) {
      return false;
    }
    current = current[key];
  }
  return true;
}
console.log(pathExists({ a: 1 }, ["a"]), pathExists({ a: 1 }, ["a", "b", "c"]));
function failing(): void {
  throw "boom";
}
try {
  failing();
} catch (error) {
  console.log(error);
}

// Short-circuit initializers before a tail.
function tailAfterOr(a: unknown, b: unknown, tag: string, ks: number[]): boolean {
  switch (tag) {
    case "x": {
      const flag = isNull(a) || (isUndef(a) && isUndef(b));
      if (!flag) {
        return false;
      }
      for (const k of ks) {
        if (k === 0) {
          return false;
        }
      }
      return true;
    }
    default:
      return false;
  }
}
function isNull(x: unknown): boolean {
  return x === null;
}
function isUndef(x: unknown): boolean {
  return x === undefined;
}
console.log(tailAfterOr(null, 2, "x", [1]), tailAfterOr(1, 2, "x", [1]), tailAfterOr(undefined, undefined, "x", [0]));
function fieldIs(a: any, name: string, want: number): boolean {
  return (a as any)[name] === want;
}
function deepEq(a: any, b: any): boolean {
  const areEqual = fieldIs(a, "x", 1) || (fieldIs(a, "y", 2) && fieldIs(b, "y", 2));
  if (!areEqual) {
    return false;
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) {
    return false;
  }
  for (let i = 0; i < aKeys.length; i++) {
    const key = aKeys[i];
    if ((a as any)[key] !== (b as any)[key]) {
      return false;
    }
  }
  return true;
}
console.log(deepEq({ x: 1 }, { x: 1 }), deepEq({ y: 2 }, { y: 2 }), deepEq({ y: 2 }, { y: 3 }), deepEq({ x: 1 }, { x: 2 }));
function someOf(source: any, predicate: (v: any, k: any, s: any) => boolean): boolean {
  if (!Array.isArray(source)) {
    for (const key in source) {
      if (predicate((source as any)[key], key, source)) {
        return true;
      }
    }
    return false;
  }
  for (let i = 0; i < (source as any[]).length; i++) {
    if (predicate((source as any[])[i], i, source)) {
      return true;
    }
  }
  return false;
}
console.log(someOf([1, 2], (v: any) => v === 2), someOf({ a: 3 }, (v: any) => v === 3), someOf({}, () => true));

// Explicit `any` locals keep their dynamic shape.
function anyLocal(): number {
  let o: any = { a: true };
  o = { a: 1, b: 2, c: 3 };
  o.b = o;
  return o.b.c;
}
console.log(anyLocal());

// A module object constant read from a callback.
const htmlUnescapes: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
};
function unescapeHtml(str: string): string {
  return str.replace(/&(?:amp|lt);/g, (match) => htmlUnescapes[match] || "'");
}
const codes: Record<string, number> = { a: 1, b: 2 };
function codeValues(items: string[]): number[] {
  return items.map(() => codes["b"]);
}
console.log(unescapeHtml("a &amp; b &lt; c"), codeValues(["x"]).join(","));

// typeof over a union inside an arrow predicate.
function countMatching(values: Array<number | string>, pred: (value: number | string) => boolean): number {
  return values.filter((item) => pred(item)).length;
}
function runTypeof(values: Array<number | string>): number {
  const isString = (value: number | string) => typeof value === "string";
  return countMatching(values, isString);
}
console.log(runTypeof([1, "a", "b"]));

// Generators.
function* countTo(limit: number): Generator<number> {
  for (let i = 0; i < limit; i += 1) {
    yield i;
  }
}
function sumTo(): number {
  let sum = 0;
  for (const v of countTo(5)) {
    sum += v;
  }
  return sum;
}
console.log(sumTo());

// A callback flowing into an owned sink, and one that only throws.
function innerSink(cb?: (x: number) => number): number {
  return cb ? cb(1) : 0;
}
function outerSink(cb: (x: number) => number): number {
  return innerSink(cb);
}
console.log(outerSink((x) => x + 1), innerSink());
function makeThrower(): () => number {
  return () => {
    throw new Error("nope");
  };
}
try {
  makeThrower()();
} catch (error) {
  console.log((error as Error).message);
}

// Dates.
const dated = new Date(2014, 8, 2, 11, 55, 0);
console.log(dated.getFullYear(), dated.getMonth(), dated.getDate());
dated.setFullYear(2020, 0, 1);
dated.setMonth(1, 2);
dated.setDate(3);
console.log(dated.getFullYear(), dated.getMonth(), dated.getDate());
function isExists(year: number, month: number, day: number): boolean {
  const date = new Date(year, month, day);
  return date.getFullYear() === year && date.getMonth() === month && date.getDate() === day;
}
console.log(isExists(2020, 1, 29), isExists(2021, 1, 29));
function keepInvalid(): boolean {
  const date = new Date(NaN);
  date.setHours(0);
  return Number.isNaN(date.getTime());
}
console.log(keepInvalid());
function nextHour(): number {
  const date = new Date(2020, 0, 1, 23, 30, 0, 0);
  date.setHours(date.getHours() + 1);
  return date.getDate();
}
console.log(nextHour());

// A class extending Error, and a generic class field map.
class HttpError extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.name = "CustomError";
    this.code = code;
  }
}
const httpError = new HttpError("x", 400);
console.log(httpError.name, httpError.message, httpError.code);
class KeyedCache<T> {
  private data: Map<T, string> = new Map();
  size(): number {
    return this.data.size;
  }
  put(key: T, value: string): void {
    this.data.set(key, value);
  }
}
const keyed = new KeyedCache<string>();
keyed.put("a", "b");
console.log(keyed.size());

// console.log of a generic value.
function show<A>(value: A): void {
  console.log(value);
}
show(1);
show("s");

// A callable interface.
interface Callable {
  (x: number): number;
  tag(): string;
}
function useApply(c: Callable): number {
  return c.apply(undefined, [1]) as number;
}
const callableValue = Object.assign((x: number): number => x + 1, { tag: () => "t" }) as Callable;
console.log(useApply(callableValue), callableValue.tag());

// A callable with properties.
const fnValue = (value: number): number => value;
const assignedFn = Object.assign(fnValue, { lazy: fnValue });
console.log(assignedFn(4), assignedFn.lazy(5));
function wrapCached(call: (...args: string[]) => void): unknown {
  let cached: string | undefined;
  return Object.assign(
    (...args: string[]) => {
      call(...args);
      return cached;
    },
    { flush: () => cached },
  );
}
const wrappedCached = wrapCached(() => {}) as any;
console.log(wrappedCached("a"), wrappedCached.flush());

// Pure-data shapes.
function pairWithNull(promise: Promise<unknown>): [null, Promise<unknown>] {
  return [null, promise] as [null, Promise<unknown>];
}
console.log(pairWithNull(Promise.resolve(1))[0]);
function fallback(value: unknown): unknown {
  return value ?? "fallback";
}
console.log(fallback(null), fallback(1));
function addComputed(index: number, value: unknown): Record<string, unknown> {
  return { [index]: value };
}
console.log(Object.keys(addComputed(3, "v")).join(","));
function identityValue<T>(value: T): T {
  return value;
}
console.log(identityValue([1, 2]).length, identityValue({ a: "b" }).a);

// A customizer that falls through.
function customizerResult(customizer: (value: unknown) => unknown): unknown {
  const cloned = customizer(1);
  if (cloned !== undefined) {
    return cloned;
  }
  return "default";
}
console.log(customizerResult(() => {}));

// Promises and async functions, sequenced so the output order is the await order.
async function lift(value: number): Promise<number> {
  return value;
}
function makeValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    resolve(1);
  });
}
async function untypedPromise(): Promise<number> {
  return new Promise((resolve) => {
    resolve(2);
  });
}
function makeQueue(): Promise<number[]> {
  const processor = async (resolve: (value: number[]) => void) => {
    while (true) {
      return resolve([1, 2]);
    }
  };
  return new Promise(processor);
}
function delayedValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    setTimeout(() => resolve(7), 0);
  });
}
function delayVoid(): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, 1));
}
function defer(value: unknown): Promise<unknown> {
  return new Promise<unknown>((resolve) => {
    setTimeout(() => resolve(value), 0);
  });
}
async function makeOne(value: unknown): Promise<unknown> {
  return defer(value);
}
class Semaphore {
  private available: number = 1;
  async acquire(): Promise<number> {
    return this.available;
  }
}
class Latch {
  done: boolean = false;
  async wait(): Promise<boolean> {
    return this.done;
  }
}
class DeferredQueue {
  private deferredTasks: Array<() => void> = [];
  acquire(): Promise<void> {
    return new Promise<void>((resolve) => {
      this.deferredTasks.push(resolve);
    });
  }
  releaseAll(): void {
    for (const task of this.deferredTasks) {
      task();
    }
  }
}
async function loadInClosure(): Promise<number> {
  const inner = async () => await lift(3);
  return await inner();
}
async function asyncCapture(arr: number[]): Promise<number> {
  let running = 0;
  let maxRunning = 0;
  const fn = async (item: number): Promise<boolean> => {
    running = running + 1;
    if (running > maxRunning) {
      maxRunning = running;
    }
    await delayVoid();
    running = running - 1;
    return item % 2 === 0;
  };
  await Promise.all(arr.map(fn));
  return maxRunning;
}
async function throwingAwait(): Promise<string> {
  try {
    await lift(1);
    return "ok";
  } catch (e) {
    return "caught";
  }
}
async function hoistedThen(p: Promise<number>, cb: (v: number) => void): Promise<void> {
  await p.then(cb);
  cb(1);
}
async function zeroArityThen(p: Promise<number>): Promise<string> {
  let hits = 0;
  const cb = () => {
    hits = hits + 1;
  };
  await p.then(cb);
  return String(hits);
}
function limitAsync(
  fn: (...args: unknown[]) => Promise<unknown>,
  concurrency: number,
): (...args: unknown[]) => Promise<unknown> {
  return fn;
}
async function throwingAsyncCallback(): Promise<string> {
  const callback = async (item: unknown): Promise<unknown> => {
    if (item === 2) {
      throw new Error("fail");
    }
    return item;
  };
  const limited = limitAsync(callback, 2);
  const first = await limited(1);
  try {
    await limited(2);
    return "no throw";
  } catch (error) {
    return `${String(first)} ${(error as Error).message}`;
  }
}
async function main(): Promise<void> {
  console.log(await lift(5), await makeValue(), await untypedPromise());
  console.log((await makeQueue()).join(","), await delayedValue());
  await delayVoid();
  const [liftedA, liftedB] = await Promise.all([lift(1), lift(2)]);
  console.log(liftedA, liftedB);
  const madeOnes = await Promise.all([makeOne(1), makeOne(2)]);
  console.log(madeOnes[0], madeOnes[1]);
  console.log(await new Semaphore().acquire(), await new Latch().wait());
  const queue = new DeferredQueue();
  const pending = queue.acquire();
  queue.releaseAll();
  await pending;
  console.log("released");
  console.log(await loadInClosure(), await asyncCapture([1, 2]), await throwingAwait());
  const seen: number[] = [];
  await hoistedThen(Promise.resolve(4), (v) => {
    seen.push(v);
  });
  console.log(seen.join(","), await zeroArityThen(Promise.resolve(1)));
  console.log(await throwingAsyncCallback());
}
main();
