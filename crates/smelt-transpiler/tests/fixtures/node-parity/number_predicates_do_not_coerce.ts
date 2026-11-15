// `Number.isInteger`/`isFinite`/`isNaN`/`isSafeInteger` answer `false` for
// every argument that is not a Number; the global `isNaN`/`isFinite` convert
// their argument first.
function check(v: unknown): string {
  return [Number.isInteger(v), Number.isFinite(v), Number.isNaN(v), Number.isSafeInteger(v)].join(",");
}
console.log(check("5"), check(5), check(5.5), check("x"), check(null), check(undefined));
console.log(check(Infinity), check(NaN), check(true), check([7]), check(2 ** 53));

function checkU(v: string | number): string {
  return [Number.isFinite(v), Number.isInteger(v), Number.isNaN(v)].join(",");
}
console.log(checkU("7"), checkU(7), checkU("NaN"), checkU(NaN));

function checkO(v?: number): string {
  return [Number.isFinite(v), Number.isNaN(v)].join(",");
}
console.log(checkO(), checkO(3), checkO(NaN));

function checkS(v: string): boolean {
  return Number.isFinite(v as unknown);
}
console.log(checkS("12"));

const loose: unknown[] = ["12", "x", "", null, 4];
console.log(loose.map((v) => isNaN(v as any)).join(","));
console.log(loose.map((v) => isFinite(v as any)).join(","));
