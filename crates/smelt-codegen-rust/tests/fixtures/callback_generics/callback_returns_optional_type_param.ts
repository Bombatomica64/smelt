// Fixture: callback_returns_optional_type_param
// Area: adapter_substitution
// Guards: `T` recovered from a `T | undefined` callback return.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function opt<T>(make: (x: number) => T | undefined): number {
  return 1;
}
export function useIt(): number {
  return opt((x: number) => "a");
}
