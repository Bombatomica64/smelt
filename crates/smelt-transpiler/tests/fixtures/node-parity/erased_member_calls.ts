// A method called on an erased value is what the value holds.
class K { hi(): string { return "hi"; } }
function dyn(o: any): unknown { return o.hi(); }
console.log(dyn(new K()));
class Q { run(): string { return "q"; } }
function viaT<T extends object>(x: T): unknown { return (x as any).run(); }
console.log(viaT(new Q()));
const rec: any = { f: () => "ok", n: 1 };
console.log(rec.f());
class S2 { greet(): unknown { return (this as any).name2(); } name2(): string { return "s2"; } }
console.log(new S2().greet());
