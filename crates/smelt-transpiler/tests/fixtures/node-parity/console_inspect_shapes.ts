// `console.log` renders values the way `util.inspect` does.

const values: number[] = [1, 2];
console.log(values);
let count = 42;
console.log(count, "label", true);

interface Point {
  x: number;
  label?: string;
}
class Pair {
  left: number;
  right: string;
  constructor(left: number, right: string) {
    this.left = left;
    this.right = right;
  }
}
const absent: Point = { x: 1 };
const present: Point = { x: 2, label: "two" };
console.log(absent);
console.log(present);
console.log(new Pair(1, "r"));
console.log([absent, present]);
console.log(["a", "b"], [true], [[1], [2, 3]]);

interface Handler {
  name: string;
  run: (x: number) => number;
}
const h: Handler = { name: "h", run: (x: number) => x };
console.log(h.name, h.run(3));
