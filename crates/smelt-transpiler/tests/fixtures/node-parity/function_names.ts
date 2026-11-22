// `Function.prototype.name` of typed function values (no erased runtime).
function hello(a: number, b: number): number {
  return a + b;
}
const arrow = (x: number): number => x * 2;
const expression = function (x: number): number {
  return x + 1;
};
const ownName = function inner(x: number): number {
  return x - 1;
};
const wrapped = ((x: number): number => x) as (x: number) => number;
let assignedLater: (x: number) => number;
assignedLater = (x: number): number => x;
const alias = hello;

class Counter {
  count = 0;
  bump(): number {
    this.count += 1;
    return this.count;
  }
}

function withDefault(cb: (x: number) => number = (x: number): number => x): string {
  return "default param";
}

function nested(): string {
  function helper(): number {
    return 1;
  }
  const local = (): number => helper();
  return `${helper.name} ${local.name}`;
}

console.log(hello.name, hello.length);
console.log(arrow.name, expression.name, ownName.name, wrapped.name);
console.log(assignedLater.name, alias.name);
const counter = new Counter();
console.log(counter.bump.name, counter.bump());
console.log(nested());
console.log(arrow.name.length, `${hello.name}!`, arrow.name === "arrow");
console.log(withDefault());
