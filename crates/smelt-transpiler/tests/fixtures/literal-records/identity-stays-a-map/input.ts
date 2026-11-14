// An unannotated object literal whose identity is observed stays a map.
//
// The map IS the JavaScript object: it enters a `Set<unknown>`, an
// `unknown[]` or an `unknown` binding as itself, and `===`, `Object.is`, `has`,
// `indexOf` and `includes` find it again. A literal used that way keeps the
// map lowering it always had (`literal_record_uses`, `Disqualifier::Identity`,
// and `Disqualifier::Call` for the literals handed to a function).
//
// This family is a precaution: lowered as record structs, these particular
// lines print the same, because a struct keeps its identity into an erased
// slot. It is not known to hold for every identity observation (the test
// framework's `toBe` on a value that came back from a callee does not), so
// the fixture pins the map lowering and Node's answers.
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
