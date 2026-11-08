// A class read as a value keeps its static methods as own properties, so a
// call through a computed key finds them; a block-local class used as a value
// constructs that class.
const key = Symbol("matcher");

class Counter {
  static [key](): number {
    return 7;
  }
  static named(): string {
    return "named";
  }
}

const byKey = Counter as unknown as Record<symbol, () => number>;
console.log(byKey[key]());
const byName = Counter as unknown as Record<string, () => string>;
console.log(byName["named"]());

function describe(name: string, body: () => void): void {
  console.log(name);
  body();
}

describe("suite", () => {
  class None {
    tag: string;
    constructor() {
      this.tag = "none";
    }
  }
  const box: { option: unknown } = { option: None };
  const make = box.option as new () => None;
  console.log(new make().tag);
});

describe("other suite", () => {
  class None {
    count: number;
    constructor() {
      this.count = 2;
    }
  }
  const box: { option: unknown } = { option: None };
  const make = box.option as new () => None;
  console.log(new make().count);
});
