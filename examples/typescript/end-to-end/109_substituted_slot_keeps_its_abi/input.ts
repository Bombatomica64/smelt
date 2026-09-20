// A generic nominal type's declaration is the only thing that knows how its
// slots are passed, and substituting its type arguments must not change that.
//
// Two shapes, one rule:
//
//  * `Pair<number>` is constructed from a `Cell<number>`, so the constructor's
//    declared `Cell<A>` parameter has to be taken at `Cell<number>`. Rebuilding
//    the record against the unsubstituted declaration erased its `value` field.
//  * `Sink<T>` declares a callable slot whose last parameter is a bare `T`. The
//    generated struct passes that slot by shared reference, and it still does
//    once `T` is a tuple — while the callable a projection rebuilds for the
//    field is rendered at the substituted tuple.

class Cell<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

class Pair<A> {
  left: Cell<A>;
  constructor(left: Cell<A>) {
    this.left = left;
  }
}

interface Sink<T> {
  add: (label: string, value: T) => void;
  size: () => number;
}

const pair = new Pair<number>(new Cell<number>(7));
console.log(pair.left.value);

const entries: Array<[string, number]> = [];
const raw: unknown = {
  add: (label: string, value: [string, number]) => {
    entries.push([label + ':' + value[0], value[1]]);
  },
  size: () => entries.length,
};

const sink = raw as Sink<[string, number]>;
sink.add('first', ['a', 1]);
sink.add('second', ['b', 2]);
console.log(sink.size());
console.log(entries[0][0]);
console.log(entries[1][1]);
