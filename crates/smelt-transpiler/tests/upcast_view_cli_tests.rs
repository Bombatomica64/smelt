//! Widened generic class handles keep the object, end to end against Node.
//!
//! TypeScript lets a generic class instance flow to an instantiation with
//! other type arguments (`const wide: Parser<unknown> = p` for a
//! `Parser<number>`). JavaScript has one object, so `wide === p`, a write
//! through either handle is seen through the other, a list field mutated
//! through `wide` is `p`'s own array, and a method called through `wide` runs
//! on `p` (and dispatches to a subclass override). Smelt used to rebuild a new
//! `Parser<SmeltUnknown>` from the fields: a copy. A widened class
//! (`smelt_codegen_rust::class_widening`) now converts through a view of the
//! same cell held in a placeholder's view slot, and keeps its handle and the
//! direct `Ref<ParserInner<f64>>` path wherever the arguments are known.
//!
//! Each test checks the generated program against Node and the emitted types.

mod common;

use std::{fs, path::Path};

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in,
};

/// Writes a one-file project at `root` around `source`.
fn write_project(root: &Path, source: &str) -> TestResult {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/main.ts"), source)?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"widened\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\nroots = [\"src\"]\n\n\
         [output]\ntarget = \"dist\"\ncrate-name = \"widened\"\nkind = \"program\"\nbuild = false\n",
    )?;
    Ok(())
}

/// Checks `source` against Node's `expected` output, builds it, runs the
/// generated program and checks it prints the same; answers the generated
/// Rust of every module.
fn agrees_with_node(source: &str, expected: &str, what: &str) -> TestResult<String> {
    ensure_node_agrees(&[("main.ts", source)], "main.ts", expected, what)?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, source)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(
        &output,
        &expected.to_owned(),
        format!("{what}: generated Rust stdout\n--- generated\n{output}--- node\n{expected}"),
    )?;
    let mut text = String::new();
    for file in fs::read_dir(root.join("dist/src"))? {
        let path = file?.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&fs::read_to_string(path)?);
            text.push('\n');
        }
    }
    Ok(text)
}

/// The program of the `same_object` test.
const SAME_OBJECT_TS: &str = r#"class Parser<V> {
  last: V | null = null;
  hits = 0;
  items: V[] = [];
  label: string;
  constructor(label: string) { this.label = label; }
  feed(v: V): void { this.last = v; this.hits += 1; this.items.push(v); }
  describe(): string { return `${this.label}:${this.hits}`; }
}
class NumParser extends Parser<number> {
  describe(): string { return `num ${super.describe()}`; }
}
function main(): void {
  const p = new Parser<number>("p");
  const wide: Parser<unknown> = p;
  console.log(wide.last === null, wide.last === undefined, wide === p);
  p.feed(1);
  console.log(wide.hits, wide.last, wide.items.length);
  wide.hits = 5;
  wide.feed(2);
  console.log(p.hits, p.items.join(","), p.last);
  const held = p.items;
  wide.items.push(7);
  wide.items[0] = 9;
  console.log(p.items.join(","), held.length, held === p.items);
  const back = wide as Parser<number>;
  back.hits += 1;
  console.log(back === p, p.hits, wide.hits);
  const n = new NumParser("n");
  const w2: Parser<unknown> = n;
  w2.feed(5);
  console.log(n.hits, w2.describe(), w2 === n, w2 instanceof NumParser);
  const anyP: Parser<any> = p;
  anyP.label = "renamed";
  console.log(p.describe(), anyP.describe());
}
main();
"#;

/// What Node prints for [`SAME_OBJECT_TS`].
const SAME_OBJECT_OUTPUT: &str = concat!(
    "true false true\n",
    "1 1 1\n",
    "6 1,2 2\n",
    "9,2,7 3 true\n",
    "true 7 7\n",
    "1 num n:1 true true\n",
    "renamed:7 renamed:7\n",
);

/// The program of the `generic_code` test.
const GENERIC_CODE_TS: &str = r#"class Box<V> {
  value: V | null = null;
  items: V[] = [];
  count = 0;
  name: string;
  constructor(name: string) { this.name = name; }
  put(v: V): number { this.value = v; this.items.push(v); this.count += 1; return this.items.length; }
  take(): V | null { const v = this.value; this.value = null; return v; }
  label(): string { return `${this.name}#${this.count}`; }
  check(limit: number): number {
    if (this.count > limit) { throw new Error(`${this.name} over ${limit}`); }
    return this.count;
  }
}
class NumBox extends Box<number> {
  label(): string { return `num:${super.label()}`; }
}
function bump<T>(q: Box<T>): number { q.count += 10; return q.items.length; }
function first<T>(q: Box<T>): T | undefined { return q.items[0]; }
function main(): void {
  const a = new Box<number>("a");
  const s = new Box<string>("s");
  const n = new NumBox("n");
  a.put(1);
  s.put("x");
  n.put(2);
  const all: Box<unknown>[] = [a, s, n];
  for (const b of all) { console.log(b.label(), b.count, b.items.length, b.value); }
  console.log(bump(all[0]), a.count, first(all[2]), first(all[1]));
  const t = all[0].take();
  console.log(t, a.value, all[0].value);
  const u: unknown = a;
  const again = u as Box<unknown>;
  again.count = 77;
  console.log(a.count, again === all[0], (u as Box<number>) === a);
  try {
    console.log(all[0].check(100));
    console.log(all[0].check(5));
  } catch (e) {
    console.log((e as Error).message);
  }
}
main();
"#;

