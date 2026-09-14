// A unique `Symbol()` bound to a MODULE-LEVEL const is a stable static key: that
// initializer runs once, so the symbol it binds is one symbol for the program's
// lifetime. A class member keyed by it is therefore an ordinary member with a
// symbol name, and `instance[KEY]` is an ordinary static read of it.
//
// Both halves were missing. The declaration was rejected outright
// ("dynamic computed method names are not lowered yet" -- Hono's
// `get [GET_MATCH_RESULT]()`), and once it lowered, the READ still resolved to
// nothing, so the program answered `undefined` for a member that exists.
//
// `SAME_A` and `SAME_B` are the identity half: two unique symbols with the same
// description are two distinct members, told apart by the source offset each
// `Symbol()` call sits at.
const MATCH_RESULT = Symbol();
const DESCRIBED = Symbol('described');
const SHARED = Symbol.for('shared');
const SAME_A = Symbol('same');
const SAME_B = Symbol('same');

class Holder {
  get [MATCH_RESULT](): number {
    return 41;
  }

  [DESCRIBED](offset: number): number {
    return 1 + offset;
  }

  [SHARED](): string {
    return 'registry';
  }

  [SAME_A](): string {
    return 'a';
  }

  [SAME_B](): string {
    return 'b';
  }

  plain(): string {
    return 'plain';
  }
}

const holder = new Holder();
console.log(holder[MATCH_RESULT]);
console.log(holder[DESCRIBED](1));
console.log(holder[SHARED]());
console.log(holder[SAME_A]() + holder[SAME_B]());
console.log(holder.plain());
