// Closures as values: capture, adapters between call shapes, and function identity.

// A callback that calls the binding its factory assigns.
type StrFn = (value: string) => void;
function wrap(callback: StrFn): StrFn {
  return callback;
}
function recurseThrough(): string[] {
  const seen: string[] = [];
  const recursive = wrap((value: string) => {
    seen.push(value);
    if (value.length < 3) {
      recursive(value + "a");
    }
  });
  recursive("a");
  return seen;
}
console.log(recurseThrough().join(","));

// An erased rest closure runs its body.
type RestVoid = (...args: unknown[]) => void;
const restLog: number[] = [];
function makeRestVoid(): RestVoid {
  return (...args: unknown[]) => {
    args.push("called");
    restLog.push(args.length);
  };
}
makeRestVoid()("x");
console.log(restLog.join(","));

// Equivalent erased rest shapes.
type AnyRest = (...args: unknown[]) => unknown;
type StringRest = (...args: unknown[]) => string;
function readRest(): StringRest {
  return (...args: unknown[]) => String(args.length);
}
const restCallback: AnyRest = readRest();
console.log(restCallback("x"));

// A callback held in an interface field.
interface Localize<T> {
  ordinalNumber: (value: number) => string;
  value?: T;
}
function readLocalize(localize: Localize<string>): string {
  return localize.ordinalNumber(1);
}
console.log(readLocalize({ ordinalNumber: (n) => `#${n}` }));

// A self-recursive closure.
function collectRows(rows: number[][]): number[] {
  const result: number[] = [];
  const recurse = (items: number[][], depth: number): void => {
    for (const item of items) {
      if (depth > 0) {
        recurse([item], depth - 1);
      } else {
        result.push(item[0]);
      }
    }
  };
  recurse(rows, 1);
  return result;
}
console.log(collectRows([[1], [2]]).join(","));

// A callback result flowing into an optional binding, and a copied struct of callbacks.
function maybeCall(callback: (value: number) => number): number | undefined {
  const value: number | undefined = callback(1);
  return value;
}
interface Callbacks {
  run: () => number;
}
function copyCallbacks(callbacks: Callbacks): Callbacks {
  return callbacks;
}
console.log(maybeCall((v) => v + 1), copyCallbacks({ run: () => 5 }).run());

// A call-bodied local arrow, and an index inside a sort-then-map chain.
function makeDataLast(fn: (value: number, extra: number) => number, extra: number): (value: number) => number {
  const dataLast = (data: number): number => fn(data, extra);
  return dataLast;
}
console.log(makeDataLast((a, b) => a - b, 1)(3));
function sampleSorted<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>([1, 0]);
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
console.log(sampleSorted(["a", "b"]).join(","));
const sortWith = <T>(data: readonly T[], compareFn: (left: T, right: T) => number): T[] => [...data].sort(compareFn);
console.log(sortWith([3, 1, 2], (a, b) => b - a).join(","));
function joinGeneric<T>(items: T[]): string {
  return items.join("-");
}
console.log(joinGeneric([1, 2]), joinGeneric(["x"]));

// Rest arguments packed for an ordinary closure call.
function makeIdentity(): (first: unknown, ...rest: unknown[]) => unknown {
  return (first: unknown) => first;
}
console.log(makeIdentity()("hello"));

// A non-escaping closure that captures a borrowed callback parameter.
function visitFirst<T>(
  data: readonly T[],
  callback: (value: T, index: number, data: readonly T[]) => boolean,
): number {
  return callback(data[0]!, 0, data) ? 1 : 0;
}
function indicesSeen(
  items: readonly unknown[],
  predicate: (item: unknown, index: number) => boolean,
): number[] {
  const indices: number[] = [];
  visitFirst(items, (pivot, index) => {
    indices.push(index);
    return predicate(pivot, index);
  });
  return indices;
}
console.log(indicesSeen(["x"], () => true).join(","));

