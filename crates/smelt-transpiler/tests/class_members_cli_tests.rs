//! Class members whose value changes at run time, end to end against Node.
//!
//! - **Live static fields**: a mutable `static` field is storage, not a
//!   constant. `Box.PI = 11`, `Box.PI += 1`, `Box.count++` (in a constructor,
//!   a static method or a closure, through `this` in a static method, and
//!   across modules) and a write through the class used as a value
//!   (`(K: typeof Box) => { K.PI = n }`) all reach the one value every read
//!   sees, through the class or through any value of it. A `static readonly`
//!   field with a literal initializer stays a folded constant.
//! - **Expression static initializers**: `static count = compute()`,
//!   `static items: string[] = []`, `static next = Box.count + 1` and a
//!   `static readonly` one run once, when the class definition runs, in
//!   source order, with `this` bound to the class, and keep their types.
//! - **Enumerable statics**: `Object.keys`/`Object.entries`/`for...in`
//!   (and `Object.values`, a spread, `Object.assign`) over a class value see
//!   its static fields in definition order, without its methods or `name`,
//!   inherited ones only through `for...in`, and every write.
//! - **Self-reading constructor-function fields**: `this.count = this.count +
//!   1` in a prototype method keeps the field the `number` its other writes
//!   give it (a fixed point over the field's own type), instead of erasing it.

mod common;

use std::{fs, path::Path};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in};

/// Writes a one-entry project at `root` around the files of `sources`.
fn write_project(root: &Path, entry: &str, sources: &[(&str, &str)]) -> TestResult {
    fs::create_dir_all(root.join("src"))?;
    for (path, text) in sources {
        fs::write(root.join("src").join(path), text)?;
    }
    fs::write(
        root.join("Smelt.toml"),
        format!(
            "[project]\nname = \"members\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"members\"\nkind = \"program\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// Builds the project at `root` and returns what the generated program prints.
fn build_and_run(root: &Path) -> TestResult<String> {
    smelt_in(root, &["build"])?;
    cargo_run_manifest(&root.join("dist/Cargo.toml"))
}

/// Static fields read and written directly, through class values (a
/// parameter and a kept `const`), from methods, and through a subclass, plus
/// a parameter named like the class that is the parameter.
const STATICS_TS: &str = r#"class Box {
  static PI = 7;
  static label: string = "box";
  static readonly MAX = 3;
  static count = 0;
  v: number;
  constructor(v: number) { this.v = v; Box.count++; }
  static bump(): number { Box.count += 10; return Box.count; }
}
class Sub extends Box { static extra = 1; }
function read(K: typeof Box) { return K.PI; }
function write(K: typeof Box, n: number) { K.PI = n; }
function relabel(K: typeof Box, s: string) { K.label = s; }
function shadow(Box: { PI: number }) { return Box.PI; }
const kept: typeof Box = Box;
console.log(read(Box), Box.PI, Box.MAX, read(Sub));
write(Box, 9);
console.log(read(Box), Box.PI, kept.PI, Sub.PI);
Box.PI = 11;
Box.PI += 1;
console.log(read(Box), kept.PI, Box.PI, read(Sub));
relabel(kept, "crate");
new Box(1); new Box(2);
console.log(Box.label, kept.label, Box.count, Box.bump(), Sub.extra, shadow({ PI: 99 }));
write(kept, 20);
console.log(Box.PI, read(Box), kept.PI);
"#;

/// What Node prints for [`STATICS_TS`].
const STATICS_OUTPUT: &str = "7 7 3 7\n9 9 9 9\n12 12 12 12\ncrate crate 2 12 1 99\n20 20 20\n";

/// Mutable static fields are live storage shared by the class and its values.
#[test]
fn static_fields_are_live_through_class_values() -> TestResult {
    let sources = [("main.ts", STATICS_TS)];
    ensure_node_agrees(&sources, "main.ts", STATICS_OUTPUT, "live statics")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &STATICS_OUTPUT.to_owned(), "the live-statics program's output")?;
    // Type preservation: the storage keeps the field's own type.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(generated.contains("::std::cell::Cell<f64> = const { ::std::cell::Cell::new(7.0) }"), "`Box.PI` is an `f64` cell")
}

/// The class a module exports, mutated by its importer: an expression
/// assignment, `++` in a closure, a static method through the class name and
/// through `this`, and a write through a class value.
const COUNTER_TS: &str = r#"export class Counter {
  static total = 0;
  static name2 = "c";
  static add(n: number): number { Counter.total += n; return Counter.total; }
  static selfAdd(n: number): number { this.total += n; return this.total; }
}
"#;

/// The importer of [`COUNTER_TS`].
const COUNTER_MAIN_TS: &str = r#"import { Counter } from "./lib.ts";
const y = (Counter.total = 5);
const inc = () => { Counter.total++; };
inc(); inc();
console.log(y, Counter.total, Counter.add(3), Counter.selfAdd(2));
function viaValue(C: typeof Counter) { C.total = C.total * 2; return C.total; }
console.log(viaValue(Counter), Counter.total, Counter.name2);
"#;

