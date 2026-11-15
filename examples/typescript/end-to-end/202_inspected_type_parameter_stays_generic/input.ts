// A generic function may ask a value of its type parameter whether it is
// absent, what `typeof` it has and whether it is truthy, and stay generic:
// the question is answered on an erased copy, the value itself keeps its type.
class Box {
  constructor(public size: number) {}
}
function isNil<T>(value: T): boolean {
  return value == null;
}
function isMissing<T>(value: T): boolean {
  return value === undefined;
}
function isNull<T>(value: T): boolean {
  return value === null;
}
function kind<T>(value: T): string {
  return typeof value;
}
function truthy<T>(value: T): boolean {
  return !!value;
}
function orElse<T>(value: T, fallback: T): T {
  return value ? value : fallback;
}
function describe<T>(value?: T): string {
  return value ? "set" : "unset";
}
function compact<T>(items: T[]): T[] {
  const out: T[] = [];
  for (const item of items) {
    if (item) {
      out.push(item);
    }
  }
  return out;
}
function countStrings<T>(items: T[]): number {
  let count = 0;
  for (const item of items) {
    if (typeof item === "string") {
      count += 1;
    }
  }
  return count;
}
function keepDefined<T>(items: T[]): T[] {
  return items.filter((item) => item != null);
}
function tally<T>(items: T[]): string {
  const kinds: string[] = [];
  items.forEach((item) => {
    kinds.push(typeof item);
  });
  return kinds.join(",");
}
// The list is the caller's own: a push through the generic function is seen
// by every alias, and the returned list is the same list.
function appendIfPresent<T>(items: T[], value: T): T[] {
  if (value != null) {
    items.push(value);
  }
  return items;
}
function pushIfTruthy<T>(items: T[], value: T): number {
  if (value) {
    items.push(value);
  }
  return items.length;
}
const box = new Box(2);
console.log(isNil(1), isNil("a"), isNil(box), isNil(0));
console.log(isMissing(1), isMissing(false), isMissing(box));
console.log(isNull(1), isNull(""), isNull(box));
console.log(kind(1), kind("a"), kind(true), kind(box), kind(1n));
console.log(truthy(box), truthy(0), truthy(""), truthy("0"), truthy(NaN), truthy(2));
console.log(orElse(0, 5), orElse(2, 5), orElse("", "x"), orElse("y", "x"), orElse(box, new Box(9)).size);
console.log(describe<number>(), describe(0), describe(1), describe("x"), describe(box));
console.log(compact([0, 1, 2, 0, 3]).join(","), compact(["", "a", "b"]).join(","));
console.log(countStrings(["a", "b"]), countStrings([1, 2]));
console.log(keepDefined([1, 2, 3]).length, tally([1, 2]), tally(["a"]), tally([box]));
const shared = [1, 2];
const same = appendIfPresent(shared, 3);
same.push(4);
console.log(shared.join(","), same === shared);
console.log(pushIfTruthy(shared, 5), pushIfTruthy(shared, 0), shared.join(","));
const boxes = [box];
console.log(pushIfTruthy(boxes, new Box(5)), boxes[1].size, boxes.length);
