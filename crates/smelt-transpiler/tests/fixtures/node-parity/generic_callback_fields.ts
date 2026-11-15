"use strict";
// A generic class's or interface's function-valued field whose parameters are
// callbacks over the type argument: the slot keeps the ABI its declaration
// gives it (`(value: T) => T` takes `T` by reference), at every instantiation,
// whether the field is called directly or through a receiver bind.
class Holder<T> {
  value: T;
  apply: (fn: (value: T) => T) => T;
  pair: (fn: (left: T, right: T) => string) => string;
  constructor(value: T) {
    this.value = value;
    this.apply = (fn) => fn(this.value);
    this.pair = (fn) => fn(this.value, this.value);
  }
}
const numbers = new Holder<number>(7);
console.log("apply number", numbers.apply((n) => n + 1));
console.log("pair number", numbers.pair((a, b) => a + "+" + b));
const words = new Holder<string>("a");
console.log("apply string", words.apply((w) => w + "b"));

// A class with a field that reads its receiver: every member call through a
// field is a receiver bind, which must keep the slot's ABI too.
class Bound<T> {
  value: T;
  show: (this: Bound<T>, label: string) => string;
  apply: (fn: (value: T) => T) => T;
  constructor(value: T) {
    this.value = value;
    this.show = function (this: Bound<T>, label: string) {
      return label + ":" + String(this.value);
    };
    this.apply = (fn) => fn(this.value);
  }
}
const bound = new Bound<number>(10);
console.log("bound show", bound.show("held"));
console.log("bound apply", bound.apply((n) => n * 2));

interface Box<T> {
  read: (reader: (value: T) => string) => string;
}
function boxOf<T>(value: T): Box<T> {
  return { read: (reader) => reader(value) };
}
console.log("interface", boxOf(3).read((n) => "got " + n));
