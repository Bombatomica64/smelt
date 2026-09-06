// An object literal whose target type is an interface with optional fields is
// built AS THE STRUCT -- `Config { label: None, count: None }` -- rather than
// materialized as an erased `SmeltRecord<String, SmeltUnknown>` and then
// reconstructed field by field. The reconstruction was avoidable erasure and,
// because it funnelled every tag through `to_string()`, would have turned
// `count?: number` into its decimal text.
interface Config {
  label?: string;
  count?: number;
}

interface Outer {
  inner?: Config;
}

const empty: Config = {};
const partial: Config = { label: 'x' };
const counted: Config = { count: 41 };
const nested: Outer = { inner: { count: 7 } };

function labelOf(config: Config): string {
  const label = config.label;
  return label === undefined ? 'none' : label;
}

function countPlusOne(config: Config): number {
  const count = config.count;
  return count === undefined ? -1 : count + 1;
}

function innerCount(outer: Outer): number {
  const inner = outer.inner;
  return inner === undefined ? -2 : countPlusOne(inner);
}

console.log(labelOf(empty));
console.log(countPlusOne(empty));
console.log(labelOf(partial));
// The numeric field stays a number: 42, not "41" concatenated.
console.log(countPlusOne(counted));
console.log(innerCount(nested));
