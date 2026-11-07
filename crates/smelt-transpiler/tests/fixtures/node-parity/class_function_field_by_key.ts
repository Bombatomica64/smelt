class Handlers {
  scale = 10;
  add = (a: number, b: number): number => a + b;
  label: (name: string) => string;
  constructor() {
    this.label = (name) => "<" + name + ">";
  }
  twice(value: number): number {
    return value * 2;
  }
  scaled(value: number): number {
    return value * this.scale;
  }
}
type Op = "twice" | "scaled";
function viaField(h: Handlers, key: "add"): number {
  return h[key](1, 2);
}
function viaAssigned(h: Handlers, key: "label", name: string): string {
  return h[key](name);
}
function viaMethod(h: Handlers, key: Op, value: number): number {
  return h[key](value);
}
function viaAny(h: Handlers, key: string, value: number): number {
  return (h as any)[key](value);
}
const h = new Handlers();
console.log(viaField(h, "add"), viaAssigned(h, "label", "x"));
console.log(viaMethod(h, "twice", 4), viaMethod(h, "scaled", 4));
console.log(viaAny(h, "twice", 5), viaAny(h, "scaled", 5));
const pick = h["twice"];
console.log(typeof pick, typeof h["add"], typeof h["scale"]);
