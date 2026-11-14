"use strict";
// Every call supplies its callee's `this`: a member call, `.call`, `.apply`
// and `.bind` supply a receiver, a plain call supplies `undefined`, typed or
// erased; an arrow sees its enclosing function's.

function attempt(label: string, run: () => unknown): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, (error as Error).name, (error as Error).message);
  }
}

function whoAmI(this: any): unknown {
  return this === undefined ? "undefined" : this.name;
}
const host: any = { name: "host", who: whoAmI };

// A typed function reading `this`: its receiver comes from the call.
attempt("member call", () => host.who());
attempt("plain call", () => whoAmI());
attempt("call", () => whoAmI.call({ name: "called" }));
attempt("plain call inside a method", () => ({ name: "outer", run(this: any) { return whoAmI(); } }).run());

// An arrow sees its enclosing function's `this`, however the arrow is called.
function arrowsSeeOwnThis(this: any): unknown {
  const inner = () => this?.name;
  return inner();
}
const holder: any = { name: "holder", go: arrowsSeeOwnThis };
attempt("arrow in a method call", () => holder.go());
attempt("arrow in a plain call", () => arrowsSeeOwnThis());
const named: any = { name: "named", each(this: any) { return [1, 2].map(() => this.name); } };
attempt("arrow callback in a method", () => named.each());

// A callback a helper invokes is a plain call.
attempt("function callback", () => [1].map(function (this: any) { return this === undefined; }));

// A callable object forwarding its own receiver (es-toolkit's `throttle`
// forwards with `debounced.apply(this, args)`) keeps the method's receiver.
interface Debounced {
  (...args: unknown[]): void;
  cancel: () => void;
}
function debounceLike(target: (this: any, ...args: unknown[]) => void): Debounced {
  let pendingThis: any = undefined;
  const debounced = function (this: any, ...args: unknown[]) {
    pendingThis = this;
    target.apply(pendingThis, args);
  };
  debounced.cancel = () => {
    pendingThis = undefined;
  };
  return debounced;
}
function throttleLike(target: (this: any, ...args: unknown[]) => void): (this: any, ...args: unknown[]) => void {
  const debounced = debounceLike(target);
  return function (this: any, ...args: unknown[]) {
    debounced.apply(this, args);
  };
}
let captured: unknown;
const logger = { msg: "hello world", log: throttleLike(function (this: any) { captured = this?.msg; }) };
attempt("throttled method", () => {
  logger.log();
  return captured;
});
attempt("throttled plain call", () => {
  const log = logger.log;
  captured = "unset";
  log();
  return captured;
});

// A `function` inside a class method has its own `this`; an arrow there, and
// an arrow inside an arrow, sees the enclosing one.
class Box {
  name = "box";
  viaFunction(): unknown {
    const f = function (this: any) {
      return this === undefined ? "own undefined" : this.name;
    };
    return f();
  }
  viaArrow(): unknown {
    const f = () => this.name;
    return f();
  }
}
const box = new Box();
attempt("function in a method", () => box.viaFunction());
attempt("arrow in a method", () => box.viaArrow());
function nestedArrows(this: any): unknown {
  const outer = () => {
    const inner = () => this?.name;
    return inner();
  };
  return outer();
}
const nestedHost: any = { name: "nested", nestedArrows };
attempt("nested arrows", () => nestedHost.nestedArrows());
