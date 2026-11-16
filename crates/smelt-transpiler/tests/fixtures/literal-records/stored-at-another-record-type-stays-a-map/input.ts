// An unannotated object literal stored in, or returned at, a slot of ANOTHER
// record type stays a map, and so does one whose record-valued property is
// replaced.
//
// Where a literal lands when it leaves its binding (an array or object
// literal, an assignment, a `return`) is not inspected, so every such literal
// keeps the map lowering (`literal_record_uses`, `Disqualifier::Escape`). The
// case that needs it is a slot of a record type the literal's type does not
// share with (a property at a wider type, an optional property the literal
// lacks, a class): a struct stored there is converted to that struct, a copy,
// while a map is adopted by the record built from it and stays the caller's
// object. A slot whose record type shares holds a view of the struct
// (`smelt-codegen-rust/src/structural_views.rs`) and would print the same
// either way; it is the slot's type that this rule does not look at.
//
// Lowered as record structs, the lines marked (*) print `1`, `kept` and
// `2 new`.
//
// Every line below is diffed against Node.

type Wide = { x: number | string; label: string };
type Roomy = { x: number; label: string; note?: string };

class Tagged {
  depth = 2;
  tag = "new";
}

function stored(): void {
  const kept = { x: 1, label: "kept" };
  const all: Wide[] = [kept];
  all[0].x = 5;
  console.log(kept.x); // (*)
}

function assigned(): void {
  const kept = { x: 1, label: "kept" };
  const holder: { point: Roomy | null } = { point: null };
  holder.point = kept;
  if (holder.point) {
    holder.point.label = "renamed";
  }
  console.log(kept.label); // (*)
}

function returned(): Wide {
  const made = { x: 7, label: "made" };
  return made;
}

function replaced(): void {
  // A record-valued property replaced by a value of another record type
  // (`Disqualifier::PartWrite`), here a class instance: as a struct,
  // `outer.inner` would hold a copy.
  const outer = { name: "outer", inner: { depth: 1, tag: "in" } };
  const replacement = new Tagged();
  outer.inner = replacement;
  replacement.depth = 3;
  console.log(outer.inner.depth, outer.inner.tag); // (*)
}

stored();
assigned();
console.log(returned().x);
replaced();