// Returned function values.
function makeMapper(): (value: number) => number {
  const mapper = (value: number) => value + 1;
  return mapper;
}
console.log(makeMapper()(1));

// Adapters between call shapes.
function adaptObjectReturn(
  callback: (value: unknown) => { next: unknown },
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
console.log((adaptObjectReturn((v) => ({ next: v }))(1, 0, []) as { next: unknown }).next);
function adaptArguments(
  callback: (values: unknown[], index?: number) => unknown,
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback as unknown as (value: unknown, index: number, data: unknown[]) => unknown;
}
console.log(adaptArguments((values, index) => `${values.length}:${index}`)([1, 2], 3, []));
function outerMake(make: () => (value: unknown) => unknown): unknown {
  return [make];
}
const made = outerMake(() => (value) => value) as Array<() => (value: unknown) => unknown>;
console.log(made[0]()(4));
function expose(callback: (value: unknown) => unknown): unknown {
  return { callback };
}
console.log((expose((v) => v) as { callback: (value: unknown) => unknown }).callback(9));

// Optional chains over callbacks.
interface OptionalCallback {
  cb?: (value: number) => number;
}
function readCallback(options?: OptionalCallback): ((value: number) => number) | undefined {
  return options?.cb;
}
function applyOptional(customizer?: (value: number) => number): number | undefined {
  return customizer?.(1);
}
console.log(readCallback() === undefined, readCallback({ cb: (v) => v })?.(2), applyOptional(), applyOptional((v) => v + 1));

// typeof inside a callback over erased values.
function mapTypes(values: unknown[]): string[] {
  return values.map((item) => typeof item);
}
console.log(mapTypes([1, "s", true, null, undefined, {}, [], () => 1, Symbol("x")]).join(","));

// Rest parameters on a function expression.
function nthArgValue(n: number): (...args: number[]) => number {
  return function (...args: number[]): number {
    return args[n];
  };
}
console.log(nthArgValue(1)(4, 5, 6));

// Callback conditions and bodies that leave the compact form.
function pickTruthy(values: number[]): number[] {
  return values.map((value, index) => (index ? value * 2 : value));
}
function orderRows(items: number[][]): number[][] {
  return items.slice().sort((a, b) => {
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        return a[i] - b[i];
      }
    }
    return 0;
  });
}
function mapAny(values: string[], fn: any): string[] {
  return values.map(fn);
}
function reassignParam(values: number[]): number[] {
  return values.map((value) => {
    if (value < 0) {
      value = 0;
    }
    return value + 1;
  });
}
console.log(pickTruthy([1, 2]).join(","), orderRows([[2], [1]]).map((row) => row.join(",")).join("|"), mapAny(["a"], (v: string) => v + "!").join(","), reassignParam([-1, 1]).join(","));
function forEachNoParam(values: number[], onTick: () => void): void {
  values.forEach(() => onTick());
}
let ticks = 0;
forEachNoParam([1, 2], () => {
  ticks = ticks + 1;
});
console.log(ticks);
function functionExpressionCallback(values: string[]): Array<string | undefined> {
  return values.map(function (value) {
    try {
      return value;
    } catch (error) {
      return undefined;
    }
  });
}
console.log(functionExpressionCallback(["z"]).join(","));

// Reduce with a named callback whose accumulator is wider than the seed.
function stepWide(acc: string | number, value: number): number {
  return value;
}
function runWide(values: number[]): string | number {
  return values.reduce(stepWide, "seed");
}
function stepUnknown(acc: number, value: number): unknown {
  return acc + value;
}
function runUnknown(values: number[]): unknown {
  return values.reduce(stepUnknown as (acc: unknown, value: number) => unknown, 0);
}
console.log(String(runWide([1, 2])), String(runWide([])), String(runUnknown([2, 3])));

// A member store inside a map callback.
function tagRows(rows: string[][], suffix: string): string[][] {
  return rows.map((row) => {
    row[0] += suffix;
    return row;
  });
}
const taggedSource = [["a"]];
console.log(tagRows(taggedSource, "!")[0][0], taggedSource[0][0]);

