// `+` with a UNION operand is string concatenation, not addition.
//
// ECMAScript's `ApplyStringOrNumericBinaryOperator` coerces both operands of
// `+` with `ToPrimitive` and concatenates as soon as either result is a String;
// only when neither is does it add. TypeScript decides that statically from the
// operand types, and a union with at least one `string` arm counts: `tsc`
// accepts every line below and types each `+` as `string`.
//
// The generated Rust used to route a union operand into the erased
// number-coercion `match` instead, so a concatenation became `ToNumber(lhs) +
// ToNumber(rhs)` — `NaN` at runtime where it type-checked at all.
//
// Every line is diffed against Node.

type StringBuffer = (string | Promise<string>)[];

// A compound assignment writes the concatenated `string` BACK into the element,
// whose declared type is still the union: the result re-enters through the
// union's string arm rather than the erased numeric path.
const buffer: StringBuffer = ["head"];
buffer[0] += "-tail";
buffer[0] += "!";
console.log(buffer[0]);

// The same operand in expression position, where the destination IS a string.
const joined: string = buffer[0] + " and more";
console.log(joined);

// The union on the RIGHT of `+` concatenates just as it does on the left.
console.log("prefix:" + buffer[0]);

// A NON-string arm reaching the site is stringified the way `${}` is: a
// `Promise` tags as `[object Promise]`, which is what `ToPrimitive` then
// `ToString` produce in JavaScript.
const withPromise: StringBuffer = ["first", Promise.resolve("second")];
console.log("tagged:" + withPromise[1]);

// A `string | number` accumulator takes both spellings of `+=`. The string
// operand makes the first one a concatenation; the number operand is
// stringified because the ACCUMULATOR is string-like, which is the same rule
// read from the other side.
let acc: string | number = "n=";
acc += "1";
acc += 2;
acc += 3.5;
console.log(acc);

// The same accumulator declared the other way round: a number-valued union
// still concatenates once a string reaches the operator.
let mixed: string | number = 10;
mixed = mixed + "x";
console.log(mixed);

// A union element read out of a list and concatenated inside a function, so the
// rule is exercised away from the literal that defined the list.
function describe(parts: StringBuffer, index: number): string {
  return "[" + parts[index] + "]";
}
console.log(describe(buffer, 0));
console.log(describe(withPromise, 0));
