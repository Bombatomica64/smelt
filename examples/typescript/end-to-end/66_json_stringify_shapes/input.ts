// `JSON.stringify` is specified by ECMA-262, not by the Rust serializer.
//
// Every value now crosses the erased boundary before it is serialized, so ONE
// rule (`Serialize for SmeltUnknown`) decides the output for every shape —
// which is also what makes a UNION work: its arm is only known at run time, and
// the per-arm answer is that tag's arm in the same impl. Serializing a
// concretely typed value directly was serde's format rather than JavaScript's,
// and printed `{"a":1.0}` for every integral number in the crate.
//
// What this fixture pins is the spec's number formatting through concretely
// typed values, and the union dispatch: an integral number has no fraction
// (`1`, not `1.0`), `-0` is `0`, a non-finite number is `null`, and `1e21`
// keeps JavaScript's exponent spelling.
//
// Three families live in
// `crates/smelt-codegen-rust/tests/json_stringify_runtime.rs` instead of here,
// because each needs a value that is ERASED today and this corpus holds a
// zero-avoidable-erasure invariant: the byte views (a view serializes as its
// element indices, `ArrayBuffer` and `DataView` as `{}`), the non-JSON values
// (a function- or symbol-valued property is omitted, one inside an array is
// `null`), and enumeration over a view. Making the byte views concrete is the
// typed-array-views item.
type Payload = number | string | Record<string, number>;

function encode(value: Payload): string {
  return JSON.stringify(value);
}

// One call site, three arms decided at run time.
console.log(encode(7));
console.log(encode("seven"));
console.log(encode({ a: 1, b: 2.5 }));

// The spec's number formatting.
const counts: Record<string, number> = { hits: 3, ratio: 0.5 };
console.log(JSON.stringify(counts));
console.log(JSON.stringify(4));
console.log(JSON.stringify(-0));
console.log(JSON.stringify(0 / 0));
console.log(JSON.stringify(1e21));
console.log(JSON.stringify(1 / 3));

// A list of numbers keeps its element type all the way to the serializer.
const ratios: number[] = [1, 2.5, 1e21];
console.log(JSON.stringify(ratios));

// A string, a boolean and an absent value.
console.log(JSON.stringify("seven"));
console.log(JSON.stringify(true));
console.log(JSON.stringify(null));
