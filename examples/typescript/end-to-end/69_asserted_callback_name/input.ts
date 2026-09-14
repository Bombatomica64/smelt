// A type assertion around a callback's NAME is transparent.
//
// `as`, `satisfies`, `!` and parentheses make a type-level claim about a value;
// none of them changes which function a callback names. Callback selection did
// not see through them, so `xs.filter(Boolean as any)` reported "array callback
// methods currently require arrow function callbacks" while the identical
// `xs.filter(Boolean)` lowered to the real builtin. Hono's `src/utils/html.ts`
// spells its truthiness filter the first way, which is the common way to
// satisfy `filter<T>`'s narrowing overload.
//
// The fix is the ladder, not a spelling: the name-based branch of callback
// selection (locals, items, global builtins, imported predicates, inlined local
// callbacks) is now reached with the name and span, so an asserted name gets
// exactly the callback the bare name gets.
//
// Two neighbours are deliberately NOT in this fixture, because neither is this
// rule and both would put a wrong or erased answer into a corpus that holds
// zero avoidable erasure:
//
// * the global-builtin arm (`Boolean as any`, `Number as any`) lowers to a
//   closure typed `Fn(&SmeltUnknown)` whichever way it is spelled — 17
//   avoidable erasures for four lines. A frontend test asserts that spelling
//   lowers at all; the erasure itself is
//   `blocker-logs/hono-h49-builtin-callback-operand.md`.
// * an arrow bound to an annotated const (`const p: Predicate = (v) => ...`)
//   answers WRONG when passed as a callback by name, with or without an
//   assertion — the captured local keeps the placeholder callback. Pre-existing
//   and independent of this change; see
//   `blocker-logs/hono-h50-local-arrow-callback-value.md`.

type Predicate = (value: string) => boolean;

function isLong(value: string): boolean {
  return value.length > 2;
}

const words = ["a", "abc", "ab", "abcd"];

// A named item as the callback, under each assertion spelling. All five must
// answer identically, which is the whole rule.
console.log(words.filter(isLong).join(","));
console.log(words.filter(isLong as Predicate).join(","));
console.log(words.filter(isLong!).join(","));
console.log(words.filter(isLong satisfies Predicate).join(","));
console.log(words.filter((isLong as Predicate)).join(","));

// An arrow LITERAL under an assertion keeps its concrete element and result
// types through the cast.
const lengths = words.map(((value: string): number => value.length) as (
  value: string
) => number);
console.log(lengths.join(","));

// A method value under an assertion, so the member arm of the ladder is
// covered too.
class Checker {
  matches(value: string): boolean {
    return value.startsWith("ab");
  }
}
const checker = new Checker();
console.log(words.filter((value) => checker.matches(value)).join(","));
