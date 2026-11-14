// A record value passed where a `Record<K, V>` is expected is the record of
// its own properties.
//
// TypeScript assigns an object type to `Record<K, V>` structurally, and
// JavaScript reads it through the same keys. A value typed by an object type
// alias (or a heterogeneous inline object type) lowers to a struct, and nothing converted a
// struct to a Rust record map, so passing one to a `Record<K, V>` parameter
// did not compile (E0308). The conversion goes through the struct's erasure
// (the object of its fields) and the record extraction from an erased
// object, the two conversions an erased value of this shape already takes.
//
// Every line below is diffed against Node.

type Names = {
  first: string;
  last: string;
};

function joinValues(record: Record<string, string>): string {
  return Object.keys(record)
    .map((key) => `${key}=${record[key]}`)
    .join("&");
}

function longest(record: Record<string, string>): number {
  let best = 0;
  for (const key in record) {
    best = Math.max(best, record[key].length);
  }
  return best;
}

const names: Names = { first: "Ada", last: "Lovelace" };
console.log(joinValues(names));
console.log(longest(names), names.first);
