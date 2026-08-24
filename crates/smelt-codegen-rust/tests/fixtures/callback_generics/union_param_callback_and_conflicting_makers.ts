// Fixture: union_param_callback_and_conflicting_makers
// Area: site_pinning
// Guards: a `T | string` callback parameter alongside a two-maker pinning site.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(cb: (value: T | string) => boolean): boolean {
  return cb as unknown as boolean;
}

export function attemptTwo<T>(make: () => T, other: () => T): T[] {
  return [make(), other()];
}

export function conflict(): number[] {
  return attemptTwo(() => 1, () => 2);
}
