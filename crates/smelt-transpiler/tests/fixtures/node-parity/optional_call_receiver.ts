// An optional call through a field that may hold no function runs the
// function with its receiver as `this` when present, and is `undefined` when
// absent.
interface Api { name: string; greet?: () => string }
const api: Api = { name: "bob", greet() { return "hi " + this.name; } };
console.log(api.greet?.());
type Named = { name: string; greet?(punct: string): string };
const ann: Named = { name: "ann", greet(punct: string) { return "hi " + this.name + punct; } };
console.log(ann.greet?.("!"));
function maybe(present: boolean): Named | undefined { return present ? ann : undefined; }
console.log(maybe(true)?.greet?.("?"));
console.log(maybe(false)?.greet?.("?"));
const bare: Named = { name: "x" };
console.log(bare.greet?.("."));
type Counter = { v: number; get(): number };
const counter: Counter = { v: 3, get() { return this.v; } };
const holder = { inner: counter as Counter | undefined };
console.log(holder.inner?.get());
const empty = { inner: undefined as Counter | undefined };
console.log(empty.inner?.get());
const total: number = (holder.inner?.get() ?? 0) + 1;
console.log(total);
