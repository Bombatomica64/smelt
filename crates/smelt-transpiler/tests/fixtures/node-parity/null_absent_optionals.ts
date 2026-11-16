// A type written `T | null` has only `null` as its absent value: erased at a
// dynamic boundary (an `unknown` slot, JSON, `String`) it is `null`, never
// `undefined`. `T | undefined` and optional fields keep `undefined`.
function findNull(items: number[], target: number): number | null {
  const index = items.indexOf(target);
  return index >= 0 ? index : null;
}
function findUndefined(items: number[], target: number): number | undefined {
  const index = items.indexOf(target);
  return index >= 0 ? index : undefined;
}
// The ts-pattern shape: the checker infers `string[] | null` for `pick`.
const pick = (flag: boolean) => (flag ? ["a", "b"] : null);
const out: unknown[] = [findNull([1, 2], 9), findUndefined([1, 2], 9), pick(false), findNull([1, 2], 2)];
console.log(out[0] === null, out[1] === undefined, out[2] === null, out[3]);
console.log(JSON.stringify(out), out.map((value) => String(value)).join("|"));
type Entry = { name: string; parent: string | null; alias?: string };
const entry: Entry = { name: "x", parent: null };
const view: unknown = entry.parent;
const alias: unknown = entry.alias;
console.log(view === null, alias === undefined, typeof view, typeof alias);
function first(values: string[]): string | null {
  return values.length > 0 ? values[0] : null;
}
const kept: Record<string, unknown> = { a: first([]), b: first(["z"]) };
console.log(JSON.stringify(kept), kept.a === null);
// A function answering `null` keeps it through an erased function adapter,
// where a `void` function answers `undefined` (ts-pattern's `.otherwise(() => null)`).
const answerNull = () => null;
const call = (cb: unknown): unknown => (cb as () => unknown)();
console.log(call(answerNull) === null, call(() => null) === null, call(() => {}) === undefined, call(() => undefined) === undefined);
// Through an optional chain the field keeps its `null` absence.
const maybeEntry: Entry | undefined = entry;
const viaChain: unknown = maybeEntry?.parent;
console.log(viaChain === null, JSON.stringify({ viaChain }));
