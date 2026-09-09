// `new Set(string)` is the set of the string's characters.
//
// A string is an iterable, so `new Set('abc')` is a three-element set in
// JavaScript. Only arrays, optional arrays, another Set and erased surfaces
// were accepted, so Hono's regexp meta-character set
// (`new Set('.\\+*[^]$()')` in `src/router/reg-exp-router/node.ts`) was
// "new Set(iterable) currently requires an array argument" — and that one line
// blocked the whole router slice from transpiling.
//
// The conversion is NOT written at the constructor: it goes through
// `list_expr_from_spread_value`, the same helper that lowers `[...iterable]`,
// so `new Set(x)` and `new Set([...x])` cannot disagree about what iterating
// `x` means. The size and membership checks below are that agreement.
//
// Two things are deliberately NOT asserted here, and neither is guessed at:
//
// * a generator or `Map` operand. `[...generator]` does not lower either, so
//   there is no shared iterable rule to route them through yet, and inventing
//   one only at the `Set` constructor would be the special case this project
//   refuses.
// * ITERATION ORDER. A JavaScript `Set` iterates in insertion order; the
//   generated `Set` does not, and `[...new Set('hello')].join('')` came back
//   as `leoh` for one construction and `hoel` for another in the same program.
//   That is a runtime-container gap, older and wider than this rule (it applies
//   to `new Set(array)` too), recorded in
//   `blocker-logs/hono-h52-set-iteration-order.md`. Asserting order here would
//   have frozen the wrong behaviour into a golden.

// Hono's line, character for character.
const regExpMetaChars = new Set(".\\+*[^]$()");
console.log(regExpMetaChars.size);
console.log(regExpMetaChars.has("*"));
console.log(regExpMetaChars.has("$"));
console.log(regExpMetaChars.has("("));
console.log(regExpMetaChars.has("a"));

// Duplicates collapse.
const repeated = new Set("aabbcc");
console.log(repeated.size);
console.log(repeated.has("a") && repeated.has("b") && repeated.has("c"));
console.log(repeated.has("d"));

// The arms that already worked, so the new one cannot have displaced them.
const fromArray = new Set(["a", "b", "a"]);
console.log(fromArray.size);
const copied = new Set(fromArray);
console.log(copied.size);
console.log(copied.has("b"));

// `new Set(x)` and `new Set([...x])` agree on contents.
const direct = new Set("hello");
const viaSpread = new Set([..."hello"]);
console.log(direct.size === viaSpread.size);
console.log([...direct].every((char) => viaSpread.has(char)));
console.log([...viaSpread].every((char) => direct.has(char)));
