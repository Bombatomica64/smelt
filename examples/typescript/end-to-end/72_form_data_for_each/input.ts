// A KEYED collection's `forEach` callback is typed from the collection.
//
// `forEach` on a keyed collection receives `(value, key, collection)` — not a
// numeric index — and the loop desugaring already knew that for a `Map` and a
// record. A modeled host collection is keyed in exactly the same way, so which
// collections are keyed now comes from one rule
// (`for_each_keyed_entry_types`) and `FormData` shares it: its key is a
// `string` and its value the same `string | File` union every other member of
// that surface answers.
//
// Before that, `formData.forEach((value, key) => ...)` fell into the ARRAY
// path, where the parameters got no type at all: Hono's
// `convertFormDataToBodyData` reported "string prefix/suffix methods require
// string receiver and argument" for a `key` the surface has always known to be
// a string. The THIRD parameter was simply left unbound for every collection,
// so a callback that declared it reported an unresolved identifier for its own
// parameter; the receiver is bound to a local first so it can be forwarded
// without evaluating the receiver expression twice.
const form = new FormData();
form.append("a", "1");
form.append("b[]", "2");
form.append("b[]", "3");

// The Hono shape: a string method on the key.
const seen: string[] = [];
form.forEach((value, key) => {
  seen.push(`${key}=${String(value)}:${key.endsWith("[]")}`);
});
console.log(seen.join(","));

// One parameter takes the value only, as it does for a `Map`.
const values: string[] = [];
form.forEach((value) => {
  values.push(String(value));
});
console.log(values.join(","));

// The third parameter is the collection itself.
let present = 0;
form.forEach((value, key, parent) => {
  if (parent.has(key)) {
    present += 1;
  }
});
console.log(present);

// The same three shapes over a `Map`, which shares the rule.
const map = new Map<string, number>([
  ["x", 1],
  ["y", 2],
]);
const pairs: string[] = [];
map.forEach((value, key) => {
  pairs.push(`${key}:${value}`);
});
console.log(pairs.join(","));

let mapSize = 0;
map.forEach((value, key, parent) => {
  mapSize = parent.size;
});
console.log(mapSize);
