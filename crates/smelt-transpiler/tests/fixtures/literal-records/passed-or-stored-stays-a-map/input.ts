// An unannotated object literal that is passed to a function, returned or
// stored stays a map.
//
// A record struct handed to a slot of another type (`Record<string, ..>`,
// `any`, a type parameter, another record) is converted, and the conversion is
// a copy: a write made through that view does not reach the caller's object,
// and a value a callee returns as its `T` is rebuilt in the struct's key order
// whatever order the callee built it in. The map is passed as itself. A
// callee's body is not inspected, so every argument position other than a
// known by-value observer keeps the map lowering the literal always had, and
// so does a literal that leaves its binding (`literal_record_uses`,
// `Disqualifier::Call`, `Disqualifier::Escape`, `Disqualifier::PartWrite`).
//
// Lowered as record structs, the lines marked (*) print `1`, `1`,
// `zeta,alpha,mid zeta,alpha,mid` and `first`: the stale value and the
// unsorted order.
//
// Every line below is diffed against Node.

function setField(target: Record<string, unknown>, key: string, value: unknown): void {
  target[key] = value;
}

function setAnything(target: any): void {
  target.count = 200;
}

function sortedKeys<T extends Record<string, any>>(object: T): T {
  const result = {} as Record<string, any>;
  for (const key of Object.keys(object).sort()) {
    result[key] = object[key];
  }
  return result as T;
}

function passed(): void {
  const counter = { count: 1, label: "counter" };
  setField(counter, "count", 100);
  console.log(counter.count); // (*)
  setAnything(counter);
  console.log(counter.count); // (*)

  const unordered = { zeta: 1, alpha: "first", mid: true };
  const ordered = sortedKeys(unordered);
  console.log(Object.keys(ordered).join(","), Object.keys(unordered).join(",")); // (*)
}

function makeRecord(): Record<string, unknown> {
  const made = { id: 7, label: "made" };
  return made;
}

function stored(): void {
  const first = { id: 1, label: "first" };
  const all: Record<string, unknown>[] = [first];
  all[0]["label"] = "renamed";
  console.log(first.label); // (*)

  const made = makeRecord();
  made["extra"] = true;
  console.log(Object.keys(made).join(","));

  const outer = { name: "outer", inner: { depth: 1, tag: "in" } };
  const replacement: { depth: number; tag: string } = { depth: 2, tag: "new" };
  outer.inner = replacement;
  replacement.depth = 3;
  console.log(outer.inner.depth, outer.inner.tag);
}

passed();
stored();
