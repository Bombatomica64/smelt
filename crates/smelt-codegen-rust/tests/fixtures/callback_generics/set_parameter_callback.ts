// Fixture: set_parameter_callback
// Area: containers
// Guards: the callback parameter is `Set<T>`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function useSet<T>(cb: (v: Set<T>) => boolean): boolean {
  return true;
}
export function callIt(): boolean {
  return useSet((v: Set<number>) => true);
}
