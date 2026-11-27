// `Object.freeze(o)` and `Object.defineProperty(o, key, d)` answer their target
// object, keeping its type: the answer is the same object, never `null`.
const frozen = Object.freeze({ q: 1, label: "a" });
console.log(frozen.q, frozen.label);
const point = { x: 1, y: 2 };
const same = Object.freeze(point);
console.log(same === point, same.x + same.y);
const list = Object.freeze([1, 2, 3]);
console.log(list.length, list[1]);
// A symbol key is not a string-keyed property, so the descriptor is not
// modelled; the call still answers its target.
const tag = Symbol("tag");
const target = { n: 5 };
const back = Object.defineProperty(target, tag, { value: 1 });
console.log(back === target, back.n);
// A target that is not a plain name is evaluated once, before the other
// arguments.
let calls = 0;
function make(): { n: number } {
  calls += 1;
  return { n: 7 };
}
const made = Object.defineProperty(make(), tag, { value: 2 });
console.log(made.n, calls);
