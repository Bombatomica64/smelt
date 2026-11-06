import { assertText, isPlainKey, isText as isString } from "./predicates.ts";

function find(source: readonly unknown[] | Record<string, unknown> | string | null, target: unknown): boolean {
  if (source == null) {
    return false;
  }
  if (isString(source)) {
    return source.includes(target as string);
  }
  if (Array.isArray(source)) {
    return source.includes(target);
  }
  return Object.keys(source).includes(target as string);
}

function shape(path: string | string[]): string {
  if (isPlainKey(path)) {
    return "key:" + path.length;
  }
  if (!Array.isArray(path)) {
    return "path:" + path;
  }
  return "list:" + path.length;
}

function shout(value: unknown): string {
  assertText(value);
  return value.toUpperCase();
}

console.log(find("hello", "ll"), find("hello", "zz"));
console.log(find(["a", 2], 2), find(["a", 2], 3));
console.log(find({ a: 1 }, "a"), find({ a: 1 }, "b"), find(null, "a"));
console.log(shape("key"), shape("a.b"), shape(["a", "b"]));
console.log(shout("hi"));
