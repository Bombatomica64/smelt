// Only a plain `function` (declaration or expression) is a constructor that
// owns a `prototype`; an arrow, an async function and a method have none
// (date-fns `transpose`: `isConstructor(c) ? new c(0) : constructFrom(c, 0)`).
// A builtin constructor reached as a value is a `function` too.
type DateConstructor = new (value: number) => Date;

function isConstructor(candidate: unknown): candidate is DateConstructor {
  return typeof candidate === "function" && candidate.prototype?.constructor === candidate;
}

const arrow = (value: number) => new Date(value);
function declared(this: unknown, value: number) {
  return new Date(value);
}
const expression = function (this: unknown, value: number) {
  return new Date(value);
};
async function later(): Promise<number> {
  return 1;
}
const methods = {
  make(value: number) {
    return new Date(value);
  },
};

console.log(isConstructor(arrow), isConstructor(declared), isConstructor(expression));
console.log(isConstructor(later), isConstructor(methods.make), isConstructor(Date));
console.log(typeof (arrow as unknown as { prototype?: unknown }).prototype, typeof Date, typeof (Date as unknown));
const box: unknown = {};
console.log(typeof box === "object", typeof (Date as unknown) === "object", typeof (Date as unknown) === "function");
const views = [Int8Array, Uint8Array, DataView];
console.log(views.map((view) => typeof view).join(","), views.map((view) => Object.prototype.toString.call(view)).join(","));

// A `function` value narrowed to a typed callback is still that function: it
// keeps the `prototype` it was given (es-toolkit `partial`).
function partial(func: (...args: any[]) => any, ...partialArgs: any[]): (...args: any[]) => any {
  const partialed = function (this: unknown, ...providedArgs: any[]) {
    return func.apply(this, partialArgs.concat(providedArgs));
  };
  if ((func as any).prototype) {
    (partialed as any).prototype = Object.create((func as any).prototype);
  }
  return partialed;
}
const addTen: (value: number) => number = partial((a: number, b: number) => a + b, 10);
console.log(addTen(5), typeof (addTen as any).prototype);

// An inline `function` expression made inside a function, returned or used
// in place, is a constructor too; an inline arrow is not.
function makeConstructor(): (value: number) => Date {
  return function (this: unknown, value: number) {
    return new Date(value);
  };
}
function checkInline(): string {
  const local = function (this: unknown) {
    return 1;
  };
  const arrowLocal = () => 1;
  return [isConstructor(local), isConstructor(arrowLocal)].join(" ");
}
console.log(isConstructor(makeConstructor()), checkInline());
