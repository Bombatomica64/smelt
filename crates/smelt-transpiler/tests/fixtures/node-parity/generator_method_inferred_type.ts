// An unannotated generator method has the generator type the checker infers,
// as an unannotated generator function does: its instances spread, iterate
// and drain through it.
class Range {
  constructor(
    private lo: number,
    private hi: number,
  ) {}
  *[Symbol.iterator]() {
    for (let i = this.lo; i < this.hi; i++) yield i;
  }
}
console.log([...new Range(1, 4)].join(","));
let total = 0;
for (const v of new Range(0, 5)) total += v;
console.log(total);
class Words {
  constructor(private words: string[]) {}
  *each() {
    for (const w of this.words) yield w.toUpperCase();
    return this.words.length;
  }
}
const it = new Words(["a", "b"]).each();
console.log(it.next().value, it.next().value, it.next().value, it.next().done);
console.log(Array.from(new Words(["x", "y"]).each()).join("|"));
