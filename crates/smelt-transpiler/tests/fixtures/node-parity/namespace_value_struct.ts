// `import * as NS` used as a value is a plain struct of the module's value
// exports: passed on, typed by `typeof NS`, enumerated by `Object.keys` in the
// namespace's key order (sorted export names, types excluded), and indexed by
// a literal-union key.
import * as Shapes from "./shapes.ts";
import * as Ops from "./ops.ts";

console.log(Shapes.unit, Shapes.label, Shapes.area(3));
function describe(ns: typeof Shapes): string {
  return `${ns.label}:${ns.unit}:${ns.area(2)}`;
}
console.log(describe(Shapes));
console.log(Object.keys(Shapes).join(","), Object.keys(Ops).length);
const held = Shapes;
console.log(held.label, Object.keys(held));

type OpName = "add" | "mul" | "sub";
function apply(name: OpName, a: number, b: number): number {
  return Ops[name](a, b);
}
console.log(apply("add", 2, 3), apply("mul", 2, 3), apply("sub", 2, 3));
const names: OpName[] = ["sub", "add"];
console.log(names.map((name) => Ops[name](10, 4)).join(","));
const table = Ops;
console.log(table.add(1, 1), table.mul(table.sub(5, 2), 4));
