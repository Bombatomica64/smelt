type Erased = (...args: unknown[]) => unknown;

const count: Erased = (...args) => args.length;
const join: Erased = (...args) => args.map(String).join("-");

function forward<T>(fn: Erased, values: T[]): unknown {
  return fn.apply(null, values);
}

function first(fn: Erased, value: unknown): unknown {
  return fn.call(null, value);
}

function each(fns: Erased[], value: unknown): unknown[] {
  return fns.map((fn) => fn(value, value));
}

let tick = 0;
function next(): number {
  tick += 1;
  return tick;
}

const nothing = count();
console.log(nothing, count(1), count(1, "a", [2]));
console.log(join(next(), next(), next()));
console.log(forward(join, [1, 2, 3]), forward(count, ["a"]), forward(count, []));
console.log(first(count, [1, 2]), first(join, "x"));
console.log(each([count, join], 7));
const list = [1, 2];
console.log(join(0, ...list), count(list, ...list), count(...list));
