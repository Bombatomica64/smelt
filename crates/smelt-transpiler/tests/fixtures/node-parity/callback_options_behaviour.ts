// Callbacks, optional parameters and the builtin calls reached from them.

// A tag check on an ambient global the profile models as absent.
declare let window: { document: unknown } | undefined;
function isBrowser(): boolean {
  return typeof window !== "undefined" && window?.document != null;
}
console.log("browser", isBrowser());

// Console rendering of numbers and lists.
const answer = 42;
console.log(answer);
const shown: number[] = [1, 2];
console.log(shown);

// String and list lengths.
const threeValues: number[] = [1, 2, 3];
console.log(threeValues.length, "smelt".length);

// First-class closure values.
const offset = 2;
const addOffset = (value: number): number => value + offset;
function applyTo(value: number, fn: (value: number) => number): number {
  return fn(value);
}
function makeAdder(base: number): (value: number) => number {
  const add = (value: number): number => value + base;
  return add;
}
const adder = makeAdder(5);
console.log(addOffset(3), applyTo(4, addOffset), adder(6));

// Outer bindings a local closure mutates stay shared.
function collectFlushes(): string[] {
  const values: string[] = [];
  let current = "x";
  const flush = (): void => {
    values.push(current);
    current = "";
  };
  flush();
  current = "y";
  flush();
  return values;
}
console.log(collectFlushes().join("|"));

// A sibling closure re-entering a captured cell mid-expression.
function siblingCall(): number {
  let count = 0;
  function bump(): number {
    count += 1;
    return count;
  }
  function accumulate(): void {
    count = count + bump();
  }
  accumulate();
  return count;
}
console.log("sibling", siblingCall());

// A string literal spelling a shared capture's name is data.
function runBump(fn: (x: number) => string): string {
  return fn(1);
}
function literalNamingCapture(): string {
  let current = 0;
  const bump = (x: number): string => {
    current = current + x;
    return "current went up";
  };
  const text = runBump(bump);
  return `${text}:${current}`;
}
console.log(literalNamingCapture());

// Indexed reads and writes of a shared-capture array.
function indexRead(): number {
  let order: number[] = [];
  function record(): void {
    order.push(1);
  }
  record();
  return order[0];
}
function indexWrite(k: number, v: number): number[] {
  const order: number[] = [0, 0, 0];
  function put(i: number, value: number): void {
    order[i] = value;
  }
  put(k, v);
  return order;
}
console.log(indexRead(), indexWrite(1, 9).join(","), indexWrite(5, 2).length);

// Callback argument positions.
const anyPass = (data: unknown, fns: Array<(value: unknown) => boolean>): boolean =>
  fns.some((fn) => fn(data));
console.log(anyPass(1, [() => false, (v) => v === 1]), anyPass(1, [() => false]));
function offsets(values: number[], factor: number): number[] {
  return values.map((value, index) => value * factor + index);
}
function positive(values: number[]): boolean {
  return values.some((value, index) => value + index > 2);
}
function view(values: number[]): number[] {
  return values.map((value, index, array) => value + array[index]);
}
function totalIndexed(values: number[]): number {
  return values.reduce((acc, value, index, array) => acc + value + array[index], 0);
}
function rebindOwnArray(values: number[]): number[] {
  return values.map((value, index, array) => {
    array = [value * 2];
    return array[0];
  });
}
console.log(offsets([1, 2], 3).join(","), positive([1, 1]), positive([0, 0]));
console.log(view([1, 2]).join(","), totalIndexed([1, 2]), rebindOwnArray([3]).join(","));
function nestedFilters(groups: number[][], limit: number): boolean[] {
  return groups.map((group) => group.filter((value) => value > limit).some((value) => value > 0));
}
console.log(nestedFilters([[1, 5], [0]], 2).join(","));
type RestCallback = (...args: unknown[]) => unknown;
function invokeAll(callbacks: RestCallback[], args: unknown[]): unknown[] {
  return callbacks.map((callback) => callback(...args));
}
console.log(invokeAll([(...a) => a.length, (...a) => a[0]], [7, 8]).join(","));
function lazyCalls(functions: Array<(value: unknown) => unknown>): Array<unknown | null> {
  return functions.map((fn) => (false ? fn(null) : null));
}
console.log(lazyCalls([(v) => v]).join(","), lazyCalls([(v) => v]).length);

