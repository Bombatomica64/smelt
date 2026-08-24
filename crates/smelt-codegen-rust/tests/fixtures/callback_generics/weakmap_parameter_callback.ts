// Fixture: weakmap_parameter_callback
// Area: containers
// Guards: the callback parameter is `WeakMap<T, number>` under an `extends object` bound.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function useWeak<T extends object>(cb: (v: WeakMap<T, number>) => boolean): boolean {
  return true;
}
