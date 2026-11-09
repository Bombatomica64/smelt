// A keyed write through an erased view reaches the instance, and an implementation
// whose overloads return an array or an object answers with what it built.
class Data {
  val = 0;
  label = "x";
}
const obj = new Data();
obj.val = 1;
obj.label = "kept";
function clone<T>(o: T): T {
  const n = new ((o as object).constructor as { new (): T })();
  Object.getOwnPropertyNames(o).forEach((p) => {
    (n as any)[p] = (o as any)[p];
  });
  return n;
}
const copy = clone(obj);
console.log(copy.val, copy.label, copy === obj, copy instanceof Data);

function pair<T extends number[]>(input: T): number[];
function pair<T extends Record<string, number>>(input: T): Record<string, number>;
function pair<T extends Record<string, number> | number[]>(input: T) {
  if (Array.isArray(input)) {
    return input.map((n) => n * 2);
  }
  return Object.fromEntries(Object.entries(input).map(([k, v]) => [k, v * 2]));
}
console.log(pair([1, 2]), pair({ a: 2 }));