// Callback adapters reached through helpers.
function uniqBy<T>(arr: T[], mapper: (item: T, index: number, arr: T[]) => unknown): T[] {
  const seen = new Set<unknown>();
  return arr.filter((item, index) => {
    const key = mapper(item, index, arr);
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}
function unionBy<T>(arr1: T[], arr2: T[], mapper: (item: T) => unknown): T[] {
  return uniqBy([...arr1, ...arr2], mapper);
}
console.log(unionBy([1, 2], [2, 3], (x) => x).join(","));
function unzipWith(arrays: number[][], iteratee: (...values: unknown[]) => unknown): unknown[] {
  const result: unknown[] = [];
  for (const group of arrays) {
    result.push(iteratee(group[0], group[1], group[2]));
  }
  return result;
}
console.log(unzipWith([[1, 2, 3]], (...values) => (values[0] as number) + (values[1] as number) + (values[2] as number)).join(","));
function invokeWith(func: (...args: unknown[]) => unknown, args: unknown[]): unknown {
  return func(...args);
}
console.log(invokeWith((...a) => a.length, [1, 2]));
function afterN(n: number, func: () => void): () => void {
  return () => {
    n = n - 1;
    if (n <= 0) {
      func();
    }
  };
}
let afterFired = 0;
const fireAfter = afterN(2, () => {
  afterFired = afterFired + 1;
});
fireAfter();
fireAfter();
console.log(afterFired);

// Erased optional returns.
function runOptionalReturn(cb: (a: unknown, b: unknown) => boolean | void, x: unknown, y: unknown): boolean {
  const r = cb(x, y);
  if (r !== undefined) {
    return r;
  }
  return false;
}
console.log(runOptionalReturn(() => true, 1, 2), runOptionalReturn(() => undefined, 1, 2), runOptionalReturn(() => {}, 1, 2));

// apply, call and bind.
function forwardApply(func: (...args: any[]) => any, args: any[]): any {
  return func.apply(null, args);
}
console.log(forwardApply((a: number, b: number) => a + b, [1, 2]));
function invokeApply(func: any, args: unknown[]): unknown {
  return func.apply(undefined, args);
}
function invokeCall(func: any, first: unknown): unknown {
  return func.call(undefined, first);
}
console.log(invokeApply((a: number, b: number) => a * b, [3, 4]), invokeCall((a: number) => a + 1, 1));

// Function values held through an adapter.
function adaptReturnedFunction(
  callback: (key: string) => (record: Record<string, number>) => number,
): (key: string) => unknown {
  return callback;
}
const adaptedGetter = adaptReturnedFunction((key) => (record) => record[key])("a") as (record: Record<string, number>) => number;
console.log(adaptedGetter({ a: 5 }));

// Shadowed callback parameters.
function shadowedRead(): number {
  const objectize =
    (callback: (value: { num: number }) => number) =>
    (num: number) => callback({ num });
  return objectize((value) => value.num)(1);
}
function shadowedMutate(): number {
  let seen = 0;
  const append =
    (items: number[]) =>
    (callback: (value: number[]) => void) => {
      items.push(1);
      callback(items);
    };
  append([])((inner) => {
    seen = inner.length;
  });
  return seen;
}
console.log(shadowedRead(), shadowedMutate());

// Closures with guards and loops over captured state.
function once<T>(fn: () => T): () => T {
  let called = false;
  let ret: T;
  return () => {
    if (!called) {
      ret = fn();
      called = true;
    }
    return ret;
  };
}
let onceRuns = 0;
const onceValue = once(() => {
  onceRuns = onceRuns + 1;
  return onceRuns;
});
console.log(onceValue(), onceValue(), onceRuns);
function closureLoopBranch(items: number[]): number[] {
  const out: number[] = [];
  const walk = (values: number[]) => {
    for (let i = 0; i < values.length; i++) {
      if (values[i] > 0) {
        out.push(values[i]);
      } else {
        out.push(0);
      }
    }
  };
  walk(items);
  return out;
}
function firstBig(values: number[], limit: number): number {
  const pick = (input: number[]) => {
    for (let i = 0; i < input.length; i++) {
      if (input[i] > limit && input[i] < 100) {
        return input[i];
      }
    }
    return -1;
  };
  return pick(values);
}
console.log(closureLoopBranch([1, -1]).join(","), firstBig([1, 5, 200], 2), firstBig([1], 2));

// Named function expressions.
function applyNamed(cb: (n: number) => number, v: number): number {
  return cb(v);
}
function countdown(start: number): number {
  return applyNamed(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}
function step(label: string): string {
  return `${label}!`;
}
console.log(countdown(3), step("go"));

// `arguments` is the actual call.
function probeArguments(_a: unknown, _b: unknown, _c: unknown): number {
  return arguments.length;
}
console.log((probeArguments as (...args: unknown[]) => number)("a", "b"));
function nestedArguments(a: number, b: number): number {
  function inner(_x: unknown): number {
    return arguments.length;
  }
  return inner(a) + a + b;
}
console.log(nestedArguments(1, 2));

// `.length` of callables.
function target(_a: unknown, _b: unknown): unknown {
  return _a;
}
function erasedArity(): number {
  const erased: unknown = target;
  return (erased as any).length;
}
function declaredArity(cb: (a: number, b: number) => void): number {
  return cb.length;
}
console.log(erasedArity(), declaredArity((a, b) => {}));

// Static properties on a function.
function partialFn(func: (...args: any[]) => any, ...args: any[]): (...rest: any[]) => any {
  return (...rest: any[]) => func(...args, ...rest);
}
partialFn.placeholder = Symbol("partial.placeholder");
function readMember(): unknown {
  return partialFn.placeholder;
}
function readDestructured(): unknown {
  const { placeholder } = partialFn;
  return placeholder;
}
console.log(readMember() === readDestructured(), String(readMember()), partialFn((a: number, b: number) => a + b, 1)(2));

// A try around a callback call, and erased handles bound to locals.
function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return "caught";
  }
}
console.log(guard(() => { throw new Error("x"); }, 1), guard((x) => String(x), 2));
function viaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3, 4);
}
function guardedViaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  try {
    return g(3, 4);
  } catch {
    return "caught";
  }
}
console.log(viaLocal((...a) => a.length), guardedViaLocal((...a) => a.length), guardedViaLocal(() => { throw 1; }));