/// What Node prints for [`COUNTER_MAIN_TS`].
const COUNTER_OUTPUT: &str = "5 7 10 12\n24 24 c\n";

/// A static field mutated across modules is one value.
#[test]
fn static_fields_are_shared_across_modules() -> TestResult {
    let sources = [("main.ts", COUNTER_MAIN_TS), ("lib.ts", COUNTER_TS)];
    ensure_node_agrees(&sources, "main.ts", COUNTER_OUTPUT, "cross-module statics")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &COUNTER_OUTPUT.to_owned(), "the cross-module statics program's output")
}

/// A constructor function whose prototype method reads and writes its own
/// field through an untyped `this`.
const SELF_READING_FIELD_TS: &str = r#"function Point(this: any, x: number) {
  this.count = 1;
  if (x > 0) { this.mixed = 2; } else { this.mixed = "neg"; }
  this.fromParam = x;
}
Point.prototype.bump = function (this: any) { this.count = this.count + 1; return this.count; };
// @ts-ignore
const p = new Point(1);
// @ts-ignore
const q = new Point(-1);
console.log(p.count, p.bump(), p.bump(), p.count + 0.5, q.mixed, p.mixed, p.fromParam);
"#;

/// What Node prints for [`SELF_READING_FIELD_TS`].
const SELF_READING_FIELD_OUTPUT: &str = "1 2 3 3.5 neg 2 1\n";

/// A self-reading constructor-function field keeps its `number` type.
#[test]
fn self_reading_constructor_function_field_stays_typed() -> TestResult {
    let sources = [("main.ts", SELF_READING_FIELD_TS)];
    ensure_node_agrees(&sources, "main.ts", SELF_READING_FIELD_OUTPUT, "self-reading field")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &SELF_READING_FIELD_OUTPUT.to_owned(), "the self-reading-field program's output")?;
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(generated.contains("    count: f64,"), "the fixed point keeps `count: f64`")
}

/// The storage declaration of the static `CLASS_FIELD` (upper-cased, as
/// the global is named), or an empty string.
fn static_storage_line<'a>(generated: &'a str, field: &str) -> &'a str {
    let needle = format!("static SMELT_GLOBAL_{field}_");
    generated.lines().find(|line| line.contains(&needle)).unwrap_or_default()
}

/// Static fields initialized by expressions: side effects in source order
/// (around the module statements between the classes), a static reading an
/// earlier one, `this` in an initializer (the class; a subclass's own `this`
/// reads the inherited static), a static method called from one, a `static
/// readonly` evaluated once, and array, Map and object statics mutated later.
const STATIC_INITIALIZERS_TS: &str = r#"function log(tag: string): number { console.log("init", tag); return tag.length; }
console.log("before");
class Box {
  static PI = 7;
  static first = log("first");
  static next = Box.first + 1;
  static doubled = this.PI * 2;
  static made = this.make(4);
  static readonly LIMIT = Box.make(log("limit"));
  static items: string[] = [];
  static cache = new Map<string, number>();
  static conf: { depth: number; tag: string } = { depth: 1, tag: "c" };
  static make(n: number): number { return n * 10; }
}
console.log("between");
class Sub extends Box { static extra = this.PI + log("extra"); }
console.log("after", Box.first, Box.next, Box.doubled, Box.made, Box.LIMIT, Sub.extra);
Box.items.push("a"); Box.items.push("b");
Box.cache.set("k", 5); Box.cache.set("j", Box.cache.size);
Box.conf.depth += 8;
Box.first = 0;
console.log(Box.items.join("+"), Box.items.length, Box.cache.get("k"), Box.cache.get("j"), Box.conf.depth, Box.conf.tag, Box.first, Box.next);
"#;

/// What Node prints for [`STATIC_INITIALIZERS_TS`].
const STATIC_INITIALIZERS_OUTPUT: &str =
    "before\ninit first\ninit limit\nbetween\ninit extra\nafter 5 6 14 40 50 12\na+b 2 5 1 9 c 0 6\n";

/// Expression static initializers run once at class definition, in source
/// order, and their storage keeps the initializer's precise type.
#[test]
fn static_initializers_run_at_class_definition() -> TestResult {
    let sources = [("main.ts", STATIC_INITIALIZERS_TS)];
    ensure_node_agrees(&sources, "main.ts", STATIC_INITIALIZERS_OUTPUT, "static initializers")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &STATIC_INITIALIZERS_OUTPUT.to_owned(), "the static-initializers program's output")?;
    // Type preservation: each storage cell has the initializer's own type,
    // and `this` in an initializer is the class, read statically rather than
    // through the dynamic `this` channel.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    for (field, ty) in [
        ("BOX_NEXT", "::std::cell::Cell<f64>"),
        ("BOX_DOUBLED", "::std::cell::Cell<f64>"),
        ("BOX_LIMIT", "::std::cell::Cell<f64>"),
        ("BOX_ITEMS", "::std::cell::RefCell<SmeltList<JsString>>"),
        ("BOX_CACHE", "::std::cell::RefCell<SmeltJsMap<JsString, f64>>"),
        ("SUB_EXTRA", "::std::cell::Cell<f64>"),
    ] {
        ensure(static_storage_line(&generated, field).contains(ty), format!("`{field}` is stored as `{ty}`"))?;
    }
    let conf = static_storage_line(&generated, "BOX_CONF");
    ensure(conf.contains("RefCell<SmeltRecord") && !conf.contains("SmeltUnknown"), format!("`Box.conf` keeps its declared struct type: {conf}"))?;
    ensure(!generated.contains("smelt_this()"), "`this` in a static initializer is the class, not the dynamic `this`")
}

