// `Promise.allSettled` waits for every input and answers one record per
// input, fulfilled or rejected; a rejection is an entry, not unhandled.
async function fail(msg: string): Promise<number> { throw new Error(msg); }
async function ok(n: number): Promise<number> { return n * 2; }
function show(r: PromiseSettledResult<unknown>): string {
  if (r.status === "fulfilled") return "ok " + String(r.value);
  return "no " + (r.reason instanceof Error ? r.reason.message : JSON.stringify(r.reason));
}
async function main(): Promise<void> {
  const [a, b, c, d] = await Promise.allSettled([ok(1), fail("boom"), Promise.resolve("s"), Promise.reject({ code: 7 })]);
  console.log(show(a), "|", show(b), "|", show(c), "|", show(d));
  if (a.status === "fulfilled") console.log(a.value + 1);
  const list: Promise<number>[] = [ok(3), fail("x"), ok(4)];
  const settled = await Promise.allSettled(list);
  console.log(settled.length, settled.map((r) => r.status).join(","));
  console.log(JSON.stringify(settled.filter((r) => r.status === "fulfilled")));
  for (const r of settled) console.log(show(r));
}
main().then(() => console.log("done"));
