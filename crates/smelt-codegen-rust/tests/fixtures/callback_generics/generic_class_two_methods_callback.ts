// Fixture: generic_class_two_methods_callback
// Area: dispatch
// Guards: a generic class whose two methods pin `T` differently.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Holder<T> {
  items: T[];
  constructor(items: T[]) { this.items = items; }
  keep(cb: (v: T) => boolean): T[] { return this.items.filter(cb); }
  transform(cb: (v: T) => T): T[] { return this.items.map(cb); }
}
export function use1(ns: number[]): number[] {
  const b = new Holder<number>(ns);
  return b.keep((v: number) => v > 1);
}
export function use2(ns: number[]): number[] {
  const b = new Holder<number>(ns);
  return b.transform((v: number) => v + 1);
}
