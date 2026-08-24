// Fixture: mutable_list_forwarded_through_hop
// Area: passthrough_ladder
// Guards: mutable composite plus callback forwarded one generic hop; both branches see the argument.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function tap<T>(xs: T[], cb: (v: T) => boolean): T[] {
  xs.push(xs[0]);
  return xs.filter(cb);
}
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return tap(xs, cb);
}
export function use1(ns: number[]): number[] {
  return outer(ns, (v: number) => v > 1);
}
