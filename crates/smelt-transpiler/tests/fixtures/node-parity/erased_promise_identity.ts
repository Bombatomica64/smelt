// Erasing a typed promise keeps the promise: it still settles, an erased view
// of one promise is that promise (`===`), and two promises stay two.
async function one(): Promise<number> {
  return 1;
}
async function later(n: number): Promise<number> {
  await null;
  return n * 10;
}
function erase(): unknown {
  const p = one();
  return p;
}
const a = erase();
const b = erase();
console.log("distinct", a === b, a instanceof Promise);
async function main(): Promise<void> {
  const p = later(4);
  const u: unknown = p;
  const v: unknown = p;
  console.log("same promise", u === v);
  const back = await (u as Promise<number>);
  console.log("awaited erased", back, await p);
  const pending = later(7);
  const erased: unknown = pending;
  await (erased as Promise<number>).then((value) => console.log("then on erased", value));
  console.log("typed after erased", await pending);
  const racing = later(9);
  const view: unknown = racing;
  (view as Promise<number>).then((value) => console.log("reaction first", value));
  console.log("await second", await racing);
}
main().then(() => console.log("done"));
