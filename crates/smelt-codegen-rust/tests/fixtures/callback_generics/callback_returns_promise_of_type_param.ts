// Fixture: callback_returns_promise_of_type_param
// Area: adapter_substitution
// Guards: `T` recovered from inside the callback's `Promise` return.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function later<T>(make: (x: number) => Promise<T>): number {
  return 1;
}
export function useIt(): number {
  return later(async (x: number) => "a");
}
