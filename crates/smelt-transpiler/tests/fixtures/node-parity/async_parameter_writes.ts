// An async function (and a generator) writing through a collection or class
// parameter writes the caller's object, as JavaScript passes objects by
// reference, even while two calls over the same object are pending at once.
class Counter {
  n = 0;
}
async function record(log: string[], m: Map<string, number>, s: Set<number>, r: Record<string, number>, c: Counter, tag: string): Promise<number> {
  log.push(tag + "1");
  m.set(tag, 1);
  s.add(tag.length);
  r[tag] = 2;
  c.n += 1;
  await null;
  log.push(tag + "2");
  m.set(tag + "!", 3);
  s.add(10 + tag.length);
  r[tag + "!"] = 4;
  c.n += 10;
  return log.length;
}
function* walk(log: string[]): Generator<number> {
  log.push("g1");
  yield 1;
  log.push("g2");
}
async function main(): Promise<void> {
  const log: string[] = [];
  const m = new Map<string, number>();
  const s = new Set<number>();
  const r: Record<string, number> = {};
  const c = new Counter();
  const first = record(log, m, s, r, c, "a");
  const second = record(log, m, s, r, c, "bb");
  log.push("sync");
  const sizes = [await first, await second];
  for (const step of walk(log)) log.push("y" + String(step));
  console.log(sizes.length, [...log].sort().join(","));
  console.log(JSON.stringify([...m.keys()].sort()), [...s].sort((x, y) => x - y).join(","));
  console.log(JSON.stringify(Object.keys(r).sort()), c.n);
}
main();
