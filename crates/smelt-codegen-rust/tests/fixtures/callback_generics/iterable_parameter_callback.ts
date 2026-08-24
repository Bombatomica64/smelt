// Fixture: iterable_parameter_callback
// Area: containers
// Guards: the callback parameter is `Iterable<T>`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function useIter<T>(cb: (v: Iterable<T>) => boolean): boolean {
  return true;
}
export function callIt(): boolean {
  return useIter((v: Iterable<number>) => true);
}
