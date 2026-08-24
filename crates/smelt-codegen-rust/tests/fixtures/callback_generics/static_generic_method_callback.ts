// Fixture: static_generic_method_callback
// Area: dispatch
// Guards: a static generic method taking a callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Util {
  static pick<T>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
}
export function use1(ns: number[]): number[] {
  return Util.pick(ns, (v: number) => v > 1);
}
