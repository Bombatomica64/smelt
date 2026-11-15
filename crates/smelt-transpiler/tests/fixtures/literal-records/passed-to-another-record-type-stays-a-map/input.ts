// An unannotated object literal passed to a parameter of ANOTHER record type
// stays a map.
//
// A record struct handed to a slot that holds a view of it (`unknown`, `any`,
// a type parameter, `Record<K, V>`) is the caller's object there. A parameter
// typed by an interface or by an object type with properties is a different
// struct in the generated Rust, and a struct converted to another struct is
// built anew: the callee would write a copy. The map is not converted that
// way: the record the callee receives is built from the map and ADOPTS it, so
// the two stay one object. So a literal passed at such a parameter keeps the
// map lowering (`literal_record_uses`, `Disqualifier::Call`), and the callee's
// write reaches the caller as it does in JavaScript.
//
// Lowered as a record struct, the lines marked (*) print `1` and `first`.
//
// Every line below is diffed against Node.

type Point = { x: number; label: string };

interface Labelled {
  label: string;
}

function moveRight(point: Point): void {
  point.x += 1;
}

function rename(target: Labelled): void {
  target.label = "renamed";
}

function count(target: Record<string, unknown>): number {
  return Object.keys(target).length;
}

function main(): void {
  // Passed where the parameter is the same shape under its own name.
  const moved = { x: 1, label: "moved" };
  moveRight(moved);
  console.log(moved.x); // (*)

  // Passed where the parameter names a subset of its properties.
  const first = { id: 1, label: "first" };
  rename(first);
  console.log(first.label); // (*)

  // Control: passed only where the parameter holds a view, this literal is a
  // record struct.
  const viewed = { id: 2, label: "viewed" };
  console.log(count(viewed), viewed.id);
}

main();
