// Fixture: unresolved_type_param_at_callback_free_site
// Area: passthrough_ladder
// Guards: the caller's own `T` is unrelated to the callee's `T`; the site pins `T` from the callback return only.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(cb: (x: number) => T): number {
  const box: unknown = cb(1);
  return 1;
}

export function outer<T>(items: T[]): number {
  return sink((x: number) => 1) + items.length;
}
