// Fixture: default_callback_parameter
// Area: callback_shape
// Guards: a defaulted callback parameter, called both with and without it.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(x: T, cb: (v: T) => boolean = () => true): T {
  return cb(x) ? x : x;
}
export function useDefault(): number {
  return pick(1);
}
export function useGiven(): number {
  return pick(2, (v: number) => v > 1);
}
