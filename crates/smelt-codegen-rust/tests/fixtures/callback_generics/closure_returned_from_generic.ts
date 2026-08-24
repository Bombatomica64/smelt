// Fixture: closure_returned_from_generic
// Area: callback_shape
// Guards: the generic returns a freshly built closure over `T`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function make<T>(x: T): (v: T) => boolean {
  return (v: T) => true;
}
export function useRet(n: number): boolean {
  return make(n)(n);
}
