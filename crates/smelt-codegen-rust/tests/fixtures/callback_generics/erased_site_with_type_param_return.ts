// Fixture: erased_site_with_type_param_return
// Area: site_pinning
// Guards: a T-returning callback pinned entirely at `unknown`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAll<T>(xs: T[], cb: (v: T) => T): T[] {
  return xs.map(cb);
}
export function use1(xs: unknown[]): unknown[] {
  return mapAll(xs, (v: unknown) => v);
}
