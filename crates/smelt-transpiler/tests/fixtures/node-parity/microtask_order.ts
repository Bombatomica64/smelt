"use strict";
// JavaScript has one FIFO job queue: a reaction on a settled promise is
// queued when it is registered, a reaction on a pending promise when that
// promise settles, at the back. Typed and erased promises share the queue.
const order: string[] = [];
const host: any = {
  async fail(label: string): Promise<never> {
    throw new Error(label);
  },
  async value(label: string): Promise<string> {
    return label;
  },
};

// An erased rejection, a typed resolution, an erased chain of two links and a
// typed chain of two links, registered in that order.
host.fail("erased reject").catch((error: Error) => order.push(error.message));
Promise.resolve("typed resolve").then((value) => order.push(value));
host
  .value("erased chain 1")
  .then((value: string) => {
    order.push(value);
    return "erased chain 2";
  })
  .then((value: string) => order.push(value));
Promise.resolve("typed chain 1")
  .then((value) => {
    order.push(value);
    return "typed chain 2";
  })
  .then((value) => order.push(value));
// A reaction on a pending erased promise runs when it settles, after the
// reactions already queued.
host
  .fail("pending then catch")
  .then(() => order.push("never"))
  .catch((error: Error) => order.push(error.message));
Promise.resolve().then(() => order.push("typed after pending"));
order.push("sync");

setTimeout(() => console.log(order.join(" | ")), 0);
