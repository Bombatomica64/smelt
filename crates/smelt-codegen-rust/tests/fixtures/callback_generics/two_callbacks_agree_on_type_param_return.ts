// Fixture: two_callbacks_agree_on_type_param_return
// Area: adapter_substitution
// Guards: two T-returning callbacks at one site that agree; both adapters must substitute the
// same way, where the conflicting-arm fixtures cover the disagreeing case.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function twoCb<T>(a: (v: number) => T, b: (v: number) => T): T[] {
  return [a(1), b(2)];
}
export function call1(): string[] {
  return twoCb((v: number) => "a", (v: number) => "b");
}
