// A spread fills a rest parameter element by element, whatever the rest's
// element type is and however the callee is reached.
const anyRest: (...args: any[]) => number = (...args) => args.length;
const second: (...args: any[]) => unknown = (...args) => args[1];
const unionRest: (...args: (number | string)[]) => number = (...args) => args.length;
const nested: (...xs: unknown[]) => number = (...xs) => xs.length;
const holder = { f: (...xs: unknown[]) => xs.length };
function viaCallback(cb: (...xs: unknown[]) => number): number {
  const numbers = [1, 2, 3];
  return cb(...numbers);
}
const numbers = [1, 2, 3];
const words = ["p", "q"];
const lists = [[1], [2, 3]];
console.log(anyRest(numbers), anyRest(...numbers));
console.log(second(...words));
console.log(unionRest(...numbers), unionRest(0, ...numbers), unionRest(...numbers, 0));
console.log(nested(...lists), nested(lists));
console.log(holder.f(...numbers), holder.f(numbers));
console.log(viaCallback((...xs) => xs.length));
