// A mutation through a value read out of a map (`m.get(k)!.push(x)`) changes
// the stored object: the read answers that object itself.
const m = new Map<string, number[]>();
m.set("k", []);
m.get("k")!.push(1);
m.get("k")!.push(2, 3);
console.log(m.get("k")!.length, m.get("k")!.join(","));
const words = ["apple", "avocado", "banana", "blueberry", "cherry"];
const groups = new Map<string, string[]>();
for (const w of words) {
  const key = w[0];
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key)!.push(w);
}
for (const [k, v] of groups) console.log(k, v.join(","));
groups.get("b")!.reverse();
groups.get("a")!.pop();
console.log(groups.get("a")!.join(","), groups.get("b")!.join(","));
const counts = new Map<string, { n: number }>();
counts.set("a", { n: 0 });
counts.get("a")!.n += 2;
console.log(counts.get("a")!.n);