// Optional concatenation.
function labelled(prefix: string, value: string | undefined): string {
  return prefix + value;
}
console.log(labelled("p", undefined), labelled("p", "q"));

// Erased receivers inside callbacks.
const unknownStrings: unknown[] = ["abc"];
const tails = unknownStrings.map((value) => (value as any).slice(1));
console.log(tails.join(","));

// Default, destructured and rest parameters.
const bumpDefault = <T extends number>(value: number = 1): number => value + 1;
console.log(bumpDefault(), bumpDefault(4));
const tuplePairs: [number, number][] = [[1, 2], [3, 4]];
console.log(tuplePairs.map(([left, right]) => left + right).join(","));
const rows: Record<string, number>[] = [{ value: 3 }];
console.log(rows.map(({ value }) => value * 2).join(","));
const restSum = (...values: number[]): number => values[0] + values[1];
function restSumFn(...values: number[]): number {
  return values[0] + values[1];
}
console.log(restSum(2, 3, 4), restSumFn(2, 3, 4));
function restCollect(context: unknown, ...rest: unknown[]): unknown[] {
  return rest;
}
function restForward<Values extends unknown[]>(first: unknown, values: Values): unknown[] {
  return restCollect(undefined, first, ...values);
}
console.log(restForward("a", [1, 2]).join(","));

// Spreading an optional options object.
interface WeekOptions {
  weekStartsOn?: number;
}
function innerWeek(date: number, options?: WeekOptions): number {
  return date + (options?.weekStartsOn ?? 100);
}
function outerWeek(date: number, options?: WeekOptions): number {
  return innerWeek(date, { ...options, weekStartsOn: 1 });
}
console.log(outerWeek(1), outerWeek(1, { weekStartsOn: 9 }));

// typeof on an optional number.
function isNumberValue(value?: number): boolean {
  return typeof value === "number";
}
function isUndefinedValue(value?: number): boolean {
  return typeof value === "undefined";
}
console.log(isNumberValue(1), isNumberValue(), isUndefinedValue(1), isUndefinedValue());

// Math.
const negFive = -5;
console.log(Math.abs(negFive), Math.floor(5.5), Math.ceil(5.5), Math.round(5.5), Math.round(-1.5), Math.trunc(5.5));
const whole = Math.trunc(1234 / 1000);
console.log(whole.toString());
console.log(Math.max(1, 2, 3), Math.min(1, 2, -1));
const four = 4;
console.log(Math.sqrt(four), Math.cbrt(8), Math.pow(four, 2), Math.sign(-four), Math.sign(0));
console.log(Math.sin(0), Math.cos(0), Math.tan(0), Math.asin(0), Math.acos(1), Math.atan(0), Math.atan2(0, 2));
console.log(Math.log(1), Math.log10(100), Math.log2(8), Math.exp(0), Math.hypot(3, 4));
const sample = Math.random();
console.log(sample >= 0 && sample < 1);

// The array callback methods.
const numbers: number[] = [1, 2, 3];
console.log(numbers.map((value) => value + 1).join(","), numbers.filter((value) => value > 1).join(","));
console.log(numbers.find((value) => value > 1), numbers.findIndex((value) => value > 1), numbers.findIndex((value) => value > 9));
console.log(numbers.some((value) => value > 1), numbers.every((value) => value > 0));
numbers.forEach((value) => console.log("each", value + 1));
console.log(numbers.reduce((acc, value) => acc + value, 0), numbers.map((value, index) => value + index).join(","));
const factor = 2;
console.log(numbers.map((value, index) => value * factor + index).join(","));
const scale = (value: number): number => value + factor;
console.log(numbers.map(scale).join(","));
function construct(context: unknown, value: unknown): unknown {
  return value;
}
const normalize = construct.bind(null, undefined);
console.log(numbers.map(normalize).join(","));
let mutableTotal = 0;
numbers.forEach((value) => (mutableTotal += value));
console.log(mutableTotal, numbers.reduce((acc, value, index) => acc + value + index));
try {
  ([] as number[]).reduce((acc, value) => acc + value);
} catch (error) {
  console.log((error as Error).message);
}

