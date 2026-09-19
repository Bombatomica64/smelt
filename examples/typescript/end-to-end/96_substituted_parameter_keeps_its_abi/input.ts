// Substituting a type parameter at a call site does not change how the callee
// is called — round 31, Agent E item 3.
//
// A callable slot declared over a bare type parameter is emitted as
// `Rc<dyn Fn(&T) -> ..>`: `param_type_is_by_shared_reference` answers TRUE for a
// type parameter, because the ABI has to be the CALLEE's and only its
// declaration knows it. Rust then instantiates that slot at `Fn(&f64)` for a
// `Chain<number>` — but MIR hands the call site the SUBSTITUTED parameter type
// `f64`, which the same predicate answers FALSE for, so the argument was packed
// by value against a slot spelled `&`:
//
//   expected `&(SmeltUnknown, RouterRoute)`, found `(SmeltUnknown, RouterRoute)`
//
// (that spelling is Hono's `Router<[unknown, RouterRoute]>`, the interface half
// of the same rule; it has no fixture here because constructing such a record
// from an object literal is a separate open seam — see
// `blocker-logs/hono-round31-generics.md` — and is asserted as emitted text by
// `a_generic_interface_slot_is_called_with_its_declared_abi` instead.)
//
// The value is still rendered at the substituted type — that is what it has to
// be coerced to — while the ABI is asked of the declaration. Both a scalar and a
// heap instantiation are covered, because the two differ in nothing the rule may
// depend on.
//
// Every line below is diffed against Node.

class Chain<T> {
  value: T;
  // A class field whose declared parameter is the class's own type parameter.
  advance: (next: T) => Chain<T>;
  seen: T[];
  constructor(value: T) {
    this.value = value;
    this.seen = [value];
    this.advance = (next: T) => {
      this.value = next;
      this.seen.push(next);
      return this;
    };
  }
}

const numbers = new Chain<number>(1);
console.log(numbers.advance(2).value);
console.log(numbers.advance(7).value);
console.log(numbers.seen.join(','));

const words = new Chain<string>('a');
console.log(words.advance('b').value);
console.log(words.advance('c').seen.join('|'));
