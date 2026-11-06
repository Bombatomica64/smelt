// A class in value position is the constructor: `instanceof`, `new`, `.name` and
// `typeof` all read it at run time.
class Oops extends Error {}
class Plain { v = 1; }
const pair = { c: Oops, name: "x" };
const e: unknown = new Oops("a");
console.log(e instanceof pair.c);
console.log(new Error("b") instanceof pair.c);
const table = { p: Plain };
const made = new table.p();
console.log(made.v);
function isIt(x: unknown, k: typeof Plain): boolean { return x instanceof k; }
console.log(isIt(new Plain(), Plain), isIt(1, Plain));
const list = [Plain];
console.log(new Plain() instanceof list[0]);
class Base { hi(): string { return "b"; } }
class Sub extends Base {}
class Sub2 extends Sub { extra = 2; }
const reg = { b: Base };
console.log(new Sub() instanceof reg.b, new Sub2() instanceof reg.b, new Plain() instanceof reg.b);
console.log(reg.b.name, typeof reg.b);
const kinds: unknown[] = [Base, Sub];
console.log(kinds.length, new Sub() instanceof (kinds[1] as typeof Sub), new Base() instanceof (kinds[1] as typeof Sub));
class Arg { n: number; s: string; constructor(n: number, s: string) { this.n = n; this.s = s; } }
const ctors = { a: Arg };
const built = new ctors.a(2, "q");
console.log(built.n, built.s, built instanceof Arg);
function pickClass(flag: boolean) { return flag ? Base : Sub; }
console.log(new Sub() instanceof pickClass(true), new Base() instanceof pickClass(false));
const alias = Base;
console.log(new alias().hi(), new Base() instanceof alias);