/// A class value enumerated: own static fields in definition order without
/// methods (`Object.keys`, `Object.entries`), inherited ones through
/// `for...in` only, a write through the class value and a mutation of an
/// array static both visible, and `Object.values`, a spread and
/// `Object.assign` (onto the class, and copying a subclass's own statics
/// into a record or an erased object) agreeing.
const CLASS_KEYS_TS: &str = r#"class Base {
  static a = 1;
  static readonly b = "two";
  static c: number[] = [3];
  static m(): number { return 0; }
}
class Child extends Base { static d = Base.a + 10; static n(): number { return 1; } }
function keys(K: typeof Base) { return Object.keys(K); }
function entries(K: typeof Base) { return JSON.stringify(Object.entries(K)); }
function forIn(K: typeof Base) { const out: string[] = []; for (const k in K) out.push(k); return out.join(","); }
function bump(K: typeof Base) { K.a = K.a + 5; }
console.log(keys(Base).join(","), "|", keys(Child).join(","));
console.log(forIn(Base), "|", forIn(Child));
console.log(entries(Base), entries(Child));
function intoErased(target: unknown, K: typeof Base) { return Object.assign(target as object, K); }
console.log(JSON.stringify(Object.assign({}, Child)), JSON.stringify({ ...Child }), JSON.stringify(Object.values(Child)), JSON.stringify(intoErased({ z: 0 }, Child)));
bump(Base); Base.c.push(4);
console.log(entries(Base), JSON.stringify(Object.values(Base)), JSON.stringify({ ...Base }));
Object.assign(Base, { a: 100 });
console.log(Base.a, Child.a, entries(Base));
"#;

/// What Node prints for [`CLASS_KEYS_TS`].
const CLASS_KEYS_OUTPUT: &str = concat!(
    "a,b,c | d\n",
    "a,b,c | d,a,b,c\n",
    "[[\"a\",1],[\"b\",\"two\"],[\"c\",[3]]] [[\"d\",11]]\n",
    "{\"d\":11} {\"d\":11} [11] {\"z\":0,\"d\":11}\n",
    "[[\"a\",6],[\"b\",\"two\"],[\"c\",[3,4]]] [6,\"two\",[3,4]] {\"a\":6,\"b\":\"two\",\"c\":[3,4]}\n",
    "100 100 [[\"a\",100],[\"b\",\"two\"],[\"c\",[3,4]]]\n",
);

/// Static fields are the enumerable own properties of a class value.
#[test]
fn class_values_enumerate_their_static_fields() -> TestResult {
    let sources = [("main.ts", CLASS_KEYS_TS)];
    ensure_node_agrees(&sources, "main.ts", CLASS_KEYS_OUTPUT, "class value keys")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &CLASS_KEYS_OUTPUT.to_owned(), "the class-value keys program's output")
}

/// A write through a class value, by name and by computed key, in a program
/// that also builds a `Proxy`: there every erased property write goes through
/// the runtime's generic keyed store (`smelt_index_assign`), which must still
/// reach the class's static storage.
const PROXY_STATICS_TS: &str = r#"class Box { static PI = 7; static tags: string[] = ["a"]; }
const p = new Proxy({ x: 1 } as Record<string, number>, { get: (t, k) => (t[k as string] ?? 0) + 1 });
function write(K: typeof Box, n: number) { K.PI = n; }
function keyed(K: typeof Box, key: "PI", n: number) { K[key] = n; }
write(Box, 9);
console.log(Box.PI, p.x, JSON.stringify(Object.entries(Box)));
keyed(Box, "PI", 12);
console.log(Box.PI, Object.keys(Box).join(","));
"#;

/// What Node prints for [`PROXY_STATICS_TS`].
const PROXY_STATICS_OUTPUT: &str = "9 2 [[\"PI\",9],[\"tags\",[\"a\"]]]\n12 PI,tags\n";

/// Writes through a class value stay live when proxies route erased writes.
#[test]
fn class_value_writes_stay_live_beside_a_proxy() -> TestResult {
    let sources = [("main.ts", PROXY_STATICS_TS)];
    ensure_node_agrees(&sources, "main.ts", PROXY_STATICS_OUTPUT, "statics beside a proxy")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &PROXY_STATICS_OUTPUT.to_owned(), "the proxy-statics program's output")?;
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(generated.contains("smelt_index_assign(&mut"), "the write goes through the generic keyed store")
}
