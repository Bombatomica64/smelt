// JavaScript's number-to-string rule, which is not Rust's.
//
// Rust's `f64` `Display` and ECMA-262's `Number::toString` agree on most values
// and part company at both ends of the range, because Rust never switches to
// exponential notation and JavaScript does: at a decimal exponent above 21, and
// at or below -7. So `1e21` printed as twenty-two digits, `1e-7` as
// `0.0000001`, and a subnormal as 321 characters of zeros — in EVERY program
// that printed such a number, through one shared helper.
//
// The digits themselves are not reimplemented: ECMA-262 asks for the shortest
// decimal string that round-trips, which is exactly what Rust's `{:e}`
// produces, so the helper reads the digits and the exponent off that and then
// lays them out by the spec's four cases. Digit generation is the part that
// would be easy to get subtly wrong.
//
// One value has two right answers, and both are Node's: `String(-0)` is `"0"`,
// because the spec's rule folds the sign of zero, while `console.log(-0)`
// prints `-0`, because that path is `util.inspect` rather than a coercion.
//
// Every line below is diffed against Node.

const values = [
  0, -0, 1, -1, 0.5, 100, 1e6, 1e20, 1e21, 1e22, 5e-324, 1e-6, 1e-7,
  1.7976931348623157e308, 0.1, 2 / 3, 255, -1e21, 1e-323,
];

// The three coercions that reach the rule: a template, `String(x)` and
// `Number.prototype.toString`. They must agree with each other as well as with
// Node.
for (const value of values) {
  console.log(`${value}|${String(value)}|${value.toString()}`);
}

// `console.log` of a number directly, where the `-0` spelling differs from the
// coercion above.
console.log(0, -0, 1e21, 1e-7);
console.log(String(-0), `${-0}`);

// NaN and the infinities, which Rust's `Display` spells `NaN`, `inf` and
// `-inf`.
console.log(NaN, Infinity, -Infinity);
console.log(String(NaN), String(Infinity), String(-Infinity));

// An array join stringifies each item the way `String(item)` does.
console.log(values.join(" "));

// An ERASED number takes the same rule — `Display for SmeltUnknown` routes its
// number tag through the same helper — but that is asserted in the codegen
// tests rather than here: a source `unknown` parameter is generated as a
// `SmeltUnknown` signature, which the SmeltUnknown report counts as avoidable
// erasure until its classifier can tell a source `unknown` from a union that
// merely lost its shape (see
// `blocker-logs/standards-example-goldens-cover-modules.md`). The coverage is
// in `emits_generic_hash_map_into_unknown_runtime_conversion` and
// `emits_string_coercion_default_sort_for_union_elements`.

// A property KEY is a stringified number too.
const keyed: Record<string, number> = {};
keyed[1e21] = 1;
console.log(Object.keys(keyed)[0]);

// And so is a JSON number.
console.log(JSON.stringify({ big: 1e21, small: 1e-7 }));
