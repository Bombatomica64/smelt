// Fixture: forwarded_generic_callback_across_type_params
// Area: dispatch
// Guards: a `U`-callback forwarded into a `T`-callee that calls it.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(cb: (v: T) => boolean): boolean {
  return cb(undefined as unknown as T);
}
export function outer<U>(cb: (v: U) => boolean, xs: U[]): boolean {
  return inner(cb) && xs.length > 0;
}
export function top(): boolean {
  return outer((v: number) => v > 0, [1, 2]);
}
