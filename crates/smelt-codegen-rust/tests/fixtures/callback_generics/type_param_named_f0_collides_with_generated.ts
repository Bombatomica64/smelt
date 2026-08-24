// Fixture: type_param_named_f0_collides_with_generated
// Area: naming_collisions
// Guards: a source type parameter literally named `F0`, colliding with generated callback generics.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function both<F0, T>(xs: T[], ys: F0[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[], ss: string[]): number[] { return both(ns, ss, (v: number) => v > 1); }
