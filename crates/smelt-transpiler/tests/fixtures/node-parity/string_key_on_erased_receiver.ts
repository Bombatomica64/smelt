// A string key is a property name on whatever the erased receiver holds.
function isObject(v?: unknown): v is object { return v !== null && (typeof v === "object" || typeof v === "function"); }
function lastKey<T>(obj: T | null | undefined, pred: (v: unknown, k: string) => boolean): string | undefined {
  if (!isObject(obj)) { return undefined; }
  const keys = Object.keys(obj);
  return keys.findLast((key) => pred(obj[key as keyof T], key));
}
console.log(lastKey([1, 2, 3], (v) => (v as number) > 1));
console.log(lastKey({ a: 1, b: 2 }, (v) => (v as number) < 2));
console.log(lastKey("abc" as unknown as object, (v) => v === "b"));
function readAll<T>(obj: T | null | undefined, key: string): unknown { if (obj == null) { return "none"; } return (obj as any)[key]; }
console.log(readAll([5, 6], "1"), readAll([5, 6], "length"), readAll([5, 6], "x"), readAll("hi", "1"), readAll({ q: 1 }, "q"), readAll(null, "q"));
function viaOptional(obj: any, key: string): unknown { return obj?.[key]; }
console.log(viaOptional([7, 8], "1"), viaOptional([7, 8], "length"), viaOptional([7, 8], "x"), viaOptional("hi", "0"), viaOptional(null, "a"), viaOptional({ a: 1 }, "a"));
function eachKey<T>(obj: T | null | undefined, keys: string[]): unknown[] { return keys.map((key) => (obj as any)?.[key]); }
console.log(eachKey([4, 5, 6], ["2", "0", "nope"]));