// Adapters over `never` and `void` sources still call the source.
function attemptCall(func: () => unknown): unknown[] {
  try {
    return [null, func()];
  } catch (error) {
    return ["caught", null];
  }
}
function useAttempt(): unknown[] {
  return attemptCall(() => {
    throw new Error("boom");
  });
}
console.log(String(useAttempt()[0]));
let voidCalls = 0;
function matchWith(
  target: unknown,
  source: unknown,
  customizer: (a: unknown, b: unknown) => boolean | undefined,
): boolean {
  return customizer(target, source) ?? false;
}
function runMatch(a: unknown, b: unknown): boolean {
  return matchWith(a, b, () => {
    voidCalls = voidCalls + 1;
    return undefined;
  });
}
console.log(runMatch(1, 2), voidCalls);

// A recursive captured callee.
function flattenAll(values: unknown[], depth: number): unknown[] {
  const walk = (items: unknown[], level: number): unknown[] => {
    const out: unknown[] = [];
    for (const item of items) {
      if (Array.isArray(item) && level < depth) {
        for (const nested of walk(item as unknown[], level + 1)) {
          out.push(nested);
        }
      } else {
        out.push(item);
      }
    }
    return out;
  };
  return walk(values, 0);
}
console.log(flattenAll([1, [2, [3]]], 2).length, flattenAll([1, [2, [3]]], 1).length);
