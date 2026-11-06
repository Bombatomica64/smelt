// `groups[key].push(item)` appends in place, inserts a missing key, answers the
// new length, and an item that reads the same dict sees the state before the push.
const groups: Record<string, number[]> = {};
for (const n of [1, 2, 3, 4, 5, 6]) {
  const key = n % 2 === 0 ? "even" : "odd";
  groups[key] ??= [];
  groups[key].push(n);
}
console.log(groups.even.length, groups.odd.length);
console.log(groups.even.join(","), groups.odd.join(","));
console.log(Object.keys(groups).join(","));

const seen: Record<string, number[]> = { a: [] };
for (let i = 0; i < 3; i++) {
  seen["a"].push(seen["a"].length);
}
console.log(seen["a"].join(","));

const before = groups["even"];
groups["even"].push(8);
console.log(before.length, groups["even"].length);

// A number-keyed record is a plain map, so the binding itself has to be mutable.
const byLength: Record<number, string[]> = {};
for (const word of ["a", "bb", "c", "dd", "eee"]) {
  byLength[word.length] ??= [];
  byLength[word.length].push(word);
}
console.log(byLength[1].join(","), byLength[2].join(","), byLength[3].join(","));

// The push is this binding's only write, so nothing else makes it mutable.
const onlyPush: Record<number, string[]> = { 7: ["x"] };
onlyPush[7].push("y");
console.log(onlyPush[7].join(","));
