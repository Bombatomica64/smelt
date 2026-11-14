// A record passed at a type parameter the callee reads properties through is
// the same object there. `T extends { x: number }` is erased in the callee
// (it reads `p.x`), so the record crosses as an erased view of itself.

type Point = { x: number; label: string };

function bump<T extends { x: number }>(target: T): T {
  target.x += 1;
  return target;
}

function main(): void {
  const point: Point = { x: 1, label: "a" };
  const returned = bump(point);
  console.log(point.x, returned.x, returned === point);
  returned.label = "b";
  point.x = 10;
  console.log(point.label, returned.x);
  console.log(bump(bump(point)).x, point.x);
}

main();
