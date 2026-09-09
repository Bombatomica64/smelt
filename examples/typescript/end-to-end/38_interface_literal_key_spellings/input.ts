// A JavaScript property key is case-sensitive and is never case-folded. An
// interface FIELD, by contrast, has two spellings: the source name it was
// written with and the Rust-safe rendering the struct field carries
// (`camelCase` -> `camel_case`). Matching an object literal's keys against the
// rendered spelling therefore drops every field whose source name is not
// already a valid Rust name -- it looked absent, took the optional default, and
// the program printed `undefined` where Node prints the value.
//
// So this fixture mixes all three shapes deliberately: a single-word key that
// renders unchanged, a camelCase key that does not, and a key that already
// looks snake_case. All three must survive, at every nesting level, and the
// numeric one must stay a number.
interface Shape {
  plain?: number;
  camelCase?: string;
  snake_case?: string;
  aVeryLongCamelName?: number;
}

interface Wrapper {
  innerShape?: Shape;
}

const full: Shape = {
  plain: 1,
  camelCase: 'a',
  snake_case: 'b',
  aVeryLongCamelName: 41,
};
const partial: Shape = { camelCase: 'only' };
const nested: Wrapper = { innerShape: { camelCase: 'deep', plain: 2 } };

function textOf(value: string | undefined): string {
  return value === undefined ? 'undefined' : value;
}

function numberOf(value: number | undefined): string {
  return value === undefined ? 'undefined' : `${value + 1}`;
}

console.log(numberOf(full.plain));
console.log(textOf(full.camelCase));
console.log(textOf(full.snake_case));
// Stays a number: 42, not the text "41" with a "1" glued on.
console.log(numberOf(full.aVeryLongCamelName));
console.log(textOf(partial.camelCase));
console.log(textOf(partial.snake_case));
const inner = nested.innerShape;
console.log(inner === undefined ? 'no inner' : textOf(inner.camelCase));
console.log(inner === undefined ? 'no inner' : numberOf(inner.plain));
