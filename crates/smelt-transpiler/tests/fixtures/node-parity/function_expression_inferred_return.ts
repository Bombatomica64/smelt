// Unannotated function expressions take the checker's inferred return, and a
// typed view of a function keeps that function's own `length`.
function retype(loose: any): (a: number, b: number) => number {
  return loose;
}
function partial<A, B, R>(func: (a: A, b: B) => R, first: A): (b: B) => R;
function partial(func: (...args: any[]) => any, ...bound: any[]): (...args: any[]) => any {
  return function (...rest: any[]) {
    return func(...bound, ...rest);
  };
}

const add = function (x: number, y: number) {
  return x + y;
};
const shout = function (name: string) {
  return name.toUpperCase() + "!";
};
const pair = function (n: number) {
  return [n, n * 2];
};
const nothing = function (_a: string, _b: string) {};
const later = async function (n: number) {
  return n + 1;
};

const sum = add(2, 3);
console.log(sum, typeof sum, sum.toFixed(1));
console.log(shout("hi"), shout("hi").length);
console.log(pair(4).length, pair(4)[1] + 1);
console.log(nothing("a", "b") === undefined);
const counted = retype(function (...xs: number[]) {
  return xs.length;
});
console.log(add.length, nothing.length, counted(1, 2), counted.length);
const inc = partial(add, 1);
console.log(inc(41), inc.length);
later(9).then((v) => console.log(v * 2));
