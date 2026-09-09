// `receiver[key]()` on a receiver whose member set is known is a CHOICE among
// known methods, so it lowers as one: a chain of `key === "<name>"` tests, each
// arm the ordinary method call. It never needs a dynamic lookup.
//
// Before this, the computed read answered `unknown`, and an `unknown` callee
// becomes `undefined`, so the whole call collapsed: `return req[key]()` emitted
// `return String::new()` -- the body replaced by a default value, with no
// diagnostic. Hono's `await req[cacheKey]()` is that shape.
class Body {
  json(): string {
    return 'json-body';
  }
  text(): string {
    return 'text-body';
  }
  raw(): string {
    return 'raw-body';
  }
}

interface Reader {
  head(): number;
  tail(): number;
}

class Counter implements Reader {
  head(): number {
    return 1;
  }
  tail(): number {
    return 2;
  }
}

function read(body: Body, key: 'json' | 'text' | 'raw'): string {
  return body[key]();
}

function count(counter: Counter, key: 'head' | 'tail'): number {
  return counter[key]();
}

const body = new Body();
console.log(read(body, 'json'));
console.log(read(body, 'text'));
console.log(read(body, 'raw'));

const counter = new Counter();
console.log(count(counter, 'head'));
console.log(count(counter, 'tail'));
