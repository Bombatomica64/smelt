// A class's type-parameter scope ends with the class. A generic class whose
// method is reassigned through `this` used to leave its `T` in scope, so the
// later `type T = number` alias resolved to that stale parameter and `twice`
// lowered as a generic `fn(T) -> T` instead of `fn(f64) -> f64`.

class Holder<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
    this.read = () => this.value;
  }
  read(): T {
    return this.value;
  }
}

type T = number;

function twice(v: T): T {
  return v * 2;
}

console.log(new Holder<string>("x").read());
console.log(twice(21));
