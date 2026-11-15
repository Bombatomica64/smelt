// A record handed to a `Record<string, unknown>` slot is the same object
// there: a write through either is read through the other, the two are one
// identity, and a property the dictionary adds or deletes belongs to the
// object, which enumerates it in JavaScript's own-key order from then on.

type Point = { x: number; label: string };

let kept: Record<string, unknown> = {};

function setX(target: Record<string, unknown>, value: number): void {
  target["x"] = value;
}

function keep(target: Record<string, unknown>): Record<string, unknown> {
  kept = target;
  return target;
}

function reshape(target: Record<string, unknown>): void {
  // Add a property the record type does not declare, delete a declared one,
  // then set it again: it is a new property and enumerates last.
  target["tag"] = "new";
  delete target["x"];
  target["x"] = 9;
}

function main(): void {
  const point: Point = { x: 1, label: "a" };

  // The callee's write is visible to the caller.
  setX(point, 100);
  console.log(point.x);

  // The caller's write is visible through the dictionary the callee kept.
  const returned = keep(point);
  point.x = 5;
  point.label = "b";
  console.log(kept["x"], kept["label"], returned["x"]);

  // The dictionary narrowed back is the caller's record.
  const back = returned as unknown as Point;
  console.log(back === point, (kept as unknown) === (point as unknown));

  // Key order after a dynamic add, a delete and a re-add through the view.
  reshape(point);
  console.log(Object.keys(point), Object.keys(kept));
  console.log(point.x, JSON.stringify(point));
  console.log(point);
}

main();
