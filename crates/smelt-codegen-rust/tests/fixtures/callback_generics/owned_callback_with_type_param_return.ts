// Fixture: owned_callback_with_type_param_return
// Area: callback_shape
// Guards: an owned T-returning callback returned by value.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function owned<T>(cb: (v: number) => T): (v: number) => T {
  return cb;
}
export function call1(): string {
  return owned((v: number) => "a")(1);
}
