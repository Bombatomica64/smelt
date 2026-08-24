// Fixture: concrete_callback_sunk_into_method
// Area: dispatch
// Guards: a non-generic callback sunk into a method call from inside a generic caller.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Holder {
  run(cb: (v: number) => boolean): boolean { return cb(1); }
}
export function outer<T>(xs: T[], b: Holder, cb: (v: number) => boolean): T[] {
  b.run(cb);
  return xs;
}
export function use1(ts: string[]): string[] { return outer(ts, new Holder(), (v: number) => v > 1); }
