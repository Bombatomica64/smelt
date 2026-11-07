// A block-local type alias redeclared in a later block is that block's type,
// whatever kind the earlier declaration was; `new Map(..)` takes the map arm of
// a union contextual type.
const withFields = (): number => {
  type Input = { kind: 1; data: number } | { kind: 2 };
  const input = { kind: 1, data: 5 } as Input;
  return input.kind === 1 ? input.data : 0;
};
const mapAfterFields = (): number => {
  type Input = Map<string, 1 | 2 | 3>;
  const input = new Map([["hello", 1]]) as Input;
  return input.size + (input.get("hello") ?? 0);
};
console.log(withFields(), mapAfterFields());
const objectShape = (): string => {
  type Shape = { value: "a" | "b" };
  const shape = { value: "a" } as Shape;
  return shape.value;
};
const mapAfterInterface = (): number => {
  type Shape = Map<string, number>;
  const shape = new Map([["k", 7]]) as Shape;
  return shape.get("k") ?? 0;
};
console.log(objectShape(), mapAfterInterface());
type Entry = { input: string | Map<string, number>; label: string };
const entries: Entry[] = [
  { input: "plain", label: "string" },
  { input: new Map([["key", 2]]), label: "map" },
];
console.log(entries.map((entry) => (typeof entry.input === "string" ? entry.input : entry.input.get("key"))));
