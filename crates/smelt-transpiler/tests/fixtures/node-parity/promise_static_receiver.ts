// The `Promise` statics read `this` as the constructor they build with: a
// call through a copy that supplies no receiver throws Node's `TypeError`,
// and one that supplies `Promise` (`.call`, `.bind`) works.
const resolve = Promise.resolve;
const reject = Promise.reject;
const all = Promise.all;
const unbound: Array<[string, () => unknown]> = [
  ["resolve", () => resolve(1)],
  ["reject", () => reject(new Error("no"))],
  ["all", () => all([1, 2])],
];
for (const [name, call] of unbound) {
  try {
    call();
    console.log(name, "did not throw");
  } catch (error) {
    console.log(name, error instanceof TypeError, (error as Error).message);
  }
}
async function main(): Promise<void> {
  console.log(await resolve.call(Promise, 5));
  const bound = all.bind(Promise);
  console.log((await bound([6, Promise.resolve(7)])).join(","));
  try {
    await reject.call(Promise, new Error("rejected"));
  } catch (error) {
    console.log((error as Error).message);
  }
}
main();
