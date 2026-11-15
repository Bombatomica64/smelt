"use strict";
// `Promise.race` settles with whichever racer wins, so its result is a promise
// of the union of the racers' types; and a promise handed to `race` or `all`
// can be awaited, raced or collected again afterwards.
async function main(): Promise<void> {
  const value = (async (): Promise<unknown> => "value")();
  const signal = new Promise<void>((resolve) => setTimeout(resolve, 10));
  console.log("race unknown|void", await Promise.race([value, signal]));

  const quick = (async (): Promise<number> => 1)();
  const late = new Promise<string>((resolve) => setTimeout(() => resolve("late"), 10));
  console.log("race number|string", await Promise.race([quick, late]));
  console.log("all after race", await Promise.all([quick, late]));
  console.log("awaited again", await quick, await late);

  const slow = new Promise<number>((resolve) => setTimeout(() => resolve(2), 20));
  const fast = new Promise<number>((resolve) => setTimeout(() => resolve(3), 5));
  console.log("race same type", await Promise.race([slow, fast]));
  console.log("race again", await Promise.race([slow, fast]));
  console.log("all same type", await Promise.all([slow, fast]));
}
main();
