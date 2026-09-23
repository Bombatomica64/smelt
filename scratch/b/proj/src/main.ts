class Bag<T> {
  items: T[] = [];
  add(x: T): void {
    this.items.push(x);
  }
  all(): T[] {
    return this.items;
  }
}
type P = { name: string; age: number };
const b = new Bag<string>();
b.add("a");
b.add("b");
console.log(JSON.stringify(b.all()));
const p = new Bag<P>();
p.add({ name: "x", age: 1 });
console.log(JSON.stringify(p.all()));
