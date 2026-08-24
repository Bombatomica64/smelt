// Fixture: higher_order_callback_parameter
// Area: callback_shape
// Guards: the callback's own parameter is a callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function apply<T>(x: T, cb: (f: (v: T) => boolean) => boolean): boolean {
  return cb((v: T) => true);
}
export function useHigher(n: number): boolean {
  return apply(n, (f: (v: number) => boolean) => f(1));
}
