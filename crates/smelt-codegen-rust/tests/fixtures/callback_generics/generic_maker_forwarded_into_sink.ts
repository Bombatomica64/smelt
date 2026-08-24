// Fixture: generic_maker_forwarded_into_sink
// Area: passthrough_ladder
// Guards: a caller-generic maker forwarded into a generic sink: neither side may claim the argument twice.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(cb: (x: number) => T): number {
  const box: unknown = cb(1);
  return 1;
}

export function outer<T>(make: (x: number) => T, items: T[]): number {
  return sink(make) + items.length;
}
