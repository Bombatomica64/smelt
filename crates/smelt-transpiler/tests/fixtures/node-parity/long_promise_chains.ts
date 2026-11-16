// A `.then` chain of any length runs to its end, and every job queued (even
// by jobs) runs before the first timer fires, as in Node.
let p: Promise<number> = Promise.resolve(0);
for (let i = 0; i < 500; i++) p = p.then((x) => x + 1);
p.then((v) => console.log("chain", v));
setTimeout(() => console.log("timeout"), 0);
let n = 0;
function spin(): void {
  n++;
  if (n < 300) Promise.resolve().then(spin);
  else console.log("spun", n);
}
Promise.resolve().then(spin);
async function deep(k: number): Promise<number> {
  if (k === 0) return 0;
  return 1 + (await deep(k - 1));
}
deep(200).then((d) => console.log("deep", d));
