// Fixture: default_callback_parameter_type_param_return
// Area: callback_shape
// Guards: a defaulted T-returning callback parameter.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function withCb<T>(x: number, make: () => T = (() => 0 as unknown as T)): T[] {
  return [make()];
}
export function call1(): unknown[] {
  return withCb(1) as unknown[];
}
export function call2(): string[] {
  return withCb(1, () => "a");
}
