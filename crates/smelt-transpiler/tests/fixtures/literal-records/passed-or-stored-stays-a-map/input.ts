// An unannotated object literal that is returned or stored stays a map; one
// that is only passed to callees whose parameters hold a view of it is a
// record struct.
//
// A record struct handed to `Record<string, ..>`, `any` or a type parameter is
// seen there through a view of itself: the callee's write reaches the caller's
// object, and a value a callee returns as its `T` keeps the key order the
// callee built it in. `counter` and `unordered` below are structs for that
// reason, and the lines marked (*) used to be the reason they were not: as
// structs they printed `1`, `1` and `zeta,alpha,mid zeta,alpha,mid`.
//
// A literal that leaves its binding (stored in a container, returned), or
// whose record-valued property is replaced, keeps the map lowering it always
// had (`literal_record_uses`, `Disqualifier::Escape`,
// `Disqualifier::PartWrite`): the slot it lands in is not inspected. Lowered
// as a record struct, the line marked (**) prints `first`.
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
  console.log(first.label); // (**)

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
