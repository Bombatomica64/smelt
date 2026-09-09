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
// `x` means. The last lines are that agreement, now asserted on ORDER as well
// as membership — when this fixture was first written the generated `Set` was a
// Rust `HashSet` and could not answer an order question (H52, fixed in
// `71`/`72`'s round; see `blocker-logs/hono-h52-set-iteration-order.md`).
//
// Still not accepted, and deliberately so rather than guessed at: a generator
// or a `Map` operand. `[...generator]` does not lower either, so there is no
// shared iterable rule to route them through yet, and adding one only at the
// `Set` constructor would be the special case this project refuses.

// Hono's line, character for character.
const regExpMetaChars = new Set(".\\+*[^]$()");
console.log(regExpMetaChars.size);
console.log(regExpMetaChars.has("*"));
console.log(regExpMetaChars.has("$"));
console.log(regExpMetaChars.has("("));
console.log(regExpMetaChars.has("a"));
console.log([...regExpMetaChars].join(""));

// Duplicates collapse, and the survivors keep first-appearance order.
const repeated = new Set("aabbcc");
console.log(repeated.size);
console.log([...repeated].join("|"));
console.log(repeated.has("d"));

// The arms that already worked, so the new one cannot have displaced them.
const fromArray = new Set(["c", "a", "b", "a"]);
console.log(fromArray.size);
console.log([...fromArray].join("|"));
const copied = new Set(fromArray);
console.log(copied.size);
console.log([...copied].join("|"));

// `new Set(x)` and `new Set([...x])` agree, element for element and in order.
const direct = new Set("hello");
const viaSpread = new Set([..."hello"]);
console.log([...direct].join("|"));
console.log([...direct].join("|") === [...viaSpread].join("|"));
