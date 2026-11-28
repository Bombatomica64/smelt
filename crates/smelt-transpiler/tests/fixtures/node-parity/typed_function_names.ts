// A typed function value reports the name it was created with wherever it
// travels: a class field reassigned to another function, a parameter, a list
// element, a record entry, a reassigned local; an anonymous one reports "".
function other(x: number) { return x * 2; }
function first(x: number) { return x + 1; }
class B { g: (x: number) => number = first; }
const b = new B();
console.log(b.g.name);
b.g = other;
console.log(b.g.name, b.g(3));
function show(f: (x: number) => number) { return f.name; }
console.log(show(other), show(first), show((x) => x));
const fs: Array<(x: number) => number> = [first, other];
console.log(fs.map((f) => f.name).join(","));
const o = { h: first };
o.h = other;
console.log(o.h.name);
let v = first;
v = other;
console.log(v.name);
const named = function inner(x: number) { return x; };
const arrow = (x: number) => x;
console.log(show(named), show(arrow));