/// What Node prints for [`GENERIC_CODE_TS`].
const GENERIC_CODE_OUTPUT: &str = concat!(
    "a#1 1 1 1\n",
    "s#1 1 1 x\n",
    "num:n#1 1 1 2\n",
    "1 11 2 x\n",
    "1 null null\n",
    "77 true true\n",
    "77\n",
    "a over 5\n",
);

/// A `Parser<number>` seen as a `Parser<unknown>`/`Parser<any>` is the same
/// object: identity, writes in both directions, list fields mutated in place,
/// method calls and the subclass override all match Node.
#[test]
fn a_widened_handle_is_the_same_object() -> TestResult {
    let all = agrees_with_node(SAME_OBJECT_TS, SAME_OBJECT_OUTPUT, "widened handle")?;
    // Type preservation: each handle keeps its own instantiation.
    ensure(
        all.contains("let p: Parser<f64>;"),
        "`p` stays a `Parser<f64>`",
    )?;
    ensure(
        all.contains("let mut wide: Parser<SmeltUnknown>;"),
        "`wide` is a `Parser<SmeltUnknown>`",
    )?;
    ensure(
        all.contains("Parser::<SmeltUnknown>::smelt_retarget(p.clone())"),
        "the upcast retargets the same object",
    )?;
    ensure(
        all.contains("Parser::<SmeltUnknown>::smelt_retarget(Parser(n.clone().0.clone()))"),
        "a subclass instance is upcast, then retargeted",
    )?;
    // The handle at known, unchanged arguments keeps the direct record path.
    ensure(
        all.contains("p.0.borrow().hits"),
        "a `Parser<f64>` reads its record directly",
    )?;
    ensure(
        all.contains("wide.smelt_read_hits()") && all.contains("(*wide.smelt_write_items())"),
        "a `Parser<SmeltUnknown>` goes through the accessors",
    )?;
    ensure(
        !all.contains("smelt_struct_value.0.borrow().last"),
        "no field-wise copy of the instance",
    )?;
    // A placeholder exists only at an erased instantiation, so every other one
    // skips the slot test at compile time.
    ensure(
        all.contains("smelt_self == ::std::any::TypeId::of::<Parser<SmeltUnknown>>()"),
        "the view test is a constant per instantiation",
    )
}

/// Generic code, an array of a wider instantiation and an erased round trip
/// all see the same objects, and a method that throws through a view throws
/// the original's error.
#[test]
fn generic_code_and_erasure_see_the_same_object() -> TestResult {
    let all = agrees_with_node(GENERIC_CODE_TS, GENERIC_CODE_OUTPUT, "widened generic code")?;
    ensure(
        all.contains("SmeltList<Box_<SmeltUnknown>>"),
        "the array keeps its `Box<unknown>` element type",
    )?;
    ensure(
        all.contains("(mut q: Box_<T>) -> f64"),
        "the generic function keeps its `Box<T>` parameter",
    )?;
    ensure(
        all.contains("q.smelt_write_count()"),
        "a handle at a type parameter may be a view: it goes through the accessors",
    )?;
    ensure(
        all.contains("smelt_origin::<Box_<f64>>()"),
        "an erased `Box<number>` is taken back as the same box at other arguments",
    )
}

/// The program of the `virtual_slot` test.
const VIRTUAL_SLOT_TS: &str = r#"type Result<T> = { value: T };
abstract class Parser<T> {
  abstract parse(input: string): Result<T>;
}
class ForwardingParser<T> extends Parser<T> {
  delegate: Parser<T>;
  constructor(delegate: Parser<T>) { super(); this.delegate = delegate; }
  parse(input: string): Result<T> { return this.delegate.parse(input); }
}
class YearParser extends Parser<number> {
  parse(input: string): Result<number> { return { value: input.length + 2000 }; }
}
function main(): void {
  const inner = new YearParser();
  const fwd = new ForwardingParser(inner);
  const parser: Parser<any> = fwd;
  console.log(parser.parse("abc").value, parser === fwd, parser instanceof ForwardingParser);
  const list: Parser<unknown>[] = [inner, fwd];
  for (const p of list) { console.log(JSON.stringify(p.parse("xy"))); }
}
main();
"#;

/// What Node prints for [`VIRTUAL_SLOT_TS`].
const VIRTUAL_SLOT_OUTPUT: &str = concat!(
    "2003 true true\n",
    "{\"value\":2002}\n",
    "{\"value\":2002}\n"
);

