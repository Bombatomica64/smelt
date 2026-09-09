// `RegExp.prototype.test` is two different operations, and which one it is
// depends on the regex's flags.
//
// Without `g` or `y` it is a pure predicate: `lastIndex` stays 0 and the answer
// depends only on the pattern and the haystack. With either flag it is
// STATEFUL — it reads `lastIndex`, advances it past the match, and answers
// `false` (resetting to 0) once the string is exhausted. So a stateless test
// lowers to a plain `is_match` and a stateful one has to run the same search
// `exec` does.

// Stateless: a literal, with and without the non-stateful flags.
console.log(/^[0-9a-f]{4}$/.test("0a1b"));
console.log(/^[0-9a-f]{4}$/.test("zzzz"));
console.log(/ab/i.test("AB"));
console.log(/^a.c$/s.test("a\nc"));

// Stateless: a constructed regex with no flags argument.
console.log(new RegExp("^a+$").test("aaa"));
console.log(RegExp("b").test("abc"));

// A constructed regex WITH a flags argument keeps the stateful path, because
// the pattern-string lowering does not carry a flags argument — taking the
// predicate path there answered `false` for a case-insensitive match.
console.log(new RegExp("^a+$", "i").test("AAA"));

// Stateful: three calls on ONE global regex answer true, true, false and the
// third resets `lastIndex`, so the fourth starts over.
const globalRe = /a/g;
console.log(globalRe.test("aa"), globalRe.test("aa"), globalRe.test("aa"));
console.log(globalRe.test("aa"));

// A sticky regex is stateful for the same reason.
const sticky = /a/y;
console.log(sticky.test("aa"), sticky.test("aa"), sticky.test("aa"));

// A stateful regex built through the constructor behaves the same.
const built = new RegExp("a", "g");
console.log(built.test("aa"), built.test("aa"), built.test("aa"));

// An INLINE global literal is a fresh object each evaluation, so no state
// carries between iterations and every one matches.
let matched = 0;
for (let index = 0; index < 3; index = index + 1) {
  if (/a/g.test("aa")) {
    matched = matched + 1;
  }
}
console.log(matched);

// A predicate over a list, which is the shape that made this worth fixing: one
// `is_match` per element rather than a match object built and thrown away.
const words = ["alpha", "beta", "gamma", "delta"];
console.log(words.filter((word) => /^[ad]/.test(word)).join(","));
console.log(words.every((word) => /a/.test(word)));
console.log(words.some((word) => /^z/.test(word)));
