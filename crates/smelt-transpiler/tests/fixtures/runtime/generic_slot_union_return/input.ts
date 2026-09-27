// A generic class's callable slot whose declared return is a union over the
// class's type parameter returns the erased carrier, and the call site
// extracts the substituted union from it.
//
// Hono's `RegExpRouter<T>` declares `match: typeof match<Router<T>, T>`,
// returning `Result<T>` — a union of tuples of `T`. A union mentioning a type
// parameter has no generated enum, so the slot renders `-> SmeltUnknown`,
// while MIR types the call at the receiver-substituted `Result<string>`, a
// concrete enum. The destination took the erased value unconverted (E0308).
//
// Every line below is diffed against Node.

type Result<T> = [[T, number][], string[]] | [[T, string][]];

function lookup<T>(this: Holder<T>, method: string): Result<T> {
  return [[[this.value, method]]];
}

class Holder<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  lookup: typeof lookup<T> = lookup;
}

const holder = new Holder<string>('x');
const [res] = holder.lookup('GET');
console.log(res.length);
