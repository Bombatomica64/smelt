// An unannotated object literal stored in, or returned at, a slot of ANOTHER
// record type stays a map, and so does one whose record-valued property is
// replaced.
//
// Where a literal lands when it leaves its binding (an array or object
// literal, an assignment, a `return`) is not inspected, so every such literal
// keeps the map lowering (`literal_record_uses`, `Disqualifier::Escape`). The
// case that needs it is a slot of another record type: a struct stored there
// is converted to that struct, a copy, while a map is adopted by the record
// built from it and stays the caller's object.
//
// Lowered as record structs, the lines marked (*) print `1`, `kept` and
// `2 new`.
//
// Every line below is diffed against Node.

type Point = { x: number; label: string };
type Inner = { depth: number; tag: string };

function stored(): void {
  const kept = { x: 1, label: "kept" };
  const all: Point[] = [kept];
  all[0].x = 5;
  console.log(kept.x); // (*)
}

function assigned(): void {
  const kept = { x: 1, label: "kept" };
  const holder: { point: Point | null } = { point: null };
  holder.point = kept;
  if (holder.point) {
    holder.point.label = "renamed";
  }
  console.log(kept.label); // (*)
}

function returned(): Point {
  const made = { x: 7, label: "made" };
  return made;
}

function replaced(): void {
  // A record-valued property replaced by a value of another record type
  // (`Disqualifier::PartWrite`): as a struct, `outer.inner` would hold a copy.
  const outer = { name: "outer", inner: { depth: 1, tag: "in" } };
  const replacement: Inner = { depth: 2, tag: "new" };
  outer.inner = replacement;
  replacement.depth = 3;
  console.log(outer.inner.depth, outer.inner.tag); // (*)
}

stored();
assigned();
console.log(returned().x);
replaced();
