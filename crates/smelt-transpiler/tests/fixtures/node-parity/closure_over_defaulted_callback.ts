// An arrow that escapes in a returned object and captures a callback parameter
// keeps its body, including when the parameter carries a default and when a
// body-local shares the parameter's spelling.
function series<T>(items: T[], toKey: (item: T) => string = (item) => `${item}`) {
  const index: Record<string, number> = {};
  items.forEach((item, i) => {
    index[toKey(item)] = i;
  });
  const min = (a: T, b: T): T => (index[toKey(a)] < index[toKey(b)] ? a : b);
  const max = (a: T, b: T): T => (index[toKey(a)] > index[toKey(b)] ? a : b);
  return { min, max };
}

const days = series(["mon", "tue", "wed"]);
console.log(days.min("tue", "mon"), days.max("tue", "wed"));
const lens = series([1, 22, 333], (n) => `#${n}`);
console.log(lens.min(333, 22), lens.max(1, 22));

// A required callback parameter captured by an escaping closure.
function wrap(cb: (n: number) => number): () => number {
  const run = () => cb(20) + 1;
  return run;
}
console.log(wrap((n) => n * 2)());

// A required callback captured by a closure that does not escape, then called twice.
function twice(cb: (n: number) => number): number {
  const run = () => cb(3);
  return run() + run() + cb(1);
}
console.log(twice((n) => n + 1));

// A closure created inside a closure, returned out of the outer call.
function nested(cb: () => number): number {
  const outer = () => {
    return () => cb();
  };
  return 1 + outer()();
}
console.log(nested(() => 41));

// A closure stored in an erased object.
function stash(cb: () => string): Record<string, unknown> {
  const bag: Record<string, unknown> = {};
  bag.run = () => cb();
  return bag;
}
console.log((stash(() => "kept").run as () => string)());
