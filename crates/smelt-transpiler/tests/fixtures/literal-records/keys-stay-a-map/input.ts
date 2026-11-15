// An unannotated object literal whose key set or key order is observed in a
// way a struct cannot answer stays a map.
//
// A struct has exactly the keys its type declares, in declaration order. An
// object's keys are whatever was written into it, in insertion order, except
// that integer-like keys come first in numeric order. So a literal read
// through a computed key, tested with `in`, enumerated with its values, spread
// into another object, or spelled with integer-like keys keeps the map
// lowering it always had (`literal_record_uses`, `Disqualifier::Keys` and
// `Disqualifier::Shape`).
//
// This family is a precaution: lowered as record structs, these particular
// lines print the same. Only these instances were checked, so the fixture
// pins the map lowering and Node's answers.
//
// Every line below is diffed against Node.

function computedKeys(): void {
  const counters = { hits: 1, name: "counters" };
  const names: ("hits" | "name")[] = ["name", "hits"];
  for (const name of names) {
    console.log(name, counters[name]);
  }
  console.log("hits" in counters, "misses" in counters);
}

function enumeration(): void {
  const settings = { zeta: 1, alpha: "first", mid: true };
  console.log(Object.entries(settings).length, Object.values(settings).length);
  const listed = { zeta: 1, alpha: "first", mid: true };
  console.log(Object.keys(listed).join(","));
  for (const key in listed) {
    console.log("key", key);
  }
  const copy = { ...settings, extra: 0 };
  console.log(Object.keys(copy).join(","));
}

function integerKeys(): void {
  // Integer-like keys enumerate first, in numeric order, wherever they were
  // written: declaration order would print `b,2,1`.
  const mixed = { b: "text", 2: 20, 1: true };
  console.log(Object.keys(mixed).join(","));
  console.log(JSON.stringify(mixed));
}

computedKeys();
enumeration();
integerKeys();
