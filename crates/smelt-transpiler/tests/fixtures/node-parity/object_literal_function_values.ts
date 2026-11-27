// A `function` written as an object-literal property value is a function the
// property holds, like a method shorthand: reading the property gives the
// function, and only a call runs its body. A `get` accessor is the only
// property whose body runs on the read.

interface Api {
  name: string;
  greet(): string;
}

// An explicit `this:` parameter is not a runtime parameter: the function still
// takes no arguments and reads its receiver through `this`.
const api: Api = {
  name: "smelt",
  greet: function (this: Api): string {
    return "hi " + this.name;
  },
};
console.log(api.greet());
api.name = "node";
console.log(api.greet());

// A zero-parameter body that is a single `return` is still a function.
let reads = 0;
const counter = {
  next: function (): number {
    reads += 1;
    return reads;
  },
};
console.log(typeof counter.next, reads);
console.log(counter.next(), counter.next(), reads);

// A function value and a method shorthand build the same kind of property.
const both = {
  viaValue: function (): string { return "value"; },
  viaMethod(): string { return "method"; },
};
console.log(typeof both.viaValue, typeof both.viaMethod);
console.log(both.viaValue(), both.viaMethod());
