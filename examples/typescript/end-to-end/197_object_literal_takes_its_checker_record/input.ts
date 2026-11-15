// An unannotated object literal whose every use keeps it typed is a record of
// its own shape, not a map of erased values.
//
// `{ id: 1, enabled: false }` has no contextual type, and a `Dict` needs one
// value type for every key, so the literal used to become
// `Dict<String, SmeltUnknown>` and every property read lost its type. The
// TypeScript checker types the binding `{ id: number; enabled: boolean }`; a
// hand port declares a struct of exactly that. The checker's type is spelled
// as that type literal and lowered as if the source had written it, which
// gives the same structurally named struct an annotation gives.
//
// It is taken only when the uses allow it (`literal_record_uses`): property
// reads and writes with static keys, aliases, closures, `for..of`, and the
// observers that read the value at once in its own key order (`console.log`,
// `JSON.stringify`). Every binding below is used only that way, so the
// program holds no erased value at all. The literals that keep the map are the
// fixtures of `smelt-transpiler/tests/literal_record_cli_tests.rs`.
//
// Every line below is diffed against Node.

const config = { label: "retry", times: 3, verbose: false };

function describeItems(): string {
  const items = [
    { id: 1, enabled: false },
    { id: 2, enabled: true },
    { id: 3, enabled: false },
  ];
  let total = 0;
  for (const item of items) {
    if (item.enabled || item.id > 2) {
      item.id *= 10;
    }
    total += item.id;
  }
  return `${total}:${items[1].id}:${items.length}`;
}

function aliasesShareTheRecord(): void {
  // An alias and a closure reach the same object: a write through either is
  // read through the other, and printing follows the source key order (`b`
  // before `a`), not an alphabetical one.
  const point = { b: 1, a: "x", inner: { depth: 2, name: "in" } };
  const alias = point;
  alias.b = 21;
  const bump = (): void => {
    point.b += 1;
    point.inner.depth += 1;
  };
  bump();
  console.log(point.b, alias.b, alias.inner.depth, point.inner.name);
  console.log(point);
  console.log(JSON.stringify(alias));
}

// The same keys in the other order are another record: each keeps its own.
const swapped = { a: "y", b: 2 };
const person = { name: "mike", age: 20, tags: ["a", "b"] };

console.log(describeItems());
aliasesShareTheRecord();
console.log(swapped, JSON.stringify(swapped));
console.log(person.name.toUpperCase(), person.age + 1, person.tags.length, person.tags[1]);
console.log(JSON.stringify(person));
console.log(config.label.length + config.times, config.verbose ? "loud" : "quiet");
