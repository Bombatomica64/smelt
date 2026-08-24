// Fixture: map_keyed_by_type_param
// Area: containers
// Guards: a `Map<T, T>` built inside a generic that is forwarded one hop.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(xs: T[]): T[] {
  const seen = new Map<T, T>();
  for (const item of xs) {
    seen.set(item, item);
  }
  return xs;
}

export function outer<T>(xs: T[]): T[] {
  return inner(xs);
}

export function top(): number[] {
  return outer([1, 2, 3]);
}
