// `Symbol.for` interns by description at run time, `Symbol.keyFor` reads it back.
function registered(name: string): symbol {
  return Symbol.for(name);
}
const a = registered("app." + "key");
const b = Symbol.for("app.key");
console.log(a === b, String(a), a.description);
console.log(Symbol.keyFor(a), Symbol.keyFor(Symbol("app.key")), Symbol.keyFor(Symbol.iterator));
const parts = ["x", "y", "x"];
const syms = parts.map((p) => Symbol.for(p));
console.log(syms[0] === syms[2], syms[0] === syms[1]);
const forValue = Symbol.for;
const keyForValue = Symbol.keyFor;
console.log(forValue("app.key") === b, keyForValue(forValue("z")));
const obj: Record<symbol, number> = {};
obj[registered("slot")] = 7;
console.log(obj[Symbol.for("slot")]);
console.log(String(Symbol.for("")), Symbol.keyFor(Symbol.for("")) === "");
// `Symbol(description)` with a run-time description keeps it, and stays unique.
function tagged(label: string): symbol {
  return Symbol(label);
}
const t1 = tagged("t" + 1);
const t2 = tagged("t" + 1);
console.log(t1 === t2, t1.description, String(t2), Symbol.keyFor(t1));
