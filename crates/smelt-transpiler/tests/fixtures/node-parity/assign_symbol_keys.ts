// `Object.assign` copies a source's own enumerable symbol-keyed properties as
// well as its string-keyed ones: a symbol flag, and `Symbol.iterator`, which
// makes the target iterable. String-key enumeration still skips symbols.
const tag = Symbol.for("@demo/tag");
function tagged<T extends object>(target: T) {
  return Object.assign(target, { [tag]: true, b: 2 });
}
function iterable<T extends object>(target: T) {
  return Object.assign(target, {
    [Symbol.iterator]() {
      let i = 0;
      return { next: () => (i < 2 ? { value: i++, done: false } : { value: undefined, done: true }) };
    },
  });
}
const t: any = tagged({ a: 1 });
console.log(t[tag], t.b, Object.keys(t).join(","));
const it: any = iterable({ a: 1 });
console.log(JSON.stringify([...it]));
