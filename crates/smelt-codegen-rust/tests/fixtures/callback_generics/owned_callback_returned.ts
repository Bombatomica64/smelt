// Fixture: owned_callback_returned
// Area: callback_shape
// Guards: the borrowed callback is returned by value, so it must be owned.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function outer<T>(xs: T[], cb: (v: T) => boolean): (v: T) => boolean { return cb; }
export function use1(ns: number[]): boolean { return outer(ns, (v: number) => v > 1)(2); }
