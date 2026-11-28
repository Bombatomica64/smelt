// `Array.from`/`Array.of`, `Reflect.has`, `Object.freeze` and the `Promise`
// statics read as values are callable through the copy (the `Promise` ones
// with `Promise` as `this`) and report Node's `name` and `length`.
const from = Array.from;
const of = Array.of;
console.log(from("abc").join("-"), from([1, 2, 3]).length, of(1, 2, 3).length, from.name, from.length, of.name, of.length);
console.log(from(new Set([4, 5])).join(","), of().length, of("x").join(""));
const has = Reflect.has;
console.log(has({ a: 1 }, "a"), has({ a: 1 }, "b"), has.name, has.length);
const freeze = Object.freeze;
const fr = freeze({ x: 1 });
console.log(fr.x, freeze.name, freeze.length);
console.log(Promise.resolve.name, Promise.resolve.length, Promise.reject.name, Promise.all.name, Promise.all.length);
const resolve = Promise.resolve.bind(Promise);
const all = Promise.all.bind(Promise);
const reject = Promise.reject.bind(Promise);
async function main() {
  console.log(await resolve(5));
  console.log((await all([Promise.resolve(1), 2])).join(","));
  try { await reject(new Error("no")); } catch (e) { console.log((e as Error).message); }
  const vals = await Promise.all([3, 4].map((v) => Promise.resolve.call(Promise, v)));
  console.log(vals.join(","));
}
main();
