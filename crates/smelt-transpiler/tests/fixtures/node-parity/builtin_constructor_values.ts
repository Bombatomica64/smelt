// A builtin named as a value is the same value `.constructor` reads, an alias of a
// builtin is callable from any body, and a callback chosen at run time is the one
// that runs.
const isObj = (value: any): boolean => !!value && value.constructor === Object;
const isSym = (value: any): boolean => !!value && value.constructor === Symbol;
console.log(isObj({}), isObj([1]), isObj(new Date()), isObj(1), isObj(null));
console.log(isSym(Symbol("")), isSym(Symbol("h")), isSym("x"));
const ctorOf = (value: any): boolean[] => [
  value.constructor === Array,
  value.constructor === Number,
  value.constructor === String,
  value.constructor === Boolean,
];
console.log(ctorOf([]), ctorOf(1), ctorOf("s"), ctorOf(true));

export const isA = Array.isArray;
const abs = Math.abs;
function viaAlias(v: any): unknown[] {
  return [isA(v), abs(-3)];
}
console.log(viaAlias([1]), viaAlias({}));

function order(list: number[], desc: boolean): number[] {
  const asc = (a: number, b: number) => a - b;
  const dsc = (a: number, b: number) => b - a;
  return list.slice().sort(desc === true ? dsc : asc);
}
console.log(order([2, 0, 1], true), order([2, 0, 1], false));
function firstMatch(list: number[], even: boolean): number | undefined {
  const isEven = (n: number) => n % 2 === 0;
  const isOdd = (n: number) => n % 2 === 1;
  return list.find(even ? isEven : isOdd);
}
console.log(firstMatch([1, 2], true), firstMatch([1, 2], false));
