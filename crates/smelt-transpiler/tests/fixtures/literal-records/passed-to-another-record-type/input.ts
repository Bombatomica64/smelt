// An unannotated object literal passed to a parameter of ANOTHER record type
// is a record struct when the two record types share, and stays a map when
// they do not.
//
// A parameter typed by an interface or by an object type with properties is a
// different struct in the generated Rust. Two record types SHARE when every
// property the parameter's type declares is a property of the literal's type,
// at the same type: the parameter then holds a view of the caller's record
// (`smelt-codegen-rust/src/structural_views.rs`), and the callee's write
// reaches the caller as it does in JavaScript. `moved` and `first` are such
// literals and are record structs.
//
// A parameter type that does not share (a property at a wider type, an
// optional property the literal does not have) still converts by building
// that other record, a copy. A map is not converted that way: the record the
// callee receives is built from the map and ADOPTS it, so the two stay one
// object. `widened` and `annotated` keep the map lowering
// (`literal_record_uses`, `Disqualifier::Call`).
//
// Lowered as record structs, the lines marked (*) print `1` and `second`.
//
// Every line below is diffed against Node.

type Point = { x: number; label: string };

interface Labelled {
  label: string;
}

type Wide = { x: number | string; label: string };

type Roomy = { x: number; label: string; note?: string };

function moveRight(point: Point): void {
  point.x += 1;
}

function rename(target: Labelled): void {
  target.label = "renamed";
}

function widen(target: Wide): void {
  target.x = "wide";
}

function annotate(target: Roomy): void {
  target.label = "noted";
  target.note = "added";
}

function count(target: Record<string, unknown>): number {
  return Object.keys(target).length;
}

function main(): void {
  // Passed where the parameter is the same shape under its own name.
  const moved = { x: 1, label: "moved" };
  moveRight(moved);
  console.log(moved.x);

  // Passed where the parameter names a subset of its properties.
  const first = { id: 1, label: "first" };
  rename(first);
  console.log(first.label, JSON.stringify(first));

  // Passed where the parameter declares a property at a wider type.
  const widened = { x: 1, label: "widened" };
  widen(widened);
  console.log(widened.x); // (*)

  // Passed where the parameter declares an optional property it lacks.
  const annotated = { x: 2, label: "second" };
  annotate(annotated);
  console.log(annotated.label, JSON.stringify(annotated)); // (*)

  // Control: passed only where the parameter holds a view, this literal is a
  // record struct.
  const viewed = { id: 2, label: "viewed" };
  console.log(count(viewed), viewed.id);
}

main();
