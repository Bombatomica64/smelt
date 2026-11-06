// Arguments past the positional parameters land in the rest list, on every kind of
// callee: a local, a field, a parameter, through `try`, with leading positionals.

const f = (a: number, ...r: number[]): number => a * 100 + r.length;
console.log(f(1), f(1, 2), f(1, 2, 3));
const g = (...r: string[]): string => r.join("+");
console.log(g(), g("a"), g("a", "b"));
const u = (...r: unknown[]): number => r.length;
console.log(u(), u(1), u(1, "x"), u([1, 2]));
function outer(cb: (...xs: number[]) => number): number { return cb(1, 2, 3); }
console.log(outer((...xs) => xs.length));
const lists = (...ls: number[][]): number => ls.length;
console.log(lists([1]), lists([1], [2]));
class K { cb: (...xs: string[]) => number = (...xs) => xs.length; }
console.log(new K().cb("a", "b"));
const opt: ((...xs: number[]) => number) | undefined = (...xs) => xs.length;
console.log(opt?.(1, 2));
class L { cb: (a: number, ...xs: string[]) => number = (a, ...xs) => a * 10 + xs.length; }
console.log(new L().cb(1), new L().cb(1, "a"), new L().cb(1, "a", "b"));
const sum = (...xs: number[]): number => xs.reduce((a, b) => a + b, 0);
const holder = { run: sum };
console.log(holder.run(1, 2, 3), holder.run(), holder.run(4));
const xs3 = [1, 2];
console.log(holder.run(...xs3), holder.run(0, ...xs3));
function viaTry(k: L): number {
  try { return k.cb(2, "a", "b"); } catch (e) { return -1; }
}
console.log(viaTry(new L()));
function throwing(cb: (a: number, ...xs: string[]) => number): number {
  try { return cb(3, "p", "q", "r"); } catch (e) { return -1; }
}
console.log(throwing((a, ...xs) => { if (a < 0) { throw new Error("no"); } return a + xs.length; }));
function spreadInto<T, R>(rows: T[][], combine: (...args: T[]) => R): R[] {
  const out: R[] = [];
  for (let i = 0; i < 2; i++) {
    const group = new Array(rows.length);
    for (let j = 0; j < rows.length; j++) {
      group[j] = rows[j][i];
    }
    out.push(combine(...group));
  }
  return out;
}
console.log(JSON.stringify(spreadInto([[1, 2], [3, 4], [5, 6]], (a, b, c) => a + b + c)));
const pick = (data: number, keep: (value: number) => boolean, ...rest: number[]): number =>
  keep(data) ? data : (rest[0] ?? -1);
function forward(...args: unknown[]): unknown {
  // @ts-expect-error: the arguments are erased on purpose.
  return pick(...args);
}
console.log(forward(1, (v: number) => v > 0), forward(1, (v: number) => v < 0, 7));
