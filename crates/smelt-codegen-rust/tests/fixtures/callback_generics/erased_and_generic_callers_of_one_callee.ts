// Fixture: erased_and_generic_callers_of_one_callee
// Area: dispatch
// Guards: one callee reached from both a generic caller and an `unknown` caller.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] { return inner(xs, cb); }
export function bad(xs: unknown[], cb: (v: unknown) => boolean): unknown[] { return inner(xs, cb); }
export function use1(ns: number[]): number[] { return outer(ns, (v: number) => v > 1); }
