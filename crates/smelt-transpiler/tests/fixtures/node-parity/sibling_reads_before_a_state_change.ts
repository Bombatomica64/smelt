// A read of an object's state is taken where it is evaluated, before a later
// argument that changes that state: a runtime-modelled op moving a RegExp's
// `lastIndex`, or a method writing its own field.
const sticky = /b/y;
sticky.lastIndex = 1;
console.log(sticky.test("abc"), sticky.lastIndex, sticky.test("abc"), sticky.lastIndex);

const global = /a/g;
console.log(global.lastIndex, global.test("aa"), global.lastIndex, global.exec("aa") !== null, global.lastIndex);

class Counter {
  n = 0;
  bump(): number {
    this.n += 1;
    return this.n * 10;
  }
}
const counter = new Counter();
console.log(counter.n, counter.bump(), counter.n, counter.bump(), counter.n);
