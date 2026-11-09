// Array method behaviour: callback spellings, search, slice, mutation, copies.

function square(value: number): number {
  return value * value;
}
function stubTrue(): boolean {
  return true;
}
function stubFalse(): boolean {
  return false;
}
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0) + (index ?? 0) + (list ? list.length : 0);
}

function callbacks(values: number[]): void {
  console.log(values.map(function (value) { return value + 1; }).join(","));
  console.log(values.map(square).join(","));
  const transform = (value: number): number => value * 3;
  console.log(values.map(transform).join(","));
  console.log(values.map(square as (value: number) => number).join(","));
  console.log(values.map(stubTrue).join(","));
  console.log(values.filter(stubFalse).length, values.filter(stubTrue).length);
  console.log(values.map(withTail).join(","));
}

function opaque(values: unknown[], matchFunc: (value: unknown) => unknown): boolean {
  return values.some(matchFunc);
}

function identity(value: unknown): unknown {
  return value;
}
function tagged(value: unknown): unknown {
  return "#" + String(value);
}
function selected(values: unknown[], flag: boolean): unknown[] {
  return values.map(flag ? tagged : identity);
}

function makeCapped(): (...args: unknown[]) => unknown {
  return (...args: unknown[]) => args[0];
}

function viaForEach(values: number[], callback: (value: number, index: number, data: number[]) => void): void {
  values.forEach(callback);
}
function viaFlatMap(values: number[], callback: (value: number, index: number, data: number[]) => number[]): number[] {
  return values.flatMap(callback);
}

function throwing(values: string[]): string {
  try {
    return values
      .map((value) => {
        if (value === "x") {
          throw new Error("hit " + value);
        }
        return value;
      })
      .join("");
  } catch (error) {
    return (error as Error).message;
  }
}

function inCallbacks(): void {
  const data = [10, 20, 30];
  console.log([0, 2, -1, 5].map((i) => data.at(i)));
  const rows: string[][] = [["a", "b"], ["c", "d"]];
  console.log(rows.map((parts) => parts.join("-")).join("|"));
  const numeric: number[][] = [[1, 2], [3, 4]];
  console.log(numeric.map((parts) => parts.join()).join("|"));
}

function everyEntry<K, V>(map: Map<K, V>, doesMatch: (value: V, key: K, map: Map<K, V>) => boolean): boolean {
  for (const [key, value] of map) {
    if (!doesMatch(value, key, map)) {
      return false;
    }
  }
  return true;
}

function sums(values: number[]): void {
  const mapped = values.map((value) => value);
  console.log(mapped.reduce((acc, value, _index, array) => acc + value + array.length, 0));
  const reversed = values.slice().reverse();
  console.log(reversed.reduce((acc, value, _index, array) => acc + value + array.length));
}

function concats(a: number[], b: number[], c: number[]): void {
  console.log(a.concat(b, c).join(","));
  console.log(a.concat(7, 8).join(","));
  const copy = a.concat();
  copy.push(99);
  console.log(a.length, copy.length);
}

function pushes(): void {
  const a: number[] = [];
  console.log(a.push(1, 2, 3));
  a.push(...[4, 5], 9);
  console.log(a.join(","));
  const self: unknown[] = [];
  console.log(self.push(self), self.length, self[0] === self);
  const pairs: Array<[number, string]> = [];
  pairs.push([1, "one"]);
  pairs.push([2, "two"]);
  console.log(pairs.map(([n, s]) => s + n).join(","));
}

function preallocated(n?: number): number {
  return new Array(n).length;
}
function fromLength<T>(keys: T[]): number {
  return new Array(keys.length).length;
}

function filling<T>(a: T[], v: T): T[] {
  return a.fill(v);
}

function ranges(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}

function typeofs(x: unknown): string[] {
  return [typeof x, "tail"];
}

function adjust(value: number): number {
  const [magnitude, exponent = 0] = value.toString().split("e");
  return Number(`${magnitude}e${Number(exponent)}`);
}

function searches(): void {
  const values = [1, 2, NaN, 3, 2];
  console.log(values.indexOf(2), values.lastIndexOf(2), values.indexOf(NaN));
  console.log(values.indexOf(2, 2), values.indexOf(2, -1), values.indexOf(2, -100), values.indexOf(2, 99));
  console.log(values.lastIndexOf(2, 3), values.lastIndexOf(2, -2), values.lastIndexOf(2, -100));
}

function slices(): void {
  const values = [1, 2, 3, 4];
  console.log(values.slice().join(), values.slice(1).join(), values.slice(1, 3).join(), values.slice(-2).join());
  const word = "smelting";
  console.log(word.slice(), word.slice(1), word.slice(1, 4), word.slice(-3));
  const part = (start?: number, end?: number): string => word.slice(start, end);
  console.log(part(), part(2), part(undefined, 3));
  console.log("a,b,c".split(","), "a,b,c".split(",", 2), "a,b,c".split(",", 0));
  const limit = (text: string, n: number | undefined): string[] => text.split(",", n);
  console.log(limit("a,b", undefined), limit("a,b", 1));
  const patterns = { separator: /[T ]/i };
  console.log("2020t10 x".split(patterns.separator));
}

