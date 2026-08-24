// Fixture: map_valued_callback
// Area: containers
// Guards: the generic container is a `Map`, and the callback runs inside `forEach`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(m: Map<string, T>, cb: (v: T) => boolean): T[] {
  const out: T[] = [];
  m.forEach((v: T) => { if (cb(v)) { out.push(v); } });
  return out;
}
export function use1(m: Map<string, number>): number[] {
  return pick(m, (v: number) => v > 1);
}
