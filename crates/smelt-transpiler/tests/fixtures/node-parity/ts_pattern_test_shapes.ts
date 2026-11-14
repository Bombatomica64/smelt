// Shapes from ts-pattern's test suites:
// - bindings of a value Smelt erases but TypeScript types (a conditional
//   return type) take TypeScript's type, so `a + b` adds numbers;
// - `new Set(..)` passed to a `Set<string | number>` parameter is built at
//   that type, and a mixed literal infers from every element;
// - an object type keyed by a block-local `Symbol()` is a keyed record that the
//   literal building it matches;
// - a member-less abstract class is the union of its concrete subclasses,
//   which keeps `instanceof` and the subclasses' fields;
// - a class holding (through `?:` or a union) a value of its own class.
type Unwrap<T> = T extends Promise<infer U> ? U : T;
function unwrap<T>(value: T): Unwrap<T> {
  return value as any;
}

const countSet = (set: Set<string | number>): string =>
  [...set].map((item) => typeof item).join(",");
function sizeOf(set: Set<string | number>): number {
  return set.size;
}

abstract class Shape {}
class Circle extends Shape {
  radius: number;
  constructor(radius: number) {
    super();
    this.radius = radius;
  }
}
class Pair extends Shape {
  a?: Shape;
  b?: Shape;
  constructor(a?: Shape, b?: Shape) {
    super();
    this.a = a;
    this.b = b;
  }
}

function describe(shape: Shape): string {
  if (shape instanceof Circle) return `circle ${shape.radius}`;
  if (shape instanceof Pair) {
    const left = shape.a ? describe(shape.a) : "-";
    const right = shape.b ? describe(shape.b) : "-";
    return `pair(${left}, ${right})`;
  }
  return "shape";
}

class ListNode {
  value: number;
  next?: ListNode;
  constructor(value: number, next?: ListNode) {
    this.value = value;
    this.next = next;
  }
}

function symbols(): string {
  const keyA = Symbol("a");
  const keyB = Symbol("b");
  type Nested = { [keyA]: { [keyB]: "foo" | "bar" } };
  const read = (obj: Nested) => obj[keyA][keyB];
  return read({ [keyA]: { [keyB]: "foo" } });
}

function main(): void {
  const pair: [number, number] = [1, 2];
  const [a, b] = unwrap(pair);
  const total = unwrap(40);
  console.log(a + b, total + 2);

  console.log(countSet(new Set(["gab", "yo"])), countSet(new Set([2])), sizeOf(new Set(["x", 1, "x"])));
  const mixed = new Set(["a", 2, "a"]);
  console.log(sizeOf(mixed), countSet(mixed));

  console.log(symbols());

  const shapes: Shape[] = [new Circle(1), new Pair(new Circle(2), new Pair())];
  for (const shape of shapes) console.log(describe(shape), shape instanceof Shape);
  const label = (s: Shape) => (s instanceof Pair ? "pair" : "other");
  console.log(label(new Pair(new Circle(3))), label(new Circle(4)));

  const list = new ListNode(1, new ListNode(2, new ListNode(3)));
  let sum = 0;
  for (let node: ListNode | undefined = list; node; node = node.next) sum += node.value;
  console.log(sum);
}

main();
