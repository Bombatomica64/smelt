// A typed Map's or Set's `keys()`/`values()`/`entries()` is a live,
// single-pass iterator, not an array: it stringifies as `{}`, `next()` and
// `for...of`/spread consume it, `[Symbol.iterator]()` is itself, and it sees
// entries added after it was made.
function main(): void {
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  const keys = m.keys();
  console.log(JSON.stringify(keys), keys[Symbol.iterator]() === keys);
  console.log(keys.next().value, [...keys].join(","), [...keys].length);
  const entries = m.entries();
  for (const [k, v] of entries) console.log(k, v);
  console.log(Array.from(entries).length);
  const lazy = m.values();
  m.set("c", 3);
  console.log(Array.from(lazy).join(","));

  const numbers = new Map<number, string>([[1, "one"]]);
  console.log(Array.from(numbers.keys()).join(","), Array.from(numbers.entries()).map(([k, v]) => `${k}=${v}`).join(","));

  const s = new Set<number>([1, 2]);
  const values = s.values();
  values.next();
  console.log(Array.from(values).join(","), Array.from(values).length);
  console.log(Array.from(s.entries()).map(([a, b]) => `${a}:${b}`).join(","));
  const later = s.keys();
  s.add(9);
  console.log([...later].join(","));

  // `for...of` walks a Map or Set live: an entry added during the loop is
  // visited, on the collection itself and through its iterators.
  const grow = new Map<string, number>([["a", 1], ["b", 2]]);
  for (const [k, v] of grow.entries()) {
    if (k === "a") grow.set("z", 26);
    console.log("entry", k, v);
  }
  for (const [k, v] of grow) console.log("direct", k, v);
  for (const k of grow.keys()) console.log("key", k);
  const members = new Set<number>([1, 2]);
  for (const x of members) {
    if (x === 1) members.add(3);
    console.log("member", x);
  }
  for (const [a, b] of members.entries()) console.log("pair", a, b);
}

main();
