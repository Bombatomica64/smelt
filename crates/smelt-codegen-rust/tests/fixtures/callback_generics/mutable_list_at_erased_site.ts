// Fixture: mutable_list_at_erased_site
// Area: passthrough_ladder
// Guards: mutable composite plus callback pinned at `unknown`, where the substituted type is the erased one.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function tap<T>(xs: T[], cb: (v: T) => boolean): T[] {
  xs.push(xs[0]);
  return xs.filter(cb);
}
export function use1(xs: unknown[]): unknown[] {
  return tap(xs, (v: unknown) => v !== null);
}
