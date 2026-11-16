// `for...of` over a tuple iterates its elements in order, each typed as the
// union of the element types; every iteration reads the tuple's current element.

const triple: [string, number, boolean] = ["x", 2, true];
let text = "";
for (const part of triple) {
  if (typeof part === "number") text += part * 10;
  else if (typeof part === "boolean") text += part ? "T" : "F";
  else text += part.toUpperCase();
}
console.log(text);

function describe(entry: readonly [string, number]): string[] {
  const out: string[] = [];
  for (const value of entry) out.push(`${typeof value}:${value}`);
  return out;
}
console.log(describe(["k", 3]).join(" "));

// A write to a later element inside the body is seen by the next iteration.
const live: [number, string] = [1, "before"];
for (const item of live) {
  console.log("live", item);
  live[1] = "after";
}

// break, continue and a labelled continue leave the element reads intact.
const mixed: [number, string, number, string] = [1, "skip", 3, "stop"];
const seen: (number | string)[] = [];
for (const item of mixed) {
  if (item === "skip") continue;
  if (item === "stop") break;
  seen.push(item);
}
console.log(seen.join(","));
outer: for (const row of [1, 2]) {
  for (const cell of ["a", row] as [string, number]) {
    if (cell === "a") continue;
    if (cell === 2) continue outer;
    console.log("cell", row, cell);
  }
}

// Each iteration is its own binding for a closure.
const callbacks: (() => string)[] = [];
for (const value of [7, "seven"] as [number, string]) callbacks.push(() => `cb ${value}`);
console.log(callbacks.map((callback) => callback()).join(" | "));

// A tuple of same-typed pairs destructures each pair.
const pairs: [[string, number], [string, number]] = [["a", 1], ["b", 2]];
for (const [key, count] of pairs) console.log(key, count * 2);

// An empty tuple runs the body zero times; a homogeneous tuple keeps its type.
const none: [] = [];
for (const item of none) console.log("never", item);
const same: [number, number] = [4, 5];
let sum = 0;
for (const n of same) sum += n;
console.log("sum", sum);

// A class field and a call result iterate the same way.
class Holder {
  items: [string, boolean] = ["s", false];
  run(): string {
    let out = "";
    for (const value of this.items) out += String(value) + ";";
    return out;
  }
}
console.log(new Holder().run());
function make(): [string, number] { return ["z", 9]; }
for (const value of make()) console.log("made", value);
