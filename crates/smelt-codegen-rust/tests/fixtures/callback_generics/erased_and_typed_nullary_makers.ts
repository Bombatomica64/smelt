// Fixture: erased_and_typed_nullary_makers
// Area: site_pinning
// Guards: erased and typed sites of both a nullary maker and a two-type-param mapper.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function attempt<T>(make: () => T): T[] {
  return [make()];
}

export function runErased(spy: unknown): unknown[] {
  return attempt(spy as () => unknown);
}

export function unionBy<T, U>(arr1: T[], arr2: T[], mapper: (item: T) => U): T[] {
  return arr1.concat(arr2);
}

export function useUnionBy(xs: number[], ys: number[]): number[] {
  return unionBy(xs, ys, (item: number) => item.toString());
}

export function useUnionByErased(xs: unknown[], ys: unknown[], m: unknown): unknown[] {
  return unionBy(xs, ys, m as (item: unknown) => unknown);
}
