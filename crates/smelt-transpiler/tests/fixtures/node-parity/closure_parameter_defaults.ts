const g = (x: number, f: (n: number) => number = (n) => n + 1) => f(x);
console.log(g(1), g(1, (n) => n * 5));
const out = 10;
const cap = (a: number, b: number = out + a) => a + b;
console.log(cap(1), cap(1, 2));
function inner() {
  const local = 7;
  const h = (s: string, t: string = s + '!', u: number = local) => s + t + u;
  console.log(h('a'), h('a', 'b'), h('a', 'b', 1));
  const z = (a = 5, b = a * 2) => a + b;
  console.log(z(), z(1), z(1, 1));
}
inner();
console.log([1, undefined, 3].map((x = 9) => x));
console.log([{ a: 1 }].map(({ a }, i = 3) => a + i));
const un: Array<number | undefined> = [undefined, 2];
un.forEach((v = 4, i) => console.log(v, i));
const nest = (a: number, cb = (x: number) => x * a) => cb(2);
console.log(nest(3));
