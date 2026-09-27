// A user class may declare methods named like Array.prototype's (`reduce`,
// `map`, `push`, `join`, ...). JavaScript calls the class's own method; the
// array builtin is consulted only for an array receiver. A function literal
// passed to such a method is contextually typed by the declared callback
// parameter.
class Bag {
  items: number[];
  constructor(items: number[]) {
    this.items = items;
  }
  reduce(cb: (acc: number, x: number) => number, init: number): number {
    let acc = init;
    for (const x of this.items) acc = cb(acc, x) + 1;
    return acc;
  }
  reduceRight(cb: (acc: number, x: number) => number, init: number): number {
    return cb(init * 100, 1);
  }
  map(label: string): string {
    return `${label}:${this.items.length}`;
  }
  filter(n: number): number[] {
    return this.items.filter((x) => x > n);
  }
  find(name: string): string {
    return `found ${name}`;
  }
  some(flag: boolean): string {
    return flag ? "some-yes" : "some-no";
  }
  every(): number {
    return this.items.length * 2;
  }
  forEach(prefix: string): string {
    return this.items.map((x) => prefix + x).join(",");
  }
  findIndex(x: number): number {
    return this.items.indexOf(x) + 10;
  }
  total(): number {
    return this.reduce((a, b) => a + b, 0);
  }
}

class Stack {
  items: string[] = [];
  push(x: string, y: string): number {
    this.items.push(x + y);
    return 100 + this.items.length;
  }
  pop(n: number): string { return "pop" + n; }
  shift(n: number): string { return "shift" + n; }
  indexOf(x: string): string { return "indexOf" + x; }
  includes(x: string): string { return "includes" + x; }
  at(i: number): string { return "at" + i; }
  slice(a: number): string { return "slice" + a; }
  join(a: number): string { return "join" + a; }
  concat(a: number): string { return "concat" + a; }
  reverse(a: number): string { return "reverse" + a; }
  sort(a: number): string { return "sort" + a; }
  entries(a: number): string { return "entries" + a; }
}

class Holder {
  stack: Stack = new Stack();
}

function make(): Stack {
  return new Stack();
}

const b = new Bag([1, 2, 3]);
console.log(b.reduce((a, x) => a + x, 0));
console.log(b.reduceRight((a, x) => a + x, 5));
console.log(b.map("m"), b.filter(1).length, b.find("get"), b.some(true));
console.log(b.every(), b.forEach("p"), b.findIndex(2), b.total());
console.log(new Bag([4, 5]).reduce((a, x) => a * x, 1));

const s = new Stack();
console.log(s.push("a", "b"), s.pop(1), s.shift(2), s.indexOf("i"), s.includes("c"));
console.log(s.at(3), s.slice(4), s.join(5), s.concat(6), s.reverse(7), s.sort(8), s.entries(9));
const h = new Holder();
console.log(h.stack.join(20), h.stack.reverse(21), make().pop(22), make().at(23));
function viaGlobal(): string {
  // The module's instance, with its state, not a fabricated empty one.
  return s.join(24) + s.slice(25) + s.items.length;
}
console.log(viaGlobal(), s.items.join("|"));

// Real arrays still take the builtins.
const arr = [1, 2, 3];
console.log(arr.reduce((a, x) => a + x, 0), arr.map((x) => x * 2).join("-"));
