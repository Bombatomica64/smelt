// Fixture: source_class_named_f0_collides_with_generated
// Area: naming_collisions
// Guards: a source class named `F0`, colliding with generated callback generics.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class F0 { x: number = 1; }
export function outer<T>(xs: T[], cb: (v: T) => boolean): number {
  const b = new F0();
  return xs.filter(cb).length + b.x;
}
export function use1(ns: number[]): number { return outer(ns, (v: number) => v > 1); }
