// A derived class with NO explicit constructor takes the BASE constructor's
// parameters — H69.
//
// JavaScript's implicit derived constructor is `constructor(...args) {
// super(...args) }`, and `...args` is not open-ended: TypeScript types a
// `new Derived(..)` call against the BASE constructor's signature, so the arity
// and the parameter types are known at lowering. Smelt used to synthesize one
// `Option<SmeltUnknown>` "super argument" instead, which was wrong three ways
// at once: a two-argument construction stopped compiling (E0061), the base's
// parameter properties and field initializers never ran, and the slot was four
// avoidable-erasure lines per such class. All three are one rule — copy the
// base constructor's parameters and forward them through the same `super(..)`
// lowering an explicit derived constructor uses.
//
// The erased forwarded slot survives only where the base is NOT reproducible
// (a host constructor, an abstract base, a generic one); that is a genuine
// dynamic boundary and `part_7_tests.rs` pins it separately, out of this
// zero-erasure corpus.
//
// Every line below is diffed against Node.

// A base with no constructor of its own: the synthesized base constructor takes
// no parameters, so the derived one takes none either — and the base's FIELD
// INITIALIZER still runs, which the erased slot silently skipped.
class Base {
  label: string = "base";
  describe(): string {
    return `Base(${this.label})`;
  }
}

class Derived extends Base {
  extra: number = 7;
}

const derived = new Derived();
console.log(derived.describe(), derived.label, derived.extra);

// Parameter properties: both arguments arrive, and the base's own constructor
// body is what assigns them.
class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}
  sum(): number {
    return this.x + this.y;
  }
}

class Point3 extends Point {
  z: number = 3;
}

const point = new Point3(1, 2);
console.log(point.sum(), point.x, point.y, point.z);

// An OPTIONAL base parameter stays optional through the forward, so the derived
// class is constructible both ways and the base's default applies.
class Tagged {
  tag: string;
  constructor(tag: string = "none") {
    this.tag = tag;
  }
}

class Labelled extends Tagged {
  seen: boolean = true;
}

console.log(new Labelled().tag, new Labelled("here").tag, new Labelled().seen);

// Two levels: the middle class has no constructor either, so the parameters
// forward the whole way down and each level's initializer still runs once.
class Deeper extends Point3 {
  w: number = 4;
}

const deeper = new Deeper(5, 6);
console.log(deeper.x, deeper.y, deeper.z, deeper.w, deeper.sum());
