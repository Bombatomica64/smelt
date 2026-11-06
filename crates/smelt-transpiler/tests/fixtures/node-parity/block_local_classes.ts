// Same-named classes in different scopes stay different classes.
function clone<T>(x: T): T { return x; }
function a(): string { class Data { v = "a"; who(): string { return this.v; } } return clone(new Data()).who(); }
function b(): number { class Data { n = 2; twice(): number { return this.n * 2; } } return clone(new Data()).twice(); }
console.log(a(), b());
function c(): string[] { class Data { tag = "s"; k(): string { return this.tag; } } class E { } return [new Data().k(), typeof new E()]; }
console.log(c());
{ class Blk { x = 1; } console.log(new Blk().x); }
{ class Blk { y = "2"; } console.log(new Blk().y); }
class Data { top = 1; }
function d(): number { class Data { inner = 2; } return new Data().inner; }
function e(): number { return new Data().top; }
console.log(d(), e(), new Data().top);
const x1 = (() => { class Tmp { a = 1; } return new Tmp().a; })();
const x2 = (() => { class Tmp { b = 2; } return new Tmp().b; })();
console.log(x1, x2);
function t1(): boolean { class Kind { a = 1; } return new Kind() instanceof Kind; }
function t2(): boolean[] { class Kind { b = 2; } const k: unknown = new Kind(); return [k instanceof Kind, new Kind() instanceof Kind]; }
console.log(t1(), t2());
function thrower(): string { class Oops extends Error {} try { throw new Oops("x"); } catch (e) { return e instanceof Oops ? "yes" : "no"; } }
function thrower2(): string { class Oops extends Error {} try { throw new Error("y"); } catch (e) { return e instanceof Oops ? "yes" : "no"; } }
console.log(thrower(), thrower2());