function mutators(): void {
  const values = [2, 3];
  console.log(values.unshift(), values.unshift(1), values.unshift(-1, 0), values.join());
  const r = [1, 2, 3];
  r.reverse();
  const same = r.reverse();
  same.push(4);
  console.log(r.join(), same === r);
  const s = [10, 2, 1];
  s.sort();
  console.log(s.join());
  const s2 = s.sort((left, right) => left - right);
  s2.push(0);
  console.log(s.join(), s2 === s);
}

function modern(): void {
  const values = [1, 2, 3, 4];
  console.log(values.splice(1, 2, 9), values.join());
  console.log(values.toSpliced(1, 1, 8).join(), values.toSpliced(1, 1, ...[10, 11]).join(), values.join());
  const longer = [1, 2, 3, 4];
  console.log(longer.fill(0, 1, 3).join(), longer.copyWithin(0, 1, 3).join(), longer.with(1, 7).join(), longer.join());
  const nested: number[][] = [[1], [2, 3]];
  console.log(nested.flat().join());
  const tupled: [word: string][] = [["one"], ["two"]];
  console.log(tupled.flat().join());
  const erased: unknown[] = [[1], [[2]], 3];
  console.log(erased.flat(2).length, erased.flat().length);
  console.log(longer.flatMap((value, index) => [value + index]).join());
  console.log(longer.toSorted((l, r) => r - l).join(), longer.toReversed().join(), longer.join());
  console.log(longer.findLast((v, i) => v > i), longer.findLastIndex((v, i) => v > i));
  console.log([...longer.keys()].join(), [...longer.values()].join(), [...longer.entries()].map(([i, v]) => i + ":" + v).join("|"));
}

function trimLast(items: number[]): number[] {
  [0].forEach((_, i) => {
    items.splice(i, 1);
  });
  return items;
}

function pickByIndex(values: number[]): number[] {
  const seen = new Set<number>([1, 2]);
  return values.filter((_, i) => seen.has(i));
}

function holes(): unknown[] {
  return [1, , 2];
}

function firstEntry(values: string[]): string | undefined {
  const iterator = values.entries();
  const result = iterator.next();
  if ("done" in result && result.done) {
    return undefined;
  }
  return result.value[1];
}

function swap(data: unknown[], i: number, j: number): void {
  [data[i], data[j]] = [data[j], data[i]];
}

function tuples(): void {
  const pair: [string, number] = ["a", 1];
  console.log(pair.length);
  const partitioned: [number[], number[]] = [[], []];
  partitioned[0].push(1);
  partitioned[1].push(2, 3);
  console.log(partitioned[0].length, partitioned[1].length);
  const asTuple = [1, null, 3].filter((x) => x != null) as [unknown];
  console.log(asTuple.length);
}

function pairOf(values: number[]): [number[], number[]] {
  return [values, values];
}
function widened(values: number[]): [unknown[], unknown[]] {
  return pairOf(values);
}

function captures(values: number[], extra: number[]): void {
  console.log(values.map((n: number): number => n + extra.length).join());
  let acc = extra;
  acc = values;
  console.log([1].map((x: number): number => acc.length + x).join());
  const store: number[] = [];
  const add = (x: number): void => { store.push(x); };
  [1, 2, 3].forEach(add);
  console.log(store.join());
}

function main(): void {
  callbacks([1, 2]);
  console.log(opaque([0, "", null], (v) => v), opaque([0, 1], (v) => v));
  console.log(selected([1, "a"], true), selected([1, "a"], false));
  console.log(["a", "b"].map(makeCapped()));
  const seen: number[] = [];
  viaForEach([5, 6], (value, index, data) => { seen.push(value + index + data.length); });
  console.log(seen.join());
  console.log(viaFlatMap([1, 2], (value, index) => [value, index]).join());
  console.log(throwing(["a", "b"]), throwing(["a", "x"]));
  inCallbacks();
  const m = new Map<string, number>([["k", 1]]);
  console.log(everyEntry(m, (value, key, original) => original.has(key) && value > 0));
  sums([1, 2, 3]);
  concats([1], [2], [3]);
  pushes();
  console.log(preallocated(3), fromLength(["a", "b"]));
  console.log(filling(["a", "b"], "z").join(), [1, 2, 3].fill(0, 1, 2).join());
  console.log(ranges(5, 3, 2).join());
  console.log(typeofs(1).join(), typeofs("s").join());
  console.log(adjust(1500), adjust(1e21));
  searches();
  slices();
  mutators();
  modern();
  console.log(trimLast([1, 2]).join());
  console.log(pickByIndex([7, 8, 9]).join());
  const holey = holes();
  console.log(holey[1] === undefined, holey[1] === null, holey.length);
  console.log(firstEntry(["p", "q"]), firstEntry([]));
  const swapped: unknown[] = [1, "two"];
  swap(swapped, 0, 1);
  console.log(swapped);
  tuples();
  const [left, right] = widened([1, 2]);
  console.log(left.length + right.length);
  captures([1, 2], [3]);
  const typedNames = ["Float32Array", "Int8Array"];
  const viewNames = [...typedNames, "DataView"];
  console.log(viewNames.join(","));
  const rows2: unknown[][] = [[1], [1, 2, 3]];
  console.log(Math.max(...rows2.map((r) => r.length)));
  const arrays: string[][] = [["x"], ["y", "z"]];
  console.log(arrays.map(({ length }) => length).join());
  console.log(["a", "ab", "abc"].filter(({ length }) => length < 3).length);
}

main();
