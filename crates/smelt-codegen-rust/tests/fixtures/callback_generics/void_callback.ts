// Fixture: void_callback
// Area: dispatch
// Guards: a `void`-returning callback driven by a for-of loop.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function each<T>(xs: T[], cb: (v: T) => void): void {
  for (const x of xs) { cb(x); }
}
export function use1(ns: number[]): void {
  each(ns, (v: number) => { console.log(v); });
}
