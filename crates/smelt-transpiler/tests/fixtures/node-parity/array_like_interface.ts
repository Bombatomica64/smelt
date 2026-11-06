// An interface with a numeric index signature admits a list.
interface Rec<T> { [index: number]: T | Rec<T>; length: number; }
function isNil(x?: unknown): x is null | undefined { return x == null; }
function flatDeep<T>(array: Rec<T> | null | undefined, depth = 1): T[] {
  const out: T[] = [];
  const list = array as unknown as unknown[];
  if (list == null) { return out; }
  for (const item of list) { if (Array.isArray(item) && depth > 0) { out.push(...flatDeep(item as unknown as Rec<T>, depth - 1)); } else { out.push(item as T); } }
  return out;
}
function mapAll(collection: unknown, iteratee?: (x: any) => any): unknown[] { return (collection as unknown[]).map(iteratee ?? ((x) => x)); }
function flatMapAny<R = any>(collection: object | null | undefined, iteratee?: any): R[] {
  if (isNil(collection)) { return []; }
  // @ts-ignore
  const mapped = isNil(iteratee) ? mapAll(collection) : mapAll(collection, iteratee);
  return flatDeep(mapped, 1) as R[];
}
console.log(flatMapAny([1, 2], (n: number) => [n, n * 2]), flatMapAny([[1], [2]]), flatMapAny(null));