/// An abstract method called through a wider handle reaches the subclass's
/// override: the view hands out the source's own virtual slot, adapted.
#[test]
fn a_virtual_slot_through_a_widened_handle_reaches_the_override() -> TestResult {
    let all = agrees_with_node(VIRTUAL_SLOT_TS, VIRTUAL_SLOT_OUTPUT, "widened virtual slot")?;
    ensure(
        all.contains("let parser: Parser<SmeltUnknown>;"),
        "`parser` is a `Parser<SmeltUnknown>`",
    )?;
    ensure(
        all.contains("parser = Parser::<SmeltUnknown>::smelt_retarget("),
        "the upcast keeps the object",
    )?;
    ensure(
        all.contains(
            "fn smelt_read_parse_view(&self) -> ::std::rc::Rc<dyn Fn(JsString) -> Result_<T>>",
        ),
        "the view adapts the source's `parse` slot",
    )
}

/// The program of the `leaf_subclass` test.
const LEAF_SUBCLASS_TS: &str = r#"abstract class Setter {
  priority = 0;
  abstract describe(): string;
}
class ValueSetter<Value> extends Setter {
  value: Value;
  constructor(value: Value, priority: number) { super(); this.value = value; this.priority = priority; }
  describe(): string { return `set ${String(this.value)} @${this.priority}`; }
}
class Plain extends Setter {
  describe(): string { return "plain"; }
}
function main(): void {
  const v = new ValueSetter<number>(5, 2);
  const wide: ValueSetter<unknown> = v;
  const setters: Setter[] = [new Plain(), wide];
  for (const s of setters) { console.log(s.describe(), s.priority); }
  const up: Setter = wide;
  up.priority = 9;
  console.log(v.priority, up === v, wide.value, up instanceof ValueSetter);
}
main();
"#;

/// What Node prints for [`LEAF_SUBCLASS_TS`].
const LEAF_SUBCLASS_OUTPUT: &str = concat!("plain 0\n", "set 5 @2 2\n", "9 true 5 true\n");

/// A widened generic leaf subclass of a polymorphic base (date-fns'
/// `ValueSetter<Value> extends Setter`): a placeholder upcast to the base is
/// the original object, through the view.
#[test]
fn a_widened_leaf_subclass_upcasts_to_its_base_as_the_same_object() -> TestResult {
    let all = agrees_with_node(
        LEAF_SUBCLASS_TS,
        LEAF_SUBCLASS_OUTPUT,
        "widened leaf subclass",
    )?;
    ensure(
        all.contains("struct ValueSetter<Value>(::std::rc::Rc<::std::cell::RefCell<ValueSetterInner<Value>>>);"),
        "the leaf keeps its concrete handle",
    )?;
    ensure(
        all.contains("Setter(match wide.clone().smelt_view() { Some(smelt_view) => smelt_view.smelt_as_Setter(), None => wide.clone().0.clone() })"),
        "a placeholder upcasts through its view",
    )
}

/// The program of the `by_reference_slot` test.
const BY_REFERENCE_SLOT_TS: &str = r"type Flags = { touched: boolean };
abstract class Step<V> {
  priority = 1;
  run(date: Date, flags: Flags, value: V): { at: number; ok: boolean } {
    const ok = this.validate(value);
    const out = this.set(date, flags, value);
    return { at: out.getTime(), ok };
  }
  protected validate(_value: V): boolean { return true; }
  protected abstract set(date: Date, flags: Flags, value: V): Date;
}
class YearStep extends Step<number> {
  protected set(date: Date, flags: Flags, value: number): Date {
    flags.touched = true;
    date.setUTCFullYear(value);
    return date;
  }
  protected validate(value: number): boolean { return value > 1900; }
}
function main(): void {
  const steps: Step<any>[] = [new YearStep()];
  for (const step of steps) {
    const flags: Flags = { touched: false };
    const date = new Date(0);
    const r = step.run(date, flags, 2001);
    console.log(r.at, r.ok, flags.touched, date.getUTCFullYear(), step.priority);
    console.log(JSON.stringify(step.run(new Date(0), flags, 1800)));
  }
}
main();
";

/// What Node prints for [`BY_REFERENCE_SLOT_TS`].
const BY_REFERENCE_SLOT_OUTPUT: &str = concat!(
    "978307200000 true true 2001 1\n",
    "{\"at\":-5364662400000,\"ok\":false}\n"
);

/// A method that runs on a placeholder (it has no erased adapter) reads a
/// virtual slot whose subclass fixed the parameter (`set(.., value: &V)`, the
/// declaring level's by-reference ABI): the view adapts the source's slot,
/// converting the borrowed argument (date-fns' `Parser.run` over `this.set`).
#[test]
fn a_by_reference_slot_through_a_widened_handle_reaches_the_override() -> TestResult {
    let all = agrees_with_node(
        BY_REFERENCE_SLOT_TS,
        BY_REFERENCE_SLOT_OUTPUT,
        "widened by-reference slot",
    )?;
    ensure(
        all.contains("fn smelt_read_set_view(&self) -> ::std::rc::Rc<dyn Fn(SmeltDate, Flags, &V) -> SmeltDate>"),
        "the view adapts the by-reference `set` slot",
    )?;
    ensure(
        all.contains("SmeltList<Step<SmeltUnknown>>"),
        "the array keeps its `Step<any>` element type",
    )
}
