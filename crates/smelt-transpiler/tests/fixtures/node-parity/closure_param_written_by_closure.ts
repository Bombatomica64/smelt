// A closure parameter a nested closure writes is shared with it.
function make(p: number): () => () => number {
  return () => { const inner = () => { p = p + 1; return p; }; inner(); return inner; };
}
const f = make(1)();
console.log(f(), f());
const g = (n: number): number => { const add = () => { n += 2; }; add(); add(); return n; };
console.log(g(1));
function counter(start: number) { const inc = () => { start++; return start; }; inc(); return () => start + inc(); }
console.log(counter(1)());
const nested = (a: string): string[] => { const out: string[] = []; const one = () => { const two = () => { a = a + "!"; out.push(a); }; two(); }; one(); one(); return out; };
console.log(nested("x"));
[1, 2].forEach((n) => { const bump = () => { n *= 3; }; bump(); console.log(n); });
