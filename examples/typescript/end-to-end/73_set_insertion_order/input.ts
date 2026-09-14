// A `Set` iterates in INSERTION ORDER, as the spec requires.
//
// A source `Set` of value-equality primitives (`string`, `number` widened to
// `i64`, `boolean`) used to be emitted as a Rust `HashSet`, which has no order
// at all: `[...new Set('hello')].join('')` answered `leoh` from one
// construction and `hoel` from another in the same program. Sizes and
// membership were right, so nothing threw and nothing failed to compile — code
// that de-duplicates while preserving order, one of the most common JS idioms
// there is, silently scrambled its output.
//
// Every `Set` is now the `SmeltJsSet` runtime container, which was already
// insertion-ordered (a `Vec` of entries plus a hash slot index, the same shape
// as the record store) and was already used for every element type that cannot
// key a Rust `HashSet`. So this is one container for all sets rather than a new
// one: lookups stay hashed, and three things the `HashSet` backing had to do
// without come along — SameValueZero membership, a stable object id, and the
// `__smelt_set` erasure marker that makes `isSet`, `instanceof Set` and the
// `unknown` round-trip work on a primitive set too.
//
// Order is observable through every one of these, which is why they are all
// asserted rather than just the spread.

const letters = new Set("hello");
console.log([...letters].join("|"));

// The same order through the other observers.
const seen: string[] = [];
letters.forEach((value) => {
  seen.push(value);
});
console.log(seen.join("|"));

const iterated: string[] = [];
for (const value of letters) {
  iterated.push(value);
}
console.log(iterated.join("|"));

console.log(Array.from(letters).join("|"));
console.log([...letters.values()].join("|"));
console.log([...letters.keys()].join("|"));

// Insertion order survives mutation: a deleted element does not hold its slot,
// and re-adding an existing element does NOT move it to the end.
const letterList = new Set(["c", "a", "b"]);
letterList.delete("a");
letterList.add("z");
letterList.add("c");
console.log([...letterList].join("|"));
console.log(letterList.size);

// Numbers keep source order rather than sorting, which is what a hash-ordered
// container looked like it was doing for small integers.
const numbers = new Set([30, 10, 20, 10]);
console.log([...numbers].join("|"));

// Booleans, the third primitive that used to take the `HashSet` path.
const flags = new Set([true, false, true]);
console.log([...flags].map((flag) => (flag ? "t" : "f")).join("|"));

// A primitive set now carries JS identity and survives the `unknown`
// round-trip, both of which the `HashSet` backing could not do.
const same = letters;
console.log(same === letters);
console.log(new Set("hello") === letters);
