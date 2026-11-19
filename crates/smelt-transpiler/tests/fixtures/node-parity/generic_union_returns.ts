// A generic function returning a union over its own type parameter is
// generic over that union (`docs/design/record-identity.md`, generic unions):
// `pick(true, 5, "s")` returns the number 5, not the string "5", and each
// call converts its result arm by arm at its own instantiation.

function pick<T>(c: boolean, a: T, b: string): T | string { return c ? a : b; }
const r = pick(true, 5, "s");
console.log(typeof r, r, pick(false, 5, "s"));
const o = pick(true, { z: 1 }, "t");
console.log(typeof o === "string" ? o : o.z);
const flags = [pick(true, false, "no"), pick(false, true, "yes")];
console.log(flags.map((f) => typeof f).join(","), flags.join(","));

function orFallback<T>(xs: T[], fallback: string): T | string {
  return xs.length > 0 ? xs[0] : fallback;
}
console.log(orFallback([3, 4], "none"), orFallback([] as number[], "none"));

function either<A, B>(left: boolean, a: A, b: B): A | B { return left ? a : b; }
const e1 = either(true, 1, "one");
const e2 = either(false, 1, "one");
console.log(typeof e1, typeof e2, e1, e2);

type Key = string | number | symbol;
function keyOf<T, K extends Key>(item: T, toKey?: (item: T) => K): Key {
  const key = toKey ? toKey(item) : (item as unknown as Key);
  return key;
}
console.log(keyOf({ id: 7 }, (x) => x.id), keyOf("raw"));
