// A method argument bound to a bare class type parameter is coerced to the
// type the RECEIVER pins, not rendered at its own type.
//
// `new Bag()` has no argument to infer `T` from, so TypeScript instantiates it
// at `unknown` and the local is `Bag<SmeltUnknown>`. The method's `item: T` is
// then `SmeltUnknown`, and a `String` argument passed through unconverted was
// `expected SmeltUnknown, found String` — 238 errors in Hono's trie-router
// tests (`node.insert('get', '/', 'get root')` on a `new Node()`). A receiver
// that pins a concrete type (`Bag<number>`) keeps the argument concrete.
//
// Every line below is diffed against Node.

class Bag<T> {
  items: T[] = [];
  add(label: string, item: T): void {
    this.items.push(item);
  }
  size(): number {
    return this.items.length;
  }
}

const loose = new Bag();
loose.add('a', 'first');
loose.add('b', 2);
console.log(loose.size());

const numbers = new Bag<number>();
numbers.add('x', 40);
numbers.add('y', 2);
console.log(numbers.size(), numbers.items[0] + numbers.items[1]);
