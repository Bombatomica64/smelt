// Fixture: optional_union_callback_narrowed_then_forwarded
// Area: passthrough_ladder
// Guards: a callback typed `F | undefined` narrowed by a guard, then forwarded: passthrough vs borrowed-callback must agree after narrowing.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(cb: (v: number) => T): number {
  const box: unknown = cb(1);
  return 1;
}
export function outer<T>(maybe: ((v: number) => T) | undefined, items: T[]): number {
  if (maybe !== undefined) {
    return sink(maybe) + items.length;
  }
  return 0;
}
