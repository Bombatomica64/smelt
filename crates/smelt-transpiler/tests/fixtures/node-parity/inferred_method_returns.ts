// An unannotated method, getter, static method, abstract override or
// function declaration returns what TypeScript inferred from its body, so a
// caller reads a typed value instead of the erased one.
class Base {
  static count = 0;
  label = "base";
  who() {
    return this.label;
  }
  static bump() {
    return ++Base.count;
  }
  toString() {
    return "Base(" + this.who() + ")";
  }
  pair(x: number) {
    return [x, x * 2];
  }
  get size() {
    return this.label.length;
  }
  maybe(flag: boolean) {
    return flag ? this.label : undefined;
  }
  async later(x: number) {
    return x + 1;
  }
}

class Derived extends Base {
  who() {
    return "derived:" + super.who();
  }
}

abstract class Shape {
  abstract area(): number;
  describe() {
    return "area " + this.area().toFixed(1);
  }
}

class Square extends Shape {
  side: number;
  constructor(side: number) {
    super();
    this.side = side;
  }
  area() {
    return this.side * this.side;
  }
}

function makeCounter(start: number) {
  let c = start;
  return { next: () => ++c, get: () => c };
}

function scaled(xs: number[], k: number) {
  return xs.map((x) => x * k);
}

const b = new Base();
const d = new Derived();
const shapes: Base[] = [b, d];
for (const s of shapes) {
  console.log(s.who(), String(s), s.pair(3).join("|"), s.size, s.maybe(true), s.maybe(false));
}
console.log(Base.bump() + Base.bump(), Base.count);
const c = makeCounter(5);
c.next();
console.log(c.get(), scaled([1, 2], 3), new Square(1.5).describe());
d.later(41).then((v) => console.log("later", v + 0));
