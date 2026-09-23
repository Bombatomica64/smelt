class Bag<T> {
  items: T[] = [];
  add(x: T): void {
    this.items.push(x);
  }
}
function f(x: unknown): void {
  console.log(x === "a");
}
f("a");
const u = new Bag<unknown>();
u.add("z");
const b = new Bag();
b.add("a");
