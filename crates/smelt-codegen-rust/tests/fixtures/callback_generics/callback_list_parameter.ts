// Fixture: callback_list_parameter
// Area: callback_shape
// Guards: the parameter is an array of callbacks, so the adapter sits inside a composite.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cbs: ((v: T) => boolean)[]): T[] {
  return xs.filter(cbs[0]);
}
export function useNested(ns: number[]): number[] {
  return pick(ns, [(v: number) => v > 1]);
}
