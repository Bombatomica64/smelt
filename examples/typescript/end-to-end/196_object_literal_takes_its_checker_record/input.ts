// An unannotated object literal whose values differ in type is a record of
// its own shape, not a map of erased values.
//
// `{ id: 1, enabled: false }` has no contextual type, and a `Dict` needs one
// value type for every key, so the literal used to become
// `Dict<String, SmeltUnknown>` and every property read lost its type. The
// TypeScript checker types the binding `{ id: number; enabled: boolean }[]`; a
// hand port declares a struct of exactly that. The checker's type is spelled
// as that type literal and lowered as if the source had written it, which
// gives the same structurally named struct an annotation gives. Only the
// binding's own unannotated declaration takes it (a literal stored into a
// typed slot is typed by that slot), a literal whose values share one type
// keeps its map lowering, and a module-level constant takes the same record.
//
// Every line below is diffed against Node.

const config = { label: "retry", times: 3, verbose: false };

function describeItems(): string {
  const items = [
    { id: 1, enabled: false },
    { id: 2, enabled: true },
    { id: 3, enabled: false },
  ];
  const kept = items.filter((item) => item.enabled || item.id > 2);
  return kept.map((item) => item.id * 10).join(",");
}

const person = { name: "mike", age: 20, tags: ["a", "b"] };
const totals = { a: 1, b: 2 };

console.log(describeItems());
console.log(person.name.toUpperCase(), person.age + 1, person.tags.length);
console.log(JSON.stringify(person));
console.log(totals.a + totals.b);
console.log(config.label.length + config.times, config.verbose ? "loud" : "quiet");
