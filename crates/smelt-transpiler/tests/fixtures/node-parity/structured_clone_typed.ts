interface Item { name: string; tags: string[]; meta: Map<string, number>; seen: Set<string>; when: Date }
const items: Item[] = [{ name: "a", tags: ["x"], meta: new Map([["k", 1]]), seen: new Set(["s"]), when: new Date(0) }];
const c: Item = structuredClone(items[0]);
c.tags.push("y");
c.meta.set("k2", 2);
let copiedSeen = c.seen;
copiedSeen.add("t");

c.name = "b";
console.log(items[0].tags.length, c.tags.length, items[0].meta.size, c.meta.size);
console.log(items[0].seen.size, c.seen.size, items[0].when.getTime(), c.when.getTime(), c.when === items[0].when, items[0].name, c.name);

const shared: number[] = [1, 2];
const pair: number[][] = [shared, shared];
const pairCopy = structuredClone(pair);
pairCopy[0].push(3);
console.log(pair[0].length, pairCopy[0].length, pairCopy[1].length, pairCopy[0] === pairCopy[1], pairCopy[0] === shared);

type TreeNode = { id: number; children: TreeNode[]; parent: TreeNode | null };
function cyclic(): void {
  const root: TreeNode = { id: 1, children: [], parent: null };
  const leaf: TreeNode = { id: 2, children: [], parent: root };
  root.children.push(leaf);
  const rootCopy = structuredClone(root);
  console.log(rootCopy.children[0].parent === rootCopy, rootCopy.children[0].parent === root, rootCopy.children[0].id);
}
cyclic();

const scores: Record<string, number[]> = { a: [1] };
const scoresCopy = structuredClone(scores);
scoresCopy.a.push(2);
console.log(scores.a.length, scoresCopy.a.length);

const re = /ab+/g;
re.lastIndex = 2;
const reCopy = structuredClone(re);
console.log(reCopy.source, reCopy.flags, reCopy.lastIndex, reCopy === re);

const tuple: [string, number[]] = ["t", [1]];
const tupleCopy = structuredClone(tuple);
tupleCopy[1].push(2);
console.log(tuple[1].length, tupleCopy[1].length);

interface WithCallback { label: string; run: () => number }
const holder: WithCallback = { label: "h", run: () => 1 };
try {
  structuredClone(holder);
  console.log("cloned");
} catch (e) {
  console.log("threw", (e as Error).name);
}

class Counter {
  count = 0;
  history: number[] = [];
  label?: string;
  mode: "a" | number = "a";
}
const counter = new Counter();
counter.history.push(1);
const counterCopy = structuredClone(counter);
counterCopy.history.push(2);
counterCopy.count = 5;
console.log(counter.count, counter.history.length, counterCopy.count, counterCopy.history.length, counterCopy.label, counterCopy.mode);
