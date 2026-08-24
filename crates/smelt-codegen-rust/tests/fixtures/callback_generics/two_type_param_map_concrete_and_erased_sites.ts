// Fixture: two_type_param_map_concrete_and_erased_sites
// Area: adapter_substitution
// Guards: `<T, U>` map called concretely and with an erased `unknown` callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapIt<T, U>(xs: T[], mapper: (item: T) => U): U[] {
  const out: U[] = [];
  for (const x of xs) { out.push(mapper(x)); }
  return out;
}
export function top(): string[] {
  return mapIt([1, 2], (n: number) => n.toString());
}
export function topErased(xs: unknown[], m: (v: unknown) => unknown): unknown[] {
  return mapIt(xs, m);
}
