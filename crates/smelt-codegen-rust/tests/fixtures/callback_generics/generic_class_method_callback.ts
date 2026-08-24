// Fixture: generic_class_method_callback
// Area: dispatch
// Guards: generic class construction with a composite `T[]` constructor parameter plus a callback method.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Box<T> {
  items: T[];
  constructor(items: T[]) { this.items = items; }
  keep(cb: (v: T) => boolean): T[] { return this.items.filter(cb); }
}
export function use1(ns: number[]): number[] {
  const b = new Box<number>(ns);
  return b.keep((v: number) => v > 1);
}
