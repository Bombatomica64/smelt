// `typeof` of an optional is decided at run time, and a `case "object"` arm keeps a
// dynamic union member, so `Array.isArray` inside it still asks the value.

function kind(value?: symbol | number | string | object | null): string {
  switch (typeof value) {
    case "object": {
      if (Array.isArray(value) && value.length === 2) {
        return "pair";
      }
      return "obj";
    }
    default: return "other";
  }
}
console.log(kind(["a", 1]), kind({}), kind(1));
function k2(value: object): string { return Array.isArray(value) ? "list" : "obj"; }
console.log(k2([1]), k2({}));
function t(value?: string | null): string { return typeof value; }
console.log(t(), t("a"));
function u(value?: number): string { return typeof value === "number" ? "n" : "none"; }
console.log(u(), u(1));
function o(value?: { a: number }): string { return typeof value; }
console.log(o(), o({ a: 1 }));
function f(value?: () => void): string { return typeof value; }
console.log(f(), f(() => {}));
