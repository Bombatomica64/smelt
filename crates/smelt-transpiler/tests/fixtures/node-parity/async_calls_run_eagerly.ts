// Calling an async function runs its body synchronously up to its first
// `await`, whether the promise is bound, awaited later or never awaited; only
// the rest runs as jobs, in the order the awaited promises settle.
async function step(name: string): Promise<number> {
  console.log(name + " start");
  await null;
  console.log(name + " resume");
  return name.length;
}
async function a(): Promise<number> {
  console.log("a start");
  return 1;
}
async function b(): Promise<number> {
  console.log("b start");
  await a();
  console.log("b after");
  return 2;
}
async function main(): Promise<void> {
  const first = step("x");
  const second = step("yy");
  console.log("sync in main");
  const total = (await first) + (await second);
  console.log("total", total);
}
const p = b();
p.then((v) => console.log("then", v));
new Promise<void>((resolve) => {
  console.log("executor");
  resolve();
});
step("floating");
main().then(() => console.log("main done"));
console.log("end of module");
