// `Object.fromEntries` takes any iterable of entries.
const fm = new Map<string, number>([["a", 1], ["b", 2]]);
console.log(JSON.stringify(Object.fromEntries(fm.entries())));
console.log(JSON.stringify(Object.fromEntries(fm)));
function* pairs(): Generator<[string, number]> { yield ["x", 1]; yield ["y", 2]; }
const fromGen = Object.fromEntries(pairs());
console.log(JSON.stringify(fromGen), fromGen.x + fromGen.y);
console.log(JSON.stringify(Object.fromEntries(new Set<[string, number]>([["s", 9]]))));
const list: Array<[string, string]> = [["k", "v"], ["k", "w"]];
console.log(JSON.stringify(Object.fromEntries(list)));
console.log(JSON.stringify(Object.fromEntries(Object.entries({ p: true, q: false }).filter(([, v]) => v))));
