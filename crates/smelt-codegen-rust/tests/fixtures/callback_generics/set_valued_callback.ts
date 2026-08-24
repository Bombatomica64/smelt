// Fixture: set_valued_callback
// Area: containers
// Guards: the generic container is a `Set`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: Set<T>, cb: (v: T) => boolean): T[] {
  const out: T[] = [];
  return out;
}
export function use1(s: Set<number>): number[] {
  return pick(s, (v: number) => v > 1);
}
