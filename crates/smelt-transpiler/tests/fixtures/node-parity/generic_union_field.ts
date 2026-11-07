type Box<T> = { value: T | boolean; label: string };
function make<T>(value: T): Box<T> { return { value, label: "x" }; }
const a = make(5);
const b = make<string>("s");
console.log(a.value, b.value, a.label);
function read<T>(box: Box<T>): T | boolean { return box.value; }
console.log(read(a), read({ value: true, label: "y" }));
const c: Box<number> = { value: 1, label: "y" };
const d: Box<number> = { value: false, label: "z" };
const v = c.value;
if (typeof v === "number") { console.log(v + 1); }
console.log(d.value === false, read(c));
class Cell<T> { held: T | boolean; constructor(v: T) { this.held = v; } get(): T | boolean { return this.held; } }
console.log(new Cell(3).get(), new Cell("q").held);
