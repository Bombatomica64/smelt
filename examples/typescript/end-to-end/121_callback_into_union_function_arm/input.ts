// A callback passed where a `Function | Class` union is expected is injected
// into the union's function arm, adapting its arity.
//
// Hono's `timeout(duration, exception: HTTPExceptionFunction | HTTPException)`
// is called with `() => new HTTPException(..)`: TypeScript admits a callback
// with fewer parameters than the arm declares, so the source is a different
// function type than the arm and the exact-member injection declined, leaving
// a bare `Rc<dyn Fn()>` where the union enum was expected (E0308). The union
// value is then called after a `typeof === 'function'` test, which dispatches
// through the erased callable ABI.
//
// Every line below is diffed against Node.

class Ex {
  code: number;
  constructor(code: number) {
    this.code = code;
  }
}

type ExFn = (context: string) => Ex;

// (The default is written inline: a default that reads a MODULE-level const
// from a lifted top-level arrow is a separate open defect, recorded in
// `blocker-logs/hono-phase3-round2.md`.)
const timeout = (duration: number, exception: ExFn | Ex = new Ex(504)): number => {
  const ex = typeof exception === 'function' ? exception('ctx') : exception;
  return duration + ex.code;
};

console.log(timeout(1));
console.log(timeout(2, () => new Ex(408)));
const e500: ExFn = (c: string) => new Ex(500 + c.length);
console.log(timeout(3, e500));
console.log(timeout(4, new Ex(1)));
