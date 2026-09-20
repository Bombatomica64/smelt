// A generic class whose stored callback produces the class itself — round 31,
// Agent E item 2.
//
// The emitted `Default` impl for a generic reference class's inner record used
// the bound set a `#[derive(Default)]` would have written (`T: Default`) plus a
// `where` clause naming every field type whose default DELEGATES. Both halves
// were wrong for a callable field whose return type is another generated class:
//
//   * the field's default is a constructed no-op closure whose BODY delegates
//     (`Rc::new(move |..| -> Chain<T> { Default::default() })`), so the textual
//     search saw a delegation and demanded `Rc<dyn Fn(..)>: Default` — a bound
//     nothing implements, which made the whole impl unusable;
//   * the delegation inside that body needs `Chain<T>: Default`, whose own impl
//     carries the crate's full generated bound set, and `T: Default` alone
//     cannot prove it.
//
// The rule: every generic item the crate emits carries the SAME bound set for
// its type parameters, and a callable slot is never a delegation of its own
// field type.
//
// Every line below is diffed against Node.

class Chain<T> {
  value: T;
  history: T[];
  // A callable field whose return type is the enclosing generic class. This is
  // the shape whose default closure body delegates to `Chain<T>::default()`.
  rewind: () => Chain<T>;
  constructor(value: T) {
    this.value = value;
    this.history = [value];
    this.rewind = () => {
      this.value = this.history[0];
      return this;
    };
  }
  push(next: T): Chain<T> {
    this.history.push(next);
    this.value = next;
    return this;
  }
}

// A second generic class that STORES the first, so its own `Default` body
// delegates to `Chain<T>::default()` through a field type rather than through a
// closure body. The two paths reach the same bound set.
class Track<T> {
  chain: Chain<T>;
  constructor(chain: Chain<T>) {
    this.chain = chain;
  }
  latest(): T {
    return this.chain.value;
  }
}

const numbers = new Chain<number>(1);
numbers.push(2);
numbers.push(3);
console.log(numbers.value);
console.log(numbers.history.join(","));
console.log(numbers.rewind().value);

const words = new Track<string>(new Chain<string>("a"));
words.chain.push("b");
console.log(words.latest());
console.log(words.chain.rewind().value);
