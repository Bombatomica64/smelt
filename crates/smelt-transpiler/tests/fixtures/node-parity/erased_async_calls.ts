"use strict";
// A promise reached through an erased value settles and runs its
// continuations: an async method of an `any` object literal, an async arrow, a
// function returning a promise, an async class method behind an `any`
// receiver and an async function expression.
const asyncHost: any = {
  async run() {
    return 2;
  },
};
asyncHost.run().then((value: unknown) => console.log("async method", value));
const arrowHost: any = { run: async () => 4 };
arrowHost.run().then((value: unknown) => console.log("async arrow", value));
function later(): Promise<number> {
  const make = async () => 5;
  return make();
}
const laterHost: any = { later };
laterHost.later().then((value: unknown) => console.log("returned promise", value));
class Service {
  async load(): Promise<string> {
    return "loaded";
  }
}
const service: any = new Service();
service.load().then((value: unknown) => console.log("class method", value));
const expression: any = async function () {
  return "expression";
};
expression().then((value: unknown) => console.log("function expression", value));
const awaitingHost: any = {
  async total(this: any, a: number, b: number) {
    const left = await Promise.resolve(a);
    return left + b;
  },
};
awaitingHost.total(1, 2).then((value: unknown) => console.log("awaiting method", value));
console.log("sync end");
