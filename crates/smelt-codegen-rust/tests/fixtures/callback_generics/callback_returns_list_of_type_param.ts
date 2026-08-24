// Fixture: callback_returns_list_of_type_param
// Area: adapter_substitution
// Guards: `T` recovered from inside the callback's list return.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function build<T>(make: (x: number) => T[]): number {
  return make(1).length;
}
export function useIt(): number {
  return build((x: number) => ["a", "b"]);
}
