// Fixture: function_item_and_arrow_for_nullary_maker
// Area: site_pinning
// Guards: a nullary `() => T` supplied once by a function item and once by an arrow.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function attempt<T>(make: () => T): T[] {
  return [make()];
}
function makeThing(): string { return "a"; }
export function runItem(): string[] {
  return attempt(makeThing);
}
export function runArrow(): number[] {
  return attempt(() => 1);
}
