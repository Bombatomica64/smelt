// `const Foo = class { … }` declares a class named `Foo`. That is the name
// TypeScript infers, and every nominal use of the binding depends on it:
// `new Foo()`, `Foo` in type position, `x instanceof Foo`, `extends Foo`.
//
// Lowered as a plain value instead, the binding held a placeholder and
// `new Foo()` erased into a dynamic construction: its methods answered `null`
// and its fields `undefined`, with no diagnostic (H68).
class Counter {
  #hits: string[] = [];

  add(key: string): void {
    this.#hits.push(key);
  }

  protected tally(): string {
    return this.#hits.join(',');
  }
}

// A class expression that EXTENDS: the subclass is named by its binding and
// calls a method it inherits.
const Reporting = class extends Counter {
  constructor() {
    super();
  }

  report(): string {
    return this.tally();
  }
};

const reporting = new Reporting();
reporting.add('a');
reporting.add('b');
console.log('expression subclass: ' + reporting.report());

// The declared form of the same class, which always worked: the control.
class DeclaredReporting extends Counter {
  constructor() {
    super();
  }

  report(): string {
    return this.tally();
  }
}

const declared = new DeclaredReporting();
declared.add('c');
console.log('declared subclass: ' + declared.report());

// A GENERIC class expression, constructed with an explicit type argument.
const Boxed = class<T> {
  items: T[] = [];

  push(item: T): number {
    this.items.push(item);
    return this.items.length;
  }
};

const numbers = new Boxed<number>();
console.log('generic: ' + String(numbers.push(7)) + ' ' + String(numbers.items[0]));
console.log('instanceof: ' + (numbers instanceof Boxed ? 'yes' : 'no'));

// A class REFERENCE is a function value, whichever way the class was written;
// an instance of it is an object.
console.log('typeof expression class: ' + typeof Boxed);
console.log('typeof declared class: ' + typeof DeclaredReporting);
console.log('typeof instance: ' + typeof numbers);
