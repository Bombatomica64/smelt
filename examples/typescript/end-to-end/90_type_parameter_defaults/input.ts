// A type reference may omit TRAILING type arguments that the declaration
// defaults — round 30, Agent C item 1.
//
// TypeScript's rule: `class Slot<T, U = string, V = number>` referenced as
// `Slot<boolean>` MEANS `Slot<boolean, string, number>`, and a declaration
// whose parameters are all defaulted may be referenced with no argument list at
// all. A default is lowered in the declaration's own parameter scope, so an
// earlier parameter may appear in a later default (`<A, B = A[]>`).
//
// Smelt lowered interface and alias references through their defaults already,
// but a CLASS reference fell through to a fallback that kept whatever short
// list the source wrote. The HIR then carried a reference of the wrong arity,
// so Rust emission wrote `Slot<bool>` against a three-parameter struct and the
// defaulted members lost their concrete types.
//
// Every line below is diffed against Node.

class Slot<T, U = string, V = number> {
  first: T;
  second: U;
  third: V;
  constructor(first: T, second: U, third: V) {
    this.first = first;
    this.second = second;
    this.third = third;
  }
  label(): U {
    return this.second;
  }
  count(): V {
    return this.third;
  }
}

// A FIELD type that omits the two defaulted arguments. `second` is `string` and
// `third` is `number` here ONLY because the defaults were taken.
class Holder {
  slot: Slot<boolean>;
  constructor(slot: Slot<boolean>) {
    this.slot = slot;
  }
  describe(): string {
    return this.slot.label().toUpperCase() + ":" + (this.slot.count() + 1);
  }
}

const holder = new Holder(new Slot<boolean, string, number>(true, "one", 1));
console.log(holder.describe(), holder.slot.first);

// A PARAMETER type and a RETURN type that both omit them.
function relabel(slot: Slot<boolean>, label: string): Slot<boolean> {
  return new Slot<boolean, string, number>(slot.first, label, slot.count() * 2);
}

const relabelled = relabel(holder.slot, "two");
console.log(relabelled.label(), relabelled.count(), relabelled.first);

// An `extends` clause obeys the same rule and the frontend records the full
// base argument list for it. It is asserted in
// `smelt-frontend-ts`'s `heritage_clause_takes_base_type_parameter_defaults`
// rather than here, because flattening a subclass of a GENERIC base into a
// monomorphic struct is a separate, unimplemented family: the inherited members
// are emitted without substituting `base_args` at all, which erases them. See
// `blocker-logs/hono-round30-types.md`.

// A declaration whose parameters are ALL defaulted, referenced with NO argument
// list at all.
class Config<A = string, B = number> {
  key: A;
  value: B;
  constructor(key: A, value: B) {
    this.key = key;
    this.value = value;
  }
  name(): A {
    return this.key;
  }
  amount(): B {
    return this.value;
  }
}

const config: Config = new Config<string, number>("width", 42);
console.log(config.name(), config.amount());

function readConfig(input: Config): string {
  return input.name() + "=" + input.amount();
}

console.log(readConfig(config));

// A later default that MENTIONS an earlier parameter: `Pair<boolean>` means
// `Pair<boolean, boolean[]>`, so the substitution runs left to right and `rest`
// is a real list of booleans rather than an unresolved parameter.
class Pair<A, B = A[]> {
  head: A;
  rest: B;
  constructor(head: A, rest: B) {
    this.head = head;
    this.rest = rest;
  }
  tail(): B {
    return this.rest;
  }
}

const pair: Pair<boolean> = new Pair<boolean, boolean[]>(true, [false, true]);
console.log(pair.head, pair.tail().length, pair.tail()[1]);
