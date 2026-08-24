// Fixture: async_generic_returns_list
// Area: dispatch
// Guards: an `async` generic returning `Promise<T[]>` built from a maker.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export async function later<T>(make: () => T): Promise<T[]> {
  return [make()];
}
export async function top(): Promise<string[]> {
  return await later(() => "a");
}