// A captured multi-argument callback, and a free function of the same name.
function zipFour(
  first: unknown[],
  second: unknown[],
  fn: (first: unknown, second: unknown, index: number, data: unknown[]) => unknown,
): unknown[] {
  return first.map((item, index) => fn(item, second[index], index, first));
}
console.log(zipFour([1], [2], (a, b, i, d) => `${a}${b}${i}${d.length}`).join(","));

// String indexing and iteration.
const word = "abc";
console.log(word[0], word.at(-1), word.at(5) === undefined);
let joined = "";
for (let ch of word) {
  joined = joined + ch;
}
console.log(joined);

// Strings in callbacks.
function regexUpper(units: string[]): string[] {
  return units.map((unit) => `x${unit.replace(/(^.)/, (m) => m.toUpperCase())}` as string);
}
function recordRead(record: Record<string, number>, keys: string[]): number[] {
  return keys.map((key) => record[key]);
}
console.log(regexUpper(["ab"]).join(","), recordRead({ a: 1 }, ["a"]).join(","));
console.log([" a ", " "].filter((value) => !!value.trim()).length);
function cleanAny(value: any): any[] {
  return [value].filter((item) => !!item.trim());
}
console.log(cleanAny(" a ").length, cleanAny("  ").length);

// String methods.
const smelt = "Smelt";
console.log(smelt.toLowerCase(), smelt.toUpperCase());
const padded = " Smelt ";
console.log(`[${padded.trim()}][${padded.trimStart()}][${padded.trimEnd()}]`);
console.log(smelt.startsWith("Sm"), smelt.endsWith("lt"), smelt.startsWith("x"));
console.log(smelt.indexOf("m"), smelt.lastIndexOf("t"), smelt.lastIndexOf("t", 2), smelt.indexOf("z"));
interface SeparatorOptions {
  separator?: string | RegExp;
}
function lastSeparator(data: string, options: SeparatorOptions = {}): number {
  const { separator } = options;
  if (typeof separator === "string") {
    return data.lastIndexOf(separator);
  }
  return -1;
}
console.log(lastSeparator("a,b", { separator: "," }), lastSeparator("a,b"));
console.log("hello hello".replace("hello", "hi"), "a.b.c".replaceAll(".", "-"), "ha".repeat(3));
console.log("7".padStart(3, "0"), `[${"7".padEnd(3)}]`, "abc".padStart(2), "1".padStart(5, "ab"));
console.log(smelt.charAt(1), smelt.charCodeAt(2), `[${smelt.charAt(-1)}]`, Number.isNaN(smelt.charCodeAt(-1)));

// Primitive conversions.
console.log(String(42), Number("42"), Number(""), Boolean(""), Number("abc"), Number(" 12 "));

// A captured class method dispatched from a callback.
class Counter {
  base: number;
  constructor(b: number) {
    this.base = b;
  }
  scaled(x: number): number {
    const multiplier = this.base;
    return x * multiplier;
  }
}
function scaleAll(xs: number[]): number[] {
  const counter = new Counter(2);
  return xs.map((x) => counter.scaled(x));
}
console.log(scaleAll([1, 2]).join(","));

// Array.from over a nullable list.
function toList(arr: unknown[] | null | undefined): unknown[] {
  return Array.from(arr as unknown[]);
}
console.log(toList([1, 2]).length);

// A predicate that throws.
function firstBad(values: unknown[]): number {
  return values.findIndex((value) => {
    if (value) {
      throw new Error("boom");
    }
    return false;
  });
}
console.log(firstBad([0, 0]));
try {
  firstBad([0, 1]);
} catch (error) {
  console.log((error as Error).message);
}

// Rebinding a parameter does not reach the caller.
function rebindList(items: unknown[]): unknown[] {
  items = [items[0]];
  return items;
}
const original = [1, 2];
console.log(rebindList(original).length, original.length);
function rebindCallback(cb: (value: unknown) => unknown): unknown {
  cb = (value) => value;
  return cb(1);
}
console.log(rebindCallback(() => "ignored"));

