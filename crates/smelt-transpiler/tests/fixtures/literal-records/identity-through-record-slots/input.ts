// An unannotated object literal is ONE JavaScript object wherever it lands at
// a declared record type: a callee's parameter, an annotated array, a
// `return`, an annotated alias. Every alias reads and writes that object.
//
// A literal whose slots all declare exactly its properties takes that record
// type from the literal on (`flags`, `stored`, `made`, `shared`, `opt`,
// `outer`), as a hand port would declare it. A literal also seen at another
// record type (`labelled`, passed where `Labelled` names a subset of its
// properties), and an annotated record passed to another interface (`point`),
// is linked to the record built there: the two cells are one object
// (`smelt-codegen-rust/src/linked_records.rs`). A spread is a copy and stays
// independent of its source. A literal that stays a dictionary (`counts`,
// whose keys are enumerated) is adopted by the record built from it.
//
// Each line marked (*) printed the value before the write when the literal
// was copied into the slot instead.
//
// Every line below is diffed against Node.

interface Flags { touched: boolean }
interface Touchable { touched: boolean }
interface Labelled { label: string }
interface Opt { a: number; b?: string }
interface Inner { depth: number }
interface Outer { name: string; inner: Inner }
interface Counter { hits: number; misses: number }
type Point = { x: number; label: string };

function touch(f: Flags): void { f.touched = true; }
function untouch(t: Touchable): void { t.touched = false; }
function rename(t: Labelled): void {
  t.label = "renamed";
  // The callee's `Labelled` is the caller's whole object.
  console.log("callee sees", JSON.stringify(t), Object.keys(t).join(","));
}
function setB(o: Opt): void { o.b = "set"; }
function dropB(o: Opt): void { delete o.b; }
function replaceInner(o: Outer): void { o.inner = { depth: 100 }; }
function bump(c: Counter): void { c.hits += 1; }

function make(): Flags {
  const made = { touched: false };
  return made;
}

function passedToAMutatingFunction(): void {
  const flags = { touched: false };
  touch(flags);
  console.log("passed", flags.touched); // (*)
}

function storedInAnArray(): void {
  const stored = { touched: false };
  const all: Flags[] = [stored];
  all[0].touched = true;
  console.log("stored", stored.touched, all[0] === stored); // (*)
  stored.touched = false;
  console.log("stored back", all[0].touched); // (*)
}

function returnedAndMutated(): void {
  const r = make();
  touch(r);
  const again: Flags = r;
  again.touched = false;
  console.log("returned", r.touched, again === r); // (*)
}

function aliasedThroughTwoInterfaces(): void {
  const shared = { touched: false };
  const f: Flags = shared;
  const t: Touchable = shared;
  f.touched = true;
  console.log("two", t.touched, shared.touched, f === t); // (*)
  untouch(shared);
  console.log("two back", f.touched, (t as unknown) === (shared as unknown)); // (*)
}

function subsetParameter(): void {
  const labelled = { label: "x", n: 2 };
  rename(labelled);
  console.log("subset", labelled.label, JSON.stringify(labelled)); // (*)
}

function recordToAnotherInterface(): void {
  const point: Point = { x: 1, label: "point" };
  rename(point);
  point.x = 5;
  console.log("record", point.label, JSON.stringify(point)); // (*)
}

function optionalAddAndDelete(): void {
  const opt = { a: 1 };
  setB(opt);
  console.log("opt", JSON.stringify(opt), Object.keys(opt).join(",")); // (*)
  dropB(opt);
  console.log("opt deleted", JSON.stringify(opt), Object.keys(opt).join(","));
}

function nestedLiteral(): void {
  const outer = { name: "n", inner: { depth: 1 } };
  replaceInner(outer);
  console.log("nested", outer.inner.depth); // (*)
  outer.inner.depth = 7;
  const view: Outer = outer;
  console.log("nested view", view.inner.depth, view === outer);
}

function enumeratedThenPassed(): void {
  // Its keys are enumerated, so it stays a dictionary (of numbers); the
  // `Counter` built for the callee adopts it, and the two stay one object.
  const counts = { hits: 0, misses: 0 };
  for (const key in counts) {
    console.log("key", key);
  }
  bump(counts);
  counts.misses = 3;
  console.log("counts", counts.hits, JSON.stringify(counts)); // (*)
  const held: Counter = counts;
  held.hits = 10;
  console.log("counts held", counts.hits, held === counts); // (*)
}

function spreadStaysIndependent(): void {
  const source = { touched: false, n: 1 };
  const copy = { ...source };
  copy.touched = true;
  touch(source);
  copy.n = 2;
  console.log("spread", source.touched, source.n, copy.touched, copy.n, JSON.stringify(copy));
}

passedToAMutatingFunction();
storedInAnArray();
returnedAndMutated();
aliasedThroughTwoInterfaces();
subsetParameter();
recordToAnotherInterface();
optionalAddAndDelete();
nestedLiteral();
enumeratedThenPassed();
spreadStaysIndependent();
