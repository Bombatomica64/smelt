// A parameter the call passes no argument for is `undefined`, never `null`:
// an erased parameter can tell the two apart, through a plain function, a
// constructor, a static method and a tuple rest element.

function describe(p?: unknown): string {
  return `${p === undefined} ${p === null} ${typeof p} ${String(p)}`;
}

class Holder {
  readonly value: unknown;
  constructor(value?: unknown) {
    this.value = value;
  }
  static show(value?: unknown): string {
    return describe(value);
  }
}

function second(...args: [first?: unknown, second?: unknown]): string {
  return describe(args[1]);
}

console.log(describe());
console.log(describe(null));
console.log(describe(0));
console.log(describe(new Holder().value));
console.log(Holder.show());
console.log(second("a"));
console.log(second("a", null));
