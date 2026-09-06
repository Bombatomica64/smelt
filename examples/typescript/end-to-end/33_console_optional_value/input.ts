// Printing an `Optional<T>` shows the value inside it, never a Rust wrapper.
//
// `console.log` on a `string | undefined` used to print Rust's `Some("ada")` /
// `None`, a shape no JavaScript runtime produces. The present arm now renders
// the inner value exactly as `console.log` renders that type on its own, and
// the absent arm prints the source language's own word for absence:
// `undefined` here, `None` for a Python `print` (both frontends lower to the
// same builtin, so the spelling travels with the call site).
function labelOf(name?: string): string | undefined {
  return name;
}

console.log(labelOf("ada"));
console.log(labelOf());

const scores = new Map<string, number>([["a", 1]]);
console.log(scores.get("a"));
console.log(scores.get("z"));

const explicit: string | undefined = "set";
console.log(explicit);
console.log(labelOf("ada"), labelOf(), explicit);
