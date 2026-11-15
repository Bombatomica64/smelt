// A record handed to an `unknown` slot is the same object there: the callee's
// write through the value narrowed back is the caller's, the alias the callee
// keeps reads the caller's later writes, and identity survives the round trip.

type Point = { x: number; label: string };

let held: unknown = null;

function retain(value: unknown): void {
  held = value;
}

function moveRight(value: unknown): void {
  // Narrowed back, the value is the caller's record, not a copy of it.
  const point = value as Point;
  point.x += 10;
}

function sameObject(left: unknown, right: unknown): boolean {
  return left === right;
}

function main(): void {
  const point: Point = { x: 1, label: "a" };

  // The callee's write is visible to the caller.
  moveRight(point);
  console.log(point.x);

  // The caller's write is visible through the alias the callee kept.
  retain(point);
  point.x = 55;
  point.label = "moved";
  const kept = held as Point;
  console.log(kept.x, kept.label);

  // Identity survives the round trip, in both directions.
  console.log(kept === point, sameObject(held, point), sameObject(held, kept));

  // A write through the kept alias reaches the record.
  kept.x = 7;
  console.log(point.x);

  // Two records of the same shape stay two objects.
  const other: Point = { x: 7, label: "moved" };
  console.log(sameObject(other, point), sameObject(other, other));

  // What the erased view prints is the record as it is now.
  console.log(JSON.stringify(held), held);
}

main();
