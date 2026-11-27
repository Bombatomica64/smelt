// `toString()` and `valueOf()` on a plain object reach `Object.prototype`
// (`"[object Object]"`, the object itself), and an array of instances whose
// class defines `toString` joins and converts through it.
const empty = {};
console.log(`${{}.toString()}`, empty.toString());
const rec = { a: 1 };
console.log(rec.toString(), String(rec), `${rec}`, rec.valueOf() === rec);
const bag: Record<string, number> = { k: 1 };
console.log(bag.toString(), bag.hasOwnProperty("k"));
class P {
  constructor(public x: number) {}
  toString() {
    return `P(${this.x})`;
  }
}
const p = new P(3);
const q = new P(4);
console.log([p, q].join(";"), [p].join());
console.log([p, q].toString(), `${[p, q]}`, String([p]));
class Q {
  constructor(public z: number) {}
  toString(): string {
    if (this.z < 0) throw new Error("negative");
    return "Q" + this.z;
  }
}
class R extends Q {}
console.log([new Q(1), new Q(2)].join("+"), [new R(5)].join());
try {
  console.log([new Q(1), new Q(-1)].join("+"));
} catch (e) {
  console.log("caught", (e as Error).message);
}
class Plain {
  constructor(public y: number) {}
}
console.log([new Plain(1)].join("|"));
