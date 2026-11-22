// A typed function's `name` in a program that never needs the erased runtime:
// the read is a compile-time string, not a dynamic property lookup.
function hello(a: number): number {
  return a;
}
const twice = (x: number): number => hello(x) * 2;
console.log(hello(1), twice(2));
console.log(hello.name, twice.name);
