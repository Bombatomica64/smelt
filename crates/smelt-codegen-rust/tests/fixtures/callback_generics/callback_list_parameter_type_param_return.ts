// Fixture: callback_list_parameter_type_param_return
// Area: callback_shape
// Guards: an array of T-returning callbacks.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function nested<T>(cbs: ((v: number) => T)[]): T[] {
  return cbs.map((c) => c(1));
}
export function call1(): string[] {
  return nested([(v: number) => "a"]);
}
