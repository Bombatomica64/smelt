"use strict";
// A continuation released by a timer runs before the clock reaches the next
// timer: Node drains the microtask queue between timer phases. Inside a
// `Promise.race`, work due at 900 ms therefore beats a 1000 ms timeout, typed
// or reached through an erased value.
const order: string[] = [];
async function work(label: string): Promise<string> {
  await new Promise((resolve) => setTimeout(resolve, 900));
  order.push(label + " resumed");
  return label;
}
const host: any = { work };
function timeout(label: string): Promise<string> {
  return new Promise<string>((_, reject) => {
    setTimeout(() => {
      order.push(label + " timeout fired");
      reject(new Error(label + " timed out"));
    }, 1000);
  });
}
async function race(label: string, run: () => Promise<string>): Promise<void> {
  try {
    const winner = await Promise.race([run(), timeout(label)]);
    order.push("winner " + winner);
  } catch (error) {
    order.push((error as Error).message);
  }
}
async function main(): Promise<void> {
  await race("typed", () => work("typed"));
  await race("erased", () => host.work("erased"));
  setTimeout(() => console.log(order.join(" | ")), 2000);
}
main();
