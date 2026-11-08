// `==` and `!=` follow IsLooselyEqual: `null == undefined`, a number
// against a numeric string or a boolean compares the numbers, a bigint
// against a number by mathematical value, and objects by identity.

function looseEq(a: unknown, b: unknown): boolean {
  return a == b;
}

const maybe: number | null = null;
const box = { v: 1 };
const same = box;
const twin = { v: 1 };
const list = [1, 2];

console.log("nullish:", looseEq(null, undefined), maybe == undefined, maybe != null, looseEq(0, null));
console.log("number/string:", looseEq(1, "1"), looseEq(0, ""), looseEq(" 2 ", 2), looseEq("x", 0));
console.log("boolean:", looseEq(true, 1), looseEq(false, "0"), looseEq(true, "2"));
console.log("bigint:", 2n == (2 as number | bigint), looseEq(10n, "10"), looseEq(1n, 1.5));
const nan: number = Number("x");
console.log("NaN:", looseEq(NaN, NaN), nan != nan);
console.log("objects:", looseEq(box, same), looseEq(box, twin), box != twin);
console.log("array to string:", looseEq(list, "1,2"), looseEq([3], 3));
