// Two `tsc` narrowing rules on union locals, and `toString()` on a union.
//
// * The false side of `x instanceof C` removes the `C` arm, so after an
//   early return the rest of the function sees the remaining arms.
// * Assigning a value of one arm narrows later reads to that arm, until the
//   next write or the end of the branch.

class Circle {
  radius: number;
  constructor(radius: number) {
    this.radius = radius;
  }
}

class Square {
  side: number;
  constructor(side: number) {
    this.side = side;
  }
}

function describe(shape: Circle | Square | string): string {
  if (shape instanceof Circle) {
    return `circle ${shape.radius}`;
  }
  if (shape instanceof Square) {
    return `square ${shape.side}`;
  }
  return `named ${shape.toUpperCase()}`;
}

function area(shape?: Circle | Square): number {
  if (shape instanceof Circle) {
    return 3 * shape.radius * shape.radius;
  }
  return shape === undefined ? 0 : shape.side * shape.side;
}

function normalize(input: string | number | URL): string {
  input = input.toString();
  return input.startsWith("http") ? input : `#${input.padStart(3, "0")}`;
}

function widen(flag: boolean): string {
  let value: string | number = 7;
  if (flag) {
    value = "seven";
    console.log(value.length);
  }
  return typeof value === "string" ? value : `n=${value + 1}`;
}

console.log(describe(new Circle(2)), describe(new Square(3)), describe("hex"));
console.log(area(new Circle(1)), area(new Square(4)), area());
console.log(normalize(5), normalize("42"), normalize(new URL("https://a.test/p?q=1")));
console.log(widen(true), widen(false));
