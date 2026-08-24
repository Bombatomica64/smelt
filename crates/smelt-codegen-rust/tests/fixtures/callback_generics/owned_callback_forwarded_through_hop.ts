// Fixture: owned_callback_forwarded_through_hop
// Area: callback_shape
// Guards: an owned-returning callback forwarded through a generic hop.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sink<T>(xs: T[], cb: (v: T) => boolean): (v: T) => boolean {
  return cb;
}
export function outer<T>(xs: T[], cb: (v: T) => boolean): (v: T) => boolean {
  return sink(xs, cb);
}
export function use1(ns: number[]): boolean { return outer(ns, (v: number) => v > 1)(3); }
