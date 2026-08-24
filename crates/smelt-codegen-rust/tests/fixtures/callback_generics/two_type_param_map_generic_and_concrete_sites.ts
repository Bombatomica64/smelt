// Fixture: two_type_param_map_generic_and_concrete_sites
// Area: adapter_substitution
// Guards: `<T, U>` map called once from a generic caller and once concretely.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAll<T, U>(xs: T[], mapper: (item: T) => U): U[] {
  const out: U[] = [];
  for (const x of xs) {
    out.push(mapper(x));
  }
  return out;
}
export function outer<T>(items: T[]): T[] {
  return mapAll(items, (item: T) => item);
}
export function outerConcrete(items: number[]): string[] {
  return mapAll(items, (item: number) => item.toString());
}
