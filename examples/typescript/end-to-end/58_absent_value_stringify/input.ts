// Stringifying an ABSENT value prints a word, not nothing.
//
// `String(x)` and `${x}` on a value that holds nothing used to render the empty
// string, because the coercion reached for Rust's `unwrap_or_default()`. That is
// wrong in every spelling JavaScript has: `String(undefined)` is `"undefined"`,
// `String(null)` is `"null"`, and `"x" + undefined` is `"xundefined"` — the
// word is part of the output, and a program that logs one silently dropped it.
//
// The word comes from the source language of the body being emitted, decided
// once during MIR lowering (`MirFunction::absent`) the way `console.log`
// already decides it: JavaScript prints `undefined`, Python prints `None`. A
// crate can hold both, so codegen cannot guess it.
//
// One known imprecision, and it is deliberate: TypeScript's `null` and
// `undefined` both intern to one internal absent type, so a value ANNOTATED
// `string | null` prints `undefined` rather than `null`. A value whose type IS
// `null` still prints `null` (below), because that type survives on its own.
// Printing the right word for both annotations needs the type split that the
// es-toolkit plan tracks as D1; until then one word has to be chosen for the
// conflated case, and `undefined` is what nearly every operation that produces
// an absent value in TypeScript returns.

const missingText: string | undefined = undefined;
console.log(String(missingText));
console.log(`${missingText}`);

const presentText: string | undefined = "here";
console.log(String(presentText));
console.log(`${presentText}`);

// An optional NUMBER and an optional BOOLEAN: both used to interpolate as
// nothing at all, which made a logged line read as if the value were an empty
// string rather than absent.
const missingCount: number | undefined = undefined;
console.log(`count=${missingCount}`);
const presentCount: number | undefined = 3;
console.log(`count=${presentCount}`);

// A value whose own type is `null` keeps its own word.
const nothing = null;
console.log(`${nothing}`);
console.log("x" + `${nothing}`);

// The concatenation spelling of the same rule.
function label(prefix: string, value: string | undefined): string {
  return prefix + value;
}
console.log(label("p:", "v"));
console.log(label("p:", undefined));
