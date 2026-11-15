// An unannotated object literal whose identity is observed is a record struct
// and keeps that identity; one aliased at an annotated binding stays a map.
//
// A record struct is one object. `===` between two of them compares their
// identities, and a struct that enters a `Set<unknown>`, an `unknown[]` or an
// `unknown` parameter does so as a view of itself with the struct's own
// identity, so `has`, `indexOf` and `includes` find it again. `origin`,
// `tracked`, `first` and `second` are structs.
//
// `third` is aliased at a binding annotated `unknown`, a use the rule does not
// model (`literal_record_uses`, `Disqualifier::Other`), and keeps the map
// lowering.
//
// Every line below is diffed against Node.

function sameObject(left: unknown, right: unknown): boolean {
  return left === right;
}

function membership(): void {
  const origin = { id: 1, label: "origin" };
  const seen = new Set<unknown>();
  seen.add(origin);
  console.log(seen.has(origin));

  const tracked = { id: 2, label: "tracked" };
  const registry: unknown[] = [];
  registry.push(tracked);
  console.log(registry.indexOf(tracked), registry.includes(tracked));
}

function comparison(): void {
  // Two literals of one shape are two objects.
  const first = { id: 3, label: "same" };
  const second = { id: 3, label: "same" };
  console.log(first === second, first !== second, Object.is(first, second));
  console.log(sameObject(second, second), sameObject(first, second));

  // The same object seen through an `unknown` binding is still that object.
  const third = { id: 4, label: "held" };
  const held: unknown = third;
  console.log(third === held, Object.is(third, held));

  // Asking whether the binding is present is not an identity observation:
  // this literal is a record.
  const present = { id: 5, label: "present" };
  console.log(present !== undefined, present !== null, present.id);
}

membership();
comparison();
