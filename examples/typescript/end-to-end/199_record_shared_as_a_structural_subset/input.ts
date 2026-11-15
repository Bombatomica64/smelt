// A record handed to a parameter that names only some of its properties is the
// same object there.
//
// `{ x: number }` accepts any object with a numeric `x`. The caller's `Point`
// has more properties, so the generated Rust converts at the call; that
// conversion used to copy, and `nudge` below moved a copy. The parameter is
// now a view of the caller's record.
//
// Every line below is diffed against Node.

type Point = { x: number; y: number };

let last: { x: number } = { x: 0 };

function nudge(target: { x: number }): void {
  target.x += 1;
}

function remember(target: { x: number }): { x: number } {
  last = target;
  return target;
}

function main(): void {
  const point: Point = { x: 1, y: 2 };

  // The callee's write is visible to the caller.
  nudge(point);
  nudge(point);
  console.log(point.x, point.y);

  // The caller's write is visible through the alias the callee kept.
  const returned = remember(point);
  point.x = 40;
  console.log(last.x, returned.x);

  // A write through the kept alias reaches the record.
  last.x += 2;
  console.log(point.x);

  // Another record with the same property is another object.
  const other: Point = { x: 100, y: 0 };
  nudge(other);
  console.log(other.x, point.x, last.x);
}

main();
