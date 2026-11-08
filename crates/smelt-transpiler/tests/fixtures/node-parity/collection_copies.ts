// `new Set(iterable)` and `new Map(iterable)` build NEW collections from the
// iterable's members, in iteration order, with their own identity.

const a = new Set([3, 1, 2]);
const b = new Set(a);
b.add(4);
a.delete(1);
console.log(a === b, [...a].join(","), [...b].join(","));

const words = new Set(["x", "y"]);
const moreWords = new Set(words);
moreWords.add("z");
console.log(words.size, moreWords.size, words.has("z"));

const m = new Map<string, number>([["x", 1], ["y", 2]]);
const copy = new Map(m);
copy.set("z", 3);
m.set("x", 10);
console.log(m === copy, m.size, copy.size, copy.get("x"), [...copy.keys()].join(""));

const pairs: [string, number][] = [["p", 1], ["q", 2], ["p", 3]];
const fromPairs = new Map(pairs);
console.log(fromPairs.size, fromPairs.get("p"), [...fromPairs.keys()].join(""));

const record: Record<string, boolean> = { on: true, off: false };
const fromEntries = new Map(Object.entries(record));
console.log(fromEntries.get("on"), fromEntries.get("off"), fromEntries.size);

function count(raw: unknown): number {
  const entries = new Map(raw as [unknown, unknown][]);
  return entries.size;
}
console.log(count([["k", 1], ["k", 2], ["j", 3]]));
