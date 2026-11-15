// A record handed to an `any` slot is the same object there: the callee's
// writes, additions and deletions are the caller's, and the alias the callee
// keeps reads the caller's later writes.

type Point = { x: number; label: string };

let held: any = null;

function bump(target: any): void {
  target.x = target.x + 199;
}

function retain(target: any): void {
  held = target;
}

function extend(target: any): void {
  target.extra = true;
  delete target.x;
  target.x = 3;
}

function main(): void {
  const point: Point = { x: 1, label: "a" };

  bump(point);
  console.log(point.x);

  retain(point);
  point.x = 55;
  point.label = "moved";
  console.log(held.x, held.label, held === point);

  held.label = "through any";
  console.log(point.label);

  extend(point);
  console.log(Object.keys(point), point.x, held.extra);
  console.log(JSON.stringify(point));
  console.log(point);

  // A second erasure of the record sees what the first one added.
  const again: any = point;
  console.log(again.extra, Object.keys(again).length, again === held);
}

main();
