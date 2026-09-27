// A derived constructor's `super(..)` runs the base constructor ON THE DERIVED
// INSTANCE — round 8 of blocker-logs/hono-phase3-round5.md.
//
// JavaScript has one object. The base constructor's field initializers and body
// initialise the instance the derived constructor is building, so:
//
// * a closure the base constructor stores captures the DERIVED instance as
//   `this`, and sees every write the derived constructor makes after `super()`;
// * a field the derived class redeclares is written AFTER the base's
//   initialization (derived field initializers run when `super()` returns);
// * a method the base constructor calls on `this` runs against the derived
//   instance;
// * a GENERIC base runs too — Hono's `class Hono<E> extends HonoBase<E>` is the
//   shape, and its base constructor is where `get`/`use`/`request` are built.
//
// Smelt used to construct a separate base value and copy its fields over, so
// the first case printed `base::x`, the second `x`, and a generic base's
// constructor never ran at all (its closures stayed defaults).
//
// Every line below is diffed against Node.

// 1. Base-created closures see the derived instance.
class Greeter {
  name: string = '';
  msg: string = 'x';
  greet: () => string;
  constructor() {
    this.greet = () => 'base:' + this.name + ':' + this.msg;
  }
}

class SmartGreeter extends Greeter {
  msg = 'hi';
  constructor() {
    super();
    this.name = 'smart';
  }
}

const smart = new SmartGreeter();
console.log(smart.greet());
smart.name = 'later';
console.log(smart.greet());

// 2. A generic base whose type parameter is type-level only, the Hono shape:
// the base constructor builds the dispatch closures, the derived constructor
// installs the state they read.
class AppBase<E = string> {
  routes: string[] = [];
  prefix: string = '';
  add: (path: string) => number;
  handle: (path: string) => string;
  constructor(label: string) {
    console.log('base constructor for ' + label);
    this.add = (path: string) => {
      this.routes.push(this.prefix + path);
      return this.routes.length;
    };
    this.handle = (path: string) => {
      const full = this.prefix + path;
      return this.routes.includes(full) ? 'hit ' + full : 'miss ' + full;
    };
  }
}

class App<E = string> extends AppBase<E> {
  constructor(prefix: string) {
    super('app');
    this.prefix = prefix;
  }
}

const app = new App<number>('/api');
console.log(app.add('/users'), app.add('/posts'));
console.log(app.handle('/users'), app.handle('/nope'));

class PlainApp extends AppBase<boolean> {
  constructor() {
    super('plain');
    this.prefix = '#';
  }
}

const plain = new PlainApp();
plain.add('a');
console.log(plain.handle('a'), plain.routes.length);

// 3. The base constructor's calls on `this` run against the derived instance:
// a method it calls reads state an earlier base statement wrote.
class Tally {
  total: number = 0;
  history: string;
  constructor(seed: number) {
    this.bump(seed);
    this.history = 'seeded ' + this.total;
  }
  bump(by: number): void {
    this.total = this.total + by;
  }
}

class DoubleTally extends Tally {
  doubled: boolean = true;
  constructor() {
    super(5);
    const seeded = this.total;
    this.bump(seeded);
  }
}

const tally = new DoubleTally();
console.log(tally.total, tally.history, tally.doubled);

// 4. Three levels of value classes: arguments, parameter properties and field
// initializers run once each, base-most first.
class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}
}

class Point3 extends Point {
  z: number = 0;
  constructor(x: number, y: number, z: number) {
    super(x, y);
    this.z = z + this.x;
  }
}

class Point4 extends Point3 {
  w: number = 9;
  constructor(public tag: string) {
    super(1, 2, 3);
    this.w = this.w + this.z;
  }
}

const p4 = new Point4('p');
console.log(p4.x, p4.y, p4.z, p4.w, p4.tag);

// 5. An abstract base's constructor runs too (it used to be dropped).
abstract class Counter {
  count: number;
  constructor(start: number) {
    this.count = start * 10;
  }
  next(): number {
    this.count = this.count + 2;
    return this.count;
  }
}

class FromThree extends Counter {
  constructor() {
    super(3);
  }
}

const counter = new FromThree();
console.log(counter.next(), counter.next());

// 6. An implicit derived constructor over a generic base forwards the base
// constructor's parameters at the derived class's type arguments.
class Labelled<T> {
  constructor(public label: string) {}
}

class Named extends Labelled<number> {}

console.log(new Named('forwarded').label);
