// Fixture: rest_callbacks_parameter
// Area: callback_shape
// Guards: a rest parameter of callbacks.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function outer<T>(xs: T[], ...cbs: ((v: T) => boolean)[]): T[] { return xs.filter(cbs[0]); }
export function use1(ns: number[]): number[] { return outer(ns, (v: number) => v > 1); }
