// Fixture: omitted_optional_callback_via_overload
// Area: passthrough_ladder
// Guards: monomorphization passthrough branch claiming an argument the borrowed-callback branch owns: an overload lets the callback be omitted entirely.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(x: number): number;
export function sink<T>(x: number, cb: (v: number) => T): number;
export function sink<T>(x: number, cb?: (v: number) => T): number {
  const box: unknown = cb ? cb(1) : undefined;
  return 1;
}

export function outer<T>(items: T[]): number {
  return sink(1) + items.length;
}