// Returning from a loop inside a closure.
function firstMatch(pairs: Array<[number, string]>): (val: number) => string | undefined {
  const length = pairs.length;
  return function (val: number): string | undefined {
    for (let i = 0; i < length; i++) {
      const pair = pairs[i];
      if (pair[0] === val) {
        return pair[1];
      }
    }
    return undefined;
  };
}
const matcher = firstMatch([[1, "one"]]);
console.log(matcher(1), matcher(2));

// Truthiness of optional unions.
function pickFlag(opts?: { separator?: string | number }): string {
  const sep = opts?.separator;
  if (sep) {
    return "has";
  }
  return "none";
}
console.log(pickFlag(), pickFlag({ separator: 0 }), pickFlag({ separator: "" }), pickFlag({ separator: 1 }), pickFlag({ separator: "," }));

// String() over an optional value.
function firstLower(words: string[]): string {
  const first: string | undefined = words[0];
  return String(first).toLowerCase();
}
console.log(firstLower(["ABC"]), firstLower([]));

// A closure nested inside a shared-capture closure.
function makeTimers(onEnd: () => void): { schedule: () => void; cancel: () => void; read: () => number | null } {
  let timeoutId: number | null = null;
  const schedule = () => {
    const cb = () => {
      timeoutId = null;
      onEnd();
    };
    timeoutId = 1;
    cb();
    timeoutId = 2;
  };
  const cancel = () => {
    timeoutId = null;
  };
  return { schedule, cancel, read: () => timeoutId };
}
let timerEnds = 0;
const timers = makeTimers(() => {
  timerEnds = timerEnds + 1;
});
timers.schedule();
console.log(timerEnds, timers.read());
timers.cancel();
console.log(timers.read() ?? "cleared");

// `.length` on an optional union.
function measure(str: string, chars?: string | string[]): number {
  if (chars === undefined) {
    return str.length;
  }
  switch (typeof chars) {
    case "string": {
      return chars.length;
    }
    default: {
      return 0;
    }
  }
}
console.log(measure("abc"), measure("abc", "xy"), measure("abc", ["x"]));

// Numeric reads that need a cast.
function lastIndexRecord(): Record<string, unknown> {
  const re = /a/g;
  const out: Record<string, unknown> = {};
  out["idx"] = re.lastIndex;
  return out;
}
function filledZero(): number[] {
  const a: number[] = [1, 2, 3];
  a.fill(0);
  return a;
}
console.log(lastIndexRecord()["idx"], filledZero().join(","));

// A switch without a default falls through to its tail; with one it does not.
function afterSwitch(tag: string, a: number): number {
  switch (tag) {
    case "call": {
      return helperAdd(a);
    }
    case "lit": {
      return 7;
    }
  }

  let total = a;
  for (let i = 0; i < 3; i++) {
    total = total + i;
  }
  return total + 1000;
}
function helperAdd(x: number): number {
  return x + 1;
}
function classify(tag: string): number {
  switch (tag) {
    case "a": {
      return 1;
    }
    case "b": {
      return 2;
    }
    default: {
      return -1;
    }
  }
}
console.log(afterSwitch("call", 1), afterSwitch("lit", 1), afterSwitch("other", 1), classify("a"), classify("b"), classify("c"));

// `+` with a union operand.
type UnionBuffer = (string | number)[];
function appendTo(buffer: UnionBuffer, extra: string): void {
  buffer[0] += extra;
}
function joinFirst(buffer: UnionBuffer, extra: string): string {
  return buffer[0] + extra;
}
const unionBuffer: UnionBuffer = [1];
appendTo(unionBuffer, "x");
console.log(String(unionBuffer[0]), joinFirst([2], "y"));

// Erasing a union local does not consume it.
type Item = string | number;
function inspectItem(items: Item[]): boolean {
  const first = items[0];
  const isText = typeof first === "string";
  return isText && String(first).length > 0;
}
console.log(inspectItem(["a"]), inspectItem([1]), inspectItem([""]));
