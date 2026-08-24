// Fixture: source_class_named_box_with_callback_sink
// Area: dispatch
// Guards: the same shape with the source class named `Box`, which collides with generated naming.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Box {
  run(cb: (v: number) => boolean): boolean { return cb(1); }
}
export function outer<T>(xs: T[], b: Box, cb: (v: number) => boolean): T[] {
  b.run(cb);
  return xs;
}
export function use1(ts: string[]): string[] { return outer(ts, new Box(), (v: number) => v > 1); }
