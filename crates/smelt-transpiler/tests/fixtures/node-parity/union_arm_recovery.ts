// An erased value recovered into a union picks the arm its shape belongs to:
// same-arity tuples by their elements' kinds, lists by their elements, and
// object shapes that differ only in a literal field by that literal.
type Pair = [1, number] | ["two", string] | [3, boolean];
const pairs = JSON.parse('[[1, 3], ["two", "x"], [3, true]]') as Pair[];
console.log(pairs.map((pair) => typeof pair[0] + ":" + typeof pair[1]).join(" "));
const asserted = ["two", "x"] as Pair;
console.log(JSON.stringify(asserted));

type Holder =
  | { type: "a"; items: ({ some: string; data: number } | string)[] }
  | { type: "b"; items: { other: boolean; data: string }[] };
const holders = JSON.parse(
  '[{"type":"a","items":["s",{"some":"x","data":1}]},{"type":"b","items":[{"other":true,"data":"d"}]}]',
) as Holder[];
console.log(holders.map((holder) => `${holder.type}:${holder.items.length}`).join(" "));
const lists = JSON.parse('[[{"some":"x","data":1}], [{"other":false,"data":"y"}]]') as (
  | { some: string; data: number }[]
  | { other: boolean; data: string }[]
)[];
console.log(lists.map((list) => Object.keys(list[0]).join("+")).join(" "));

// Two shapes with the same fields, told apart by a literal.
type Shape = { kind: "a"; size: number } | { kind: "b"; size: number } | { kind: "c"; size: boolean };
const sizeOf = (shape: Shape): string => `${shape.kind}=${shape.size}`;
const shapes = JSON.parse('[{"kind":"b","size":10},{"kind":"a","size":2},{"kind":"c","size":true}]') as Shape[];
console.log(shapes.map(sizeOf).join(" "));
type Other = { kind: "a"; size: number; extra: string };
const other: Other = { kind: "a", size: 1, extra: "e" };
console.log(other.extra);
