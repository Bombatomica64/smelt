// Fixture: mutable_extra_list_param_with_callback
// Area: passthrough_ladder
// Guards: a second, non-generic mutable list argument alongside the monomorphizing one.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function bump<T>(xs: T[], cb: (v: T) => boolean, counter: number[]): T[] {
  counter.push(1);
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  const c: number[] = [];
  return bump(ns, (v: number) => v > 1, c);
}
