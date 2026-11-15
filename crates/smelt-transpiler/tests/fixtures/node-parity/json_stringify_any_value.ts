// `JSON.stringify` has an answer for every value (ECMA-262): `undefined`, a
// function or a symbol at the top level gives `undefined` (not the text
// "null"), inside an object it is omitted and inside an array it is `null`;
// a regular-expression match result is an array, and no match is `null`.
const missing: unknown = undefined;
const fn = () => 1;
console.log(JSON.stringify(undefined), JSON.stringify(missing), JSON.stringify(missing) === undefined);
console.log(typeof JSON.stringify(missing), JSON.stringify(fn as unknown), JSON.stringify(Symbol("s") as unknown));
console.log(JSON.stringify({ a: undefined, b: 1, f: fn }), JSON.stringify([undefined, fn, 2]));
console.log(JSON.stringify(null), JSON.stringify(missing, null, 2), JSON.stringify({ k: [1] } as unknown, null, 2));
console.log("value=" + JSON.stringify(missing));
const found = "a1b2".match(/(\d)/);
console.log(JSON.stringify(found), JSON.stringify("a1b2".match(/\d/g)), JSON.stringify("zz".match(/\d/)));
const optional: number | undefined = [1, 2].find((n) => n > 5);
console.log(JSON.stringify(optional), JSON.stringify([optional]));
// A value whose absent case the checker types as `null` (not `undefined`)
// always has a JSON text: `"null"`, a string `length` 4.
function pick(flag: boolean): number[] | null {
  return flag ? [1] : null;
}
console.log(JSON.stringify(pick(true)), JSON.stringify(pick(false)), JSON.stringify(pick(false)).length);
console.log(JSON.stringify("zz".match(/\d/)).length, JSON.stringify(null).length);
