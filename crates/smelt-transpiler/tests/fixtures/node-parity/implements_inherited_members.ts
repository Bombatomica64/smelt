// `implements` is satisfied by what the class inherits.
interface R { go(): string; size: number; }
class A { size = 3; go(): string { return "a"; } }
class B extends A implements R { extra(): string { return "b" + this.go(); } }
const r: R = new B();
console.log(r.go(), r.size, new B().extra());
class C extends B {}
const c: R = new C();
console.log(c.go(), c.size);
interface Named { label?: string; name: string; }
class Base2 { name = "n"; }
class D extends Base2 implements Named {}
console.log(new D().name);
class MyErr extends Error implements Pick<Error, "message"> {}
console.log(new MyErr("m").message);
class G<T> { v!: T; get(): T { return this.v; } }
interface Getter { get(): number; }
class H extends G<number> implements Getter {}
const h = new H(); h.v = 4;
console.log(h.get());
