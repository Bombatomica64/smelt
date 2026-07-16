// Exercises callable-object construction: a function-typed local (`counter`)
// collects straight-line property writes (`counter.bump = …`) and is returned at
// a callable-interface type. The construction becomes a real struct literal whose
// fields are the base callable (`__smelt_call`) plus each written method, all
// sharing the captured `count` state through their (Rc/RefCell-backed) closure
// environments (design §4: a by-value struct of shared-behavior closures).
//
// The consumer calls the stored methods and observes the shared mutable state,
// proving the writes were bundled into a live value rather than dropped.
interface Counter {
  (): number;
  bump(): number;
  current(): number;
  reset(): void;
}

function makeCounter(): Counter {
  let count = 0;
  const counter = function (): number {
    return count;
  };
  counter.bump = function (): number {
    count = count + 1;
    return count;
  };
  counter.current = function (): number {
    return count;
  };
  counter.reset = function (): void {
    count = 0;
  };
  return counter;
}

const c = makeCounter();
console.log(c.bump());
console.log(c.bump());
console.log(c.current());
c.reset();
console.log(c.current());
console.log(c.bump());
