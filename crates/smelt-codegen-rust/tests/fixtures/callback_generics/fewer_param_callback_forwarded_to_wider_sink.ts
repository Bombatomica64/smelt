// Fixture: fewer_param_callback_forwarded_to_wider_sink
// Area: passthrough_ladder
// Guards: a 1-parameter callback forwarded where a 2-parameter callback is declared; the adapter must widen, not re-pin.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(cb: (x: number, y: number) => T): number {
  const box: unknown = cb(1, 2);
  return 1;
}
export function outer<T>(make: (x: number) => T, items: T[]): number {
  return sink(make) + items.length;
}
