// Unannotated literals holding functions spelled in place (methods, `function`
// values, arrows) take the record type the checker infers, while no method
// reads its own `this`.
let total = 0;
const calc = {
  factor: 3,
  scale(x: number) {
    return x * 2;
  },
  add: (n: number) => {
    total += n;
    return total;
  },
  neg: function (x: number) {
    return -x;
  },
  label: "c",
};
console.log(calc.scale(4), calc.add(5), calc.add(1), calc.neg(2), calc.factor, calc.label);

// A method read off the record is the one closure the field holds.
const scale = calc.scale;
console.log(scale(10), scale === calc.scale, calc.scale === calc.scale);

// A data field written after construction is seen through every alias.
const alias = calc;
calc.factor = 7;
console.log(alias.factor, alias === calc);

// An array of such literals, iterated and called.
const ops = [
  { name: "one", run() { return 1; } },
  { name: "two", run() { return 2; } },
];
let sum = 0;
for (const op of ops) {
  sum += op.run();
  console.log(op.name);
}
console.log(sum);

// Handed to a slot of an interface declaring exactly its members.
interface Shape {
  sides: number;
  area(scale: number): number;
}
function describe(shape: Shape): string {
  return `${shape.sides}:${shape.area(2)}`;
}
const square = { sides: 4, area(scale: number) { return scale * scale; } };
console.log(describe(square));

// A method that reads its own `this` keeps the map (bound receivers come next).
const counter = {
  n: 0,
  inc() {
    this.n += 1;
    return this.n;
  },
};
counter.inc();
console.log(counter.inc(), counter.n);
