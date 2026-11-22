// A function used at an object type keeps the properties every function owns.
interface Named {
  name: string;
  length: number;
}
interface Labelled {
  name: string;
  label?: string;
}
function hello(a: number, b: number): number {
  return a + b;
}
const arrow = (x: number): number => x;

const n: Named = hello;
const m: Named = arrow;
console.log(n.name, n.length, m.name, m.length);

function describe(value: Named): string {
  return `${value.name}/${value.length}`;
}
console.log(describe(hello), describe(arrow));

const labelled: Labelled = hello;
console.log(labelled.name, labelled.label === undefined);

const erased: unknown = hello;
const viaUnknown = erased as Named;
console.log(viaUnknown.name, viaUnknown.length);
const erasedArrow: unknown = arrow;
const arrowView = erasedArrow as Named;
console.log(arrowView.name, arrowView.length);
console.log(describe(erased as Named), describe(erasedArrow as Named));

function fromUnknown(value: unknown): string {
  const named = value as Named;
  return `${named.name}:${named.length}`;
}
console.log(fromUnknown(hello), fromUnknown(arrow));
console.log(hello(1, 2), arrow(3));
