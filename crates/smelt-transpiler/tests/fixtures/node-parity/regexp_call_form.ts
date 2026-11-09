// `RegExp(pattern, flags)` called without `new` constructs exactly like
// `new RegExp(pattern, flags)`, for literal and computed patterns alike.

const word = "a+b";
const exact = RegExp(`^${word.replace(/[+]/g, "\\+")}$`);
console.log(exact.test("a+b"), exact.test("aab"));

const parts = ["cat", "dog"];
const either = RegExp(parts.join("|"), "g");
console.log("cat and dog".replace(either, "pet"), either.global);

const literal = RegExp("x\\d+");
console.log(literal.source, literal.test("x12"), literal.flags === "");
