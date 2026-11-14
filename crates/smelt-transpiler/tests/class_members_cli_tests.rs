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
//! - **Per-evaluation subclasses and generics**: a class extending a class
//!   declared in a function (beside it, or at module level through the value
//!   the function returns) inherits that evaluation's statics live and
//!   shadows them on write; a generic per-evaluation class keeps its type
//!   parameters.
//! - **Uninitialized statics in typed slots**: `static x: number;` read before
//!   any write is `undefined` wherever TypeScript types it `number`.
//! - **Live erased containers**: a static `Map`/`Set` reached through an
//!   erased class value is the static itself.
//! - **Static `this`**: `this` in a static method is the class.

mod common;

use std::{fs, path::Path};

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in,
};

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
    ensure_eq(
        &output,
        &STATICS_OUTPUT.to_owned(),
        "the live-statics program's output",
    )?;
    // Type preservation: the storage keeps the field's own type.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(
        generated.contains("::std::cell::Cell<f64> = const { ::std::cell::Cell::new(7.0) }"),
        "`Box.PI` is an `f64` cell",
    )
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
    ensure_eq(
        &output,
        &COUNTER_OUTPUT.to_owned(),
        "the cross-module statics program's output",
    )
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
    ensure_node_agrees(
        &sources,
        "main.ts",
        SELF_READING_FIELD_OUTPUT,
        "self-reading field",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &SELF_READING_FIELD_OUTPUT.to_owned(),
        "the self-reading-field program's output",
    )?;
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(
        generated.contains("    count: f64,"),
        "the fixed point keeps `count: f64`",
    )
}

/// The storage declaration of the static `CLASS_FIELD` (upper-cased, as
/// the global is named), or an empty string.
fn static_storage_line<'a>(generated: &'a str, field: &str) -> &'a str {
    let needle = format!("static SMELT_GLOBAL_{field}_");
    generated
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_default()
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
const STATIC_INITIALIZERS_OUTPUT: &str = "before\ninit first\ninit limit\nbetween\ninit extra\nafter 5 6 14 40 50 12\na+b 2 5 1 9 c 0 6\n";

/// Expression static initializers run once at class definition, in source
/// order, and their storage keeps the initializer's precise type.
#[test]
fn static_initializers_run_at_class_definition() -> TestResult {
    let sources = [("main.ts", STATIC_INITIALIZERS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        STATIC_INITIALIZERS_OUTPUT,
        "static initializers",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &STATIC_INITIALIZERS_OUTPUT.to_owned(),
        "the static-initializers program's output",
    )?;
    // Type preservation: each storage cell has the initializer's own type,
    // and `this` in an initializer is the class, read statically rather than
    // through the dynamic `this` channel.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    for (field, ty) in [
        ("BOX_NEXT", "::std::cell::Cell<f64>"),
        ("BOX_DOUBLED", "::std::cell::Cell<f64>"),
        ("BOX_LIMIT", "::std::cell::Cell<f64>"),
        ("BOX_ITEMS", "::std::cell::RefCell<SmeltList<JsString>>"),
        (
            "BOX_CACHE",
            "::std::cell::RefCell<SmeltJsMap<JsString, f64>>",
        ),
        ("SUB_EXTRA", "::std::cell::Cell<f64>"),
    ] {
        ensure(
            static_storage_line(&generated, field).contains(ty),
            format!("`{field}` is stored as `{ty}`"),
        )?;
    }
    let conf = static_storage_line(&generated, "BOX_CONF");
    ensure(
        conf.contains("RefCell<SmeltRecord") && !conf.contains("SmeltUnknown"),
        format!("`Box.conf` keeps its declared struct type: {conf}"),
    )?;
    ensure(
        !generated.contains("smelt_this()"),
        "`this` in a static initializer is the class, not the dynamic `this`",
    )
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
    ensure_eq(
        &output,
        &CLASS_KEYS_OUTPUT.to_owned(),
        "the class-value keys program's output",
    )
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
    ensure_node_agrees(
        &sources,
        "main.ts",
        PROXY_STATICS_OUTPUT,
        "statics beside a proxy",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &PROXY_STATICS_OUTPUT.to_owned(),
        "the proxy-statics program's output",
    )?;
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(
        generated.contains("smelt_index_assign(&mut"),
        "the write goes through the generic keyed store",
    )
}

/// Static fields declared without an initializer: a read before the first
/// write is `undefined` (also through `typeof`, arithmetic, `++`, a class
/// value and `Object.keys`), a later write is read back, and an optional
/// static `Map` is created on first use.
const UNINITIALIZED_STATICS_TS: &str = r#"class Box {
  static x: number;
  static label: string;
  static count: number;
  static cache?: Map<string, number>;
  static bump(): number { Box.count = (Box.count ?? 0) + 1; return Box.count; }
  static remember(key: string, value: number): number {
    if (!Box.cache) Box.cache = new Map();
    Box.cache.set(key, value);
    return Box.cache.size;
  }
}
function read(K: typeof Box) { return K.x; }
console.log(typeof Box.x, Box.x, Box.x === undefined, typeof Box.label, read(Box), Box.cache === undefined);
console.log(Box.x + 1, Box.x * 2, "v" + Box.x, Box.x > 3, Object.keys(Box).join(","));
const before = Box.count++;
console.log(before, Box.count);
Box.x = 5;
Box.label = "hi";
Box.count = 1;
Box.count++;
console.log(typeof Box.x, Box.x, Box.x + 1, Box.label.length, Box.count, read(Box), Box.bump());
console.log(Box.remember("a", 1), Box.remember("b", 2), Box.cache?.get("b"), Box.cache === undefined);
"#;

/// What Node prints for [`UNINITIALIZED_STATICS_TS`].
const UNINITIALIZED_STATICS_OUTPUT: &str = concat!(
    "undefined undefined true undefined undefined true\n",
    "NaN NaN vundefined false x,label,count,cache\n",
    "NaN NaN\n",
    "number 5 6 2 2 5 3\n",
    "1 2 2 false\n",
);

/// A static without an initializer is `undefined` until written, and its
/// storage keeps the declared type as `Option<T>`.
#[test]
fn uninitialized_statics_read_undefined_until_written() -> TestResult {
    let sources = [("main.ts", UNINITIALIZED_STATICS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        UNINITIALIZED_STATICS_OUTPUT,
        "uninitialized statics",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &UNINITIALIZED_STATICS_OUTPUT.to_owned(),
        "the uninitialized-statics program's output",
    )?;
    // Type preservation: the storage is the declared type widened with
    // `undefined`, never an erased value.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    for (field, ty) in [
        (
            "BOX_X",
            "::std::cell::RefCell<Option<f64>> = ::std::cell::RefCell::new(None)",
        ),
        (
            "BOX_LABEL",
            "::std::cell::RefCell<Option<JsString>> = ::std::cell::RefCell::new(None)",
        ),
        (
            "BOX_CACHE",
            "::std::cell::RefCell<Option<SmeltJsMap<JsString, f64>>> = ::std::cell::RefCell::new(None)",
        ),
    ] {
        ensure(
            static_storage_line(&generated, field).contains(ty),
            format!("`{field}` is stored as `{ty}`"),
        )?;
    }
    Ok(())
}

/// A class declared in a factory whose statics read the factory's
/// parameter: two evaluations are two classes, each with its own statics,
/// mutated by its own static methods (through `this` and the class name),
/// its constructor and its instances' methods, and `instanceof` tells the
/// evaluations apart.
const FACTORY_STATICS_TS: &str = r#"function make(n: number) {
  class Box {
    static size = n;
    static count = 0;
    static doubled = this.size * 2;
    static history: number[] = [];
    v: number;
    constructor(v: number) { this.v = v; Box.count++; Box.history.push(v); }
    describe(): string { return `${this.v}/${Box.size}/${Box.count}`; }
    static grow(): number { this.size += 1; return this.size; }
    static twice(): number { return Box.size * 2; }
    static build(v: number): Box { return new this(v); }
  }
  const reset = () => { Box.size = n; };
  return { Box, reset };
}
const one = make(1);
const two = make(10);
const a = new one.Box(5);
const b = two.Box.build(6);
new two.Box(7);
console.log(one.Box.size, two.Box.size, one.Box.grow(), one.Box.grow(), two.Box.grow(), one.Box.twice(), two.Box.twice());
console.log(a.describe(), b.describe(), one.Box.count, two.Box.count, one.Box.doubled, two.Box.doubled, one.Box.history.join("+"), two.Box.history.join("+"));
console.log(a instanceof one.Box, a instanceof two.Box, b instanceof two.Box, b instanceof one.Box);
one.reset();
console.log(one.Box.size, two.Box.size, a.describe());
"#;

/// What Node prints for [`FACTORY_STATICS_TS`].
const FACTORY_STATICS_OUTPUT: &str = concat!(
    "1 10 2 3 11 6 22\n",
    "5/3/1 6/11/2 1 2 2 20 5 6+7\n",
    "true false true false\n",
    "1 11 5/1/1\n",
);

/// Each evaluation of a class declaration that reads the enclosing
/// function's locals has its own statics, typed all the way down.
#[test]
fn factory_classes_have_per_evaluation_statics() -> TestResult {
    let sources = [("main.ts", FACTORY_STATICS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        FACTORY_STATICS_OUTPUT,
        "per-evaluation statics",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &FACTORY_STATICS_OUTPUT.to_owned(),
        "the per-evaluation statics program's output",
    )?;
    // Type preservation: the statics frame is a typed reference class whose
    // fields keep their types, the instance holds it, and no static is a
    // module global (one storage for every evaluation).
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    let source = fs::read_to_string(root.join("dist/src/source_main.rs")).unwrap_or_default();
    let all = format!("{generated}\n{source}");
    ensure(all.contains("struct Box__smelt_statics(::std::rc::Rc<::std::cell::RefCell<Box__smelt_staticsInner>>)"), "the frame is a reference class")?;
    ensure(
        all.contains("    size: f64,") && all.contains("    history: SmeltList<f64>,"),
        "the frame's fields keep their types",
    )?;
    ensure(
        all.contains("    __smelt_statics: Box__smelt_statics,"),
        "each instance holds its frame",
    )?;
    ensure(
        !all.contains("SMELT_GLOBAL_BOX_SIZE"),
        "no evaluation-shared storage",
    )
}

/// An anonymous class expression returned by a factory, reached only
/// through erased class values: static reads and writes, static methods
/// through `this` (including `new this`), construction, `instanceof`,
/// `typeof`, `name` and `Object.keys` of the class and of an instance.
const FACTORY_CLASS_EXPRESSION_TS: &str = r#"function make(n: number, tag: string) {
  return class {
    static size = n;
    static label: string = tag + "!";
    static hits: number;
    v: number;
    constructor(v: number) { this.v = v; }
    static grow(): number { this.size += 1; return this.size; }
    static make(v: number) { return new this(v); }
  };
}
const A = make(1, "a");
const B = make(10, "b");
const a = new A(5);
const b = B.make(6);
console.log(A.size, B.size, A.grow(), A.grow(), B.grow(), A.label, B.label, A.hits, typeof B.hits);
console.log(a instanceof A, a instanceof B, b instanceof B, b instanceof A, a.v, b.v);
function erased(K: unknown) { return K; }
const EA = erased(A) as typeof A;
console.log(EA.size, Object.keys(A).join(","), Object.keys(a).join(","), typeof A, JSON.stringify(A.name));
const c = new EA(7);
EA.size = 40;
console.log(c instanceof A, c instanceof B, c.v, EA.grow(), A.size);
"#;

/// What Node prints for [`FACTORY_CLASS_EXPRESSION_TS`].
const FACTORY_CLASS_EXPRESSION_OUTPUT: &str = concat!(
    "1 10 2 3 11 a! b! undefined undefined\n",
    "true false true false 5 6\n",
    "3 size,label,hits v function \"\"\n",
    "true false 7 41 41\n",
);

/// A per-evaluation anonymous class expression behaves as a class value at
/// every erased boundary.
#[test]
fn factory_class_expressions_are_class_values_per_evaluation() -> TestResult {
    let sources = [("main.ts", FACTORY_CLASS_EXPRESSION_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        FACTORY_CLASS_EXPRESSION_OUTPUT,
        "per-evaluation class expressions",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &FACTORY_CLASS_EXPRESSION_OUTPUT.to_owned(),
        "the per-evaluation class-expression program's output",
    )
}

/// The text of every generated Rust file of the project at `root`.
fn generated_text(root: &Path) -> TestResult<String> {
    let main = fs::read_to_string(root.join("dist/src/main.rs"))?;
    let source = fs::read_to_string(root.join("dist/src/source_main.rs")).unwrap_or_default();
    Ok(format!("{main}\n{source}"))
}

/// The body of the generated `struct {name} { .. }`.
fn struct_text<'a>(generated: &'a str, name: &str) -> TestResult<&'a str> {
    impl_text(generated, &format!("struct {name} {{"))
}

/// The text from `header` to the end of its item (the next line that is a
/// lone closing brace).
fn impl_text<'a>(generated: &'a str, header: &str) -> TestResult<&'a str> {
    let start = generated
        .find(header)
        .ok_or_else(|| format!("`{header}` is not in the generated Rust"))?;
    let rest = &generated[start..];
    let end = rest
        .find("\n}\n")
        .map_or(rest.len(), |end| end.saturating_add(3));
    Ok(&rest[..end])
}

/// Per-evaluation subclasses declared beside their base in a function: inherited
/// statics read live from the base until the subclass writes its own (through
/// `Sub.grow()` with `this` = `Sub`), a three-level chain, the base name
/// inside a subclass method, `new this()` in an inherited static method,
/// in-place mutation of an inherited container static (which leaves it
/// inherited), and `Object.keys` listing the statics the subclass owns.
const FACTORY_SUBCLASS_TS: &str = r#"function run(n: number): string {
  class Base {
    static size = n;
    static made = 0;
    v: number;
    constructor(v: number) { this.v = v; Base.made++; }
    static grow(): number { this.size += 1; return this.size; }
    static baseSize(): number { return Base.size; }
    static create(v: number): Base { return new this(v); }
    describe(): string { return `B${this.v}/${Base.size}`; }
  }
  class Sub extends Base {
    static tag = "t" + n;
    describe(): string { return `S${this.v}/${Sub.size}/${Sub.tag}`; }
  }
  class Deep extends Sub {
    static extra = n * 3;
    w: number;
    constructor(v: number, w: number) { super(v); this.w = w; }
    static deepGrow(): number { Deep.extra += Deep.size; return Deep.extra; }
  }
  const out: string[] = [];
  out.push(`${Sub.size} ${Deep.size} ${Sub.grow()} ${Base.size} ${Sub.size} ${Sub.baseSize()} ${Deep.size}`);
  Base.size = 100;
  out.push(`${Sub.size} ${Deep.size} ${Deep.grow()} ${Sub.size} ${Deep.deepGrow()}`);
  const d = new Deep(4, 5);
  const c = Sub.create(9);
  out.push(`${d.describe()} ${d.w} ${c.describe()} ${Base.made} ${d instanceof Base} ${d instanceof Sub}`);
  out.push(`${Object.keys(Sub).join(",")}|${Object.keys(Deep).join(",")}`);
  return out.join("\n");
}
console.log(run(1));
console.log(run(10));
function makeLog(n: number) {
  class Base {
    static history: number[] = [n];
    static counts: Map<string, number> = new Map();
    static log(v: number): number { this.history.push(v); this.counts.set("k", v); return this.history.length; }
  }
  class Sub extends Base {
    static note(): string { return Sub.history.join("+") + "/" + Base.history.join("+") + "/" + Sub.counts.get("k"); }
  }
  const logged = [Base.log(1), Sub.log(2), Sub.note(), Base.history.length].join(" ");
  Base.history = [99];
  return `${logged} ${Sub.history.join("+")} ${Object.keys(Sub).join(",")}`;
}
console.log(makeLog(7));
console.log(makeLog(8));
"#;

/// What Node prints for [`FACTORY_SUBCLASS_TS`].
const FACTORY_SUBCLASS_OUTPUT: &str = concat!(
    "1 1 2 1 2 1 2\n",
    "2 2 3 2 6\n",
    "S4/2/t1 5 S9/2/t1 2 true true\n",
    "tag,size|extra,size\n",
    "10 10 11 10 11 10 11\n",
    "11 11 12 11 42\n",
    "S4/11/t10 5 S9/11/t10 2 true true\n",
    "tag,size|extra,size\n",
    "2 3 7+1+2/7+1+2/2 3 99 \n",
    "2 3 8+1+2/8+1+2/2 3 99 \n",
);

/// A subclass of a per-evaluation class is per-evaluation too, and resolves its
/// statics through its base exactly as JavaScript's constructor prototype chain.
#[test]
fn per_evaluation_subclasses_inherit_and_shadow_statics() -> TestResult {
    let sources = [("main.ts", FACTORY_SUBCLASS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        FACTORY_SUBCLASS_OUTPUT,
        "per-evaluation subclasses",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &FACTORY_SUBCLASS_OUTPUT.to_owned(),
        "the per-evaluation subclasses program's output",
    )?;
    let all = generated_text(root)?;
    // Type preservation: the subclass frame links to its base frame by its
    // concrete type, shadows each inherited static at its own type, keeps no
    // copy of the base's storage, and no frame holds an erased value.
    let sub_frame = struct_text(&all, "Sub__smelt_staticsInner")?;
    ensure(
        sub_frame.contains("__smelt_base_1: Base__smelt_statics,")
            && sub_frame.contains("__smelt_shadow_size_1: Option<f64>,")
            && sub_frame.contains("tag: JsString,"),
        "the subclass frame links to the base frame and shadows `size` as `Option<f64>`",
    )?;
    ensure(
        !sub_frame.contains("    size: f64,"),
        "the subclass frame stores no copy of the base's `size`",
    )?;
    ensure(
        all.contains("__smelt_statics_2: Deep__smelt_statics,"),
        "a `Deep` instance holds its own frame",
    )?;
    for frame in [
        "Base__smelt_staticsInner",
        "Sub__smelt_staticsInner",
        "Deep__smelt_staticsInner",
    ] {
        ensure(
            !struct_text(&all, frame)?.contains("SmeltUnknown"),
            format!("`{frame}` holds no erased value"),
        )?;
    }
    Ok(())
}

/// A factory returning a per-evaluation class and its subclass inside an erased
/// record: construction, inherited and own statics, static methods and
/// `instanceof` across two evaluations, all through erased class values.
const ERASED_SUBCLASS_TS: &str = r"function make(n: number) {
  class Base {
    static size = n;
    static count = 0;
    v: number;
    constructor(v: number) { this.v = v; Base.count++; }
    describe(): string { return `${this.v}/${Base.size}/${Base.count}`; }
    static grow(): number { this.size += 1; return this.size; }
  }
  class Sub extends Base {
    static extra = n * 100;
    w: number;
    constructor(v: number, w: number) { super(v); this.w = w; Sub.extra++; }
    describe(): string { return super.describe() + `:${this.w}:${Sub.extra}:${Sub.size}`; }
  }
  return { Base, Sub };
}
const one = make(1);
const two = make(10);
const s = new one.Sub(5, 6);
const t = new two.Sub(7, 8);
console.log(s.describe(), t.describe(), one.Base.count, two.Base.count, one.Sub.grow(), one.Base.size, two.Sub.size);
console.log(s instanceof one.Base, s instanceof two.Base, s instanceof one.Sub, t instanceof two.Sub);
";

/// What Node prints for [`ERASED_SUBCLASS_TS`].
const ERASED_SUBCLASS_OUTPUT: &str = concat!(
    "5/1/1:6:101:1 7/10/1:8:1001:10 1 1 2 1 10\n",
    "true false true true\n",
);

/// Erased class values of a per-evaluation subclass answer statics and
/// `instanceof` along the evaluation's own chain.
#[test]
fn per_evaluation_subclasses_through_erased_class_values() -> TestResult {
    let sources = [("main.ts", ERASED_SUBCLASS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        ERASED_SUBCLASS_OUTPUT,
        "erased per-evaluation subclasses",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &ERASED_SUBCLASS_OUTPUT.to_owned(),
        "the erased per-evaluation subclasses program's output",
    )?;
    let all = generated_text(root)?;
    ensure(
        all.contains("struct Sub__smelt_statics(::std::rc::Rc<::std::cell::RefCell<Sub__smelt_staticsInner>>)"),
        "the subclass frame is a reference class",
    )
}

/// Module-level classes extending a class a function returns: through a
/// binding (`extends B1`) and through the call itself (`extends make(20)`).
const MODULE_SUBCLASS_TS: &str = r#"function make(n: number) {
  class Base {
    static size = n;
    v: number;
    constructor(v: number) { this.v = v; }
    static grow(): number { this.size += 1; return this.size; }
    describe(): string { return `${this.v}/${Base.size}`; }
  }
  return Base;
}
const B1 = make(1);
class Ext extends B1 {
  static label = "ext";
  describe(): string { return `ext:${super.describe()}:${Ext.size}`; }
}
class Ext2 extends make(20) {}
const e = new Ext(3);
console.log(e.describe(), Ext.size, Ext.grow(), B1.size, Ext.size, Ext.label, Ext2.size, new Ext2(4).describe());
console.log(e instanceof B1, e instanceof Ext, new Ext2(1) instanceof B1);
"#;

/// What Node prints for [`MODULE_SUBCLASS_TS`].
const MODULE_SUBCLASS_OUTPUT: &str =
    concat!("ext:3/1:1 1 2 1 2 ext 20 4/20\n", "true true false\n",);

/// A module-level subclass of a per-evaluation class keeps its frame in a module
/// global, linked to the base the `extends` clause evaluated to.
#[test]
fn module_level_subclasses_of_returned_classes() -> TestResult {
    let sources = [("main.ts", MODULE_SUBCLASS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        MODULE_SUBCLASS_OUTPUT,
        "module-level subclasses",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &MODULE_SUBCLASS_OUTPUT.to_owned(),
        "the module-level subclasses program's output",
    )?;
    let all = generated_text(root)?;
    ensure(
        all.contains("static SMELT_GLOBAL_EXT__SMELT_STATICS_VALUE_1: ::std::cell::RefCell<Ext__smelt_statics>"),
        "the module-level subclass frame is a typed module global",
    )
}

/// Generic per-evaluation classes: instantiated by argument inference and by
/// explicit type arguments, a generic static method, and construction through an
/// erased class value.
const GENERIC_FACTORY_TS: &str = r#"function make(tag: string) {
  class Box<T> {
    static label = tag;
    static count = 0;
    value: T;
    constructor(value: T) { this.value = value; Box.count++; }
    get(): T { return this.value; }
    describe(): string { return `${Box.label}:${String(this.value)}:${Box.count}`; }
    static of<U>(value: U): Box<U> { return new Box(value); }
  }
  return Box;
}
const A = make("a");
const B = make("b");
const a = new A(1);
const s = new A<string>("x");
const b = B.of(true);
console.log(a.get() + 1, s.get().toUpperCase(), b.get(), a.describe(), s.describe(), b.describe(), A.count, B.count);
console.log(a instanceof A, a instanceof B, b instanceof B);
function makeExpr(tag: string) {
  return class Box<T> {
    static label = tag;
    value: T;
    constructor(value: T) { this.value = value; }
    get(): T { return this.value; }
  };
}
const GA = makeExpr("a");
function erasedValue(K: unknown) { return K; }
const EA = erasedValue(GA) as typeof GA;
const e = new EA<number>(7);
console.log(e.get(), EA.label, e instanceof GA, Object.keys(GA).join(","));
"#;

/// What Node prints for [`GENERIC_FACTORY_TS`].
const GENERIC_FACTORY_OUTPUT: &str = concat!(
    "2 X true a:1:2 a:x:2 b:true:1 2 1\n",
    "true false true\n",
    "7 a true label\n",
);

/// A per-evaluation class keeps its type parameters as Rust generics.
#[test]
fn generic_per_evaluation_classes_keep_type_parameters() -> TestResult {
    let sources = [("main.ts", GENERIC_FACTORY_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        GENERIC_FACTORY_OUTPUT,
        "generic per-evaluation classes",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &GENERIC_FACTORY_OUTPUT.to_owned(),
        "the generic per-evaluation classes program's output",
    )?;
    let all = generated_text(root)?;
    // Type preservation: the owner stays generic and each construction keeps
    // its instantiation.
    ensure(
        all.contains("struct Box_<T> {"),
        "the owner is a generic struct",
    )?;
    ensure(
        all.contains(": Box_<f64>;") && all.contains(": Box_<JsString>;"),
        "`new A(1)` is a `Box_<f64>` and `new A<string>(..)` a `Box_<JsString>`",
    )
}

/// Statics declared without an initializer at a type that does not admit
/// `undefined`, read before any write into typed slots: locals (annotated,
/// inferred, reassigned), parameters of each primitive type, and a
/// per-evaluation class's static.
const ABSENT_STATIC_TS: &str = r#"class Box { static x: number; static s: string; static flag: boolean; static maybe?: number; }
const y: number = Box.x;
console.log(y, y + 1, y === undefined, typeof y, `${y}`, y * 2 > 1, y !== undefined);
function twice(n: number): number { return n * 2; }
function shout(s: string): string { return s + "!"; }
function either(b: boolean): string { return b ? "yes" : "no"; }
console.log(twice(Box.x), shout(Box.s), either(Box.flag));
const t: string = Box.s;
console.log(t, t === undefined, `${t}!`, typeof t);
let z: number = Box.x;
z = z + 1;
console.log(z, typeof z);
const inferred = Box.x;
console.log(inferred, typeof inferred, inferred === undefined);
const m = Box.maybe;
console.log(m, m === undefined);
Box.x = 4;
const w: number = Box.x;
console.log(w + 1, typeof w, w === undefined);
function make(seed: number) {
  class Counter { static base = seed; static total: number; }
  const first: number = Counter.total;
  Counter.total = Counter.base * 2;
  const second: number = Counter.total;
  return `${first} ${typeof first} ${first + 1} ${second} ${second + 1}`;
}
console.log(make(3));
"#;

/// What Node prints for [`ABSENT_STATIC_TS`].
const ABSENT_STATIC_OUTPUT: &str = concat!(
    "undefined NaN true undefined undefined false false\n",
    "NaN undefined! no\n",
    "undefined true undefined! undefined\n",
    "NaN number\n",
    "undefined undefined true\n",
    "undefined true\n",
    "5 number false\n",
    "undefined undefined NaN 6 7\n",
);

/// A read of a static that is still `undefined` behaves as `undefined` in every
/// slot TypeScript types `T`: printed, compared, `typeof`, and converted by
/// arithmetic, concatenation and conditions, with no panic.
#[test]
fn uninitialized_static_reads_stay_undefined_in_typed_slots() -> TestResult {
    let sources = [("main.ts", ABSENT_STATIC_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        ABSENT_STATIC_OUTPUT,
        "absent static reads",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &ABSENT_STATIC_OUTPUT.to_owned(),
        "the absent static reads program's output",
    )?;
    let all = generated_text(root)?;
    // Type preservation: a binding initialized from such a read keeps the
    // storage's `Option<T>`; it is never erased.
    for binding in [
        "let y: Option<f64>",
        "let t: Option<JsString>",
        "let first: Option<f64>",
    ] {
        ensure(
            all.contains(binding),
            format!("`{binding}` keeps `T | undefined`"),
        )?;
    }
    Ok(())
}

/// Static `Map`s and `Set`s mutated through an erased class value, a held
/// erased view, an erased parameter, and narrowed back to the typed container.
const STATIC_CONTAINERS_TS: &str = r#"class Box {
  static m: Map<string, number> = new Map();
  static s: Set<number> = new Set();
  static tags: Set<string> = new Set(["a"]);
  static o: { n: number } = { n: 1 };
}
const C: any = Box;
C.m.set("k", 1);
C.s.add(3);
C.tags.add("b");
C.o.n = 5;
console.log(Box.m.get("k"), Box.m.size, Box.s.has(3), Box.s.size, [...Box.tags].join(","), Box.o.n);
const held = C.m;
Box.m.set("z", 3);
console.log(held.size, held.get("z"), held.has("k"), C.m.get("z"));
held.delete("k");
console.log(Box.m.has("k"), Box.m.size, [...Box.m.keys()].join(","));
function viaUnknown(K: unknown) { (K as any).m.set("j", 2); (K as any).s.delete(3); }
viaUnknown(Box);
console.log(Box.m.get("j"), Box.s.size, C.m.get(5), C.m.has(5));
const back = C.m as Map<string, number>;
back.set("w", 9);
console.log(Box.m.get("w"), back === Box.m);
function fill(target: any) { target.set("p", 7); target.clear; }
const local: Map<string, number> = new Map();
fill(local);
const ls: Set<number> = new Set();
((x: any) => { x.add(4); x.add(4); })(ls);
console.log(local.get("p"), local.size, ls.size, ls.has(4));
C.tags.clear();
console.log(Box.tags.size);
"#;

/// What Node prints for [`STATIC_CONTAINERS_TS`].
const STATIC_CONTAINERS_OUTPUT: &str = concat!(
    "1 1 true 1 a,b 5\n",
    "2 3 true 3\n",
    "false 1 z\n",
    "2 0 undefined false\n",
    "9 true\n",
    "7 1 1 true\n",
    "0\n",
);

/// An erased `Map` or `Set` is the typed container itself: every write through
/// either side is seen by the other.
#[test]
fn static_containers_share_identity_through_erased_values() -> TestResult {
    let sources = [("main.ts", STATIC_CONTAINERS_TS)];
    ensure_node_agrees(
        &sources,
        "main.ts",
        STATIC_CONTAINERS_OUTPUT,
        "static containers",
    )?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &STATIC_CONTAINERS_OUTPUT.to_owned(),
        "the static containers program's output",
    )?;
    let all = generated_text(root)?;
    // Type preservation: the statics keep their typed containers; only the
    // erasure boundary changes to the live view.
    ensure(
        all.contains(
            "static SMELT_GLOBAL_BOX_M_1: ::std::cell::RefCell<SmeltJsMap<JsString, f64>>",
        ),
        "the static `Map` keeps its typed storage",
    )?;
    ensure(
        all.contains(".into_live_smelt_unknown()"),
        "a typed container erases to its live view",
    )
}

/// A class with module-global statics whose static methods read and write
/// statics through `this`, call another static through `this` in an arrow,
/// and construct with `new this()`.
const STATIC_THIS_TS: &str = r#"class Counter {
  static total = 0;
  static label = "c";
  static add(n: number): number { this.total += n; return this.total; }
  static twice(): number { const run = () => this.add(1); run(); return run(); }
  static make(): Counter { return new this(); }
  static describe(): string { return `${this.label}:${this.total}`; }
}
console.log(Counter.add(2), Counter.total, Counter.twice(), Counter.describe(), Counter.make() instanceof Counter);
"#;

/// What Node prints for [`STATIC_THIS_TS`].
const STATIC_THIS_OUTPUT: &str = "2 2 4 c:4 true\n";

/// `this` in a static method is the class, read and written through typed
/// storage rather than the dynamic receiver channel.
#[test]
fn static_methods_bind_this_to_their_class() -> TestResult {
    let sources = [("main.ts", STATIC_THIS_TS)];
    ensure_node_agrees(&sources, "main.ts", STATIC_THIS_OUTPUT, "static `this`")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(
        &output,
        &STATIC_THIS_OUTPUT.to_owned(),
        "the static `this` program's output",
    )?;
    let all = generated_text(root)?;
    ensure(all.contains("fn add(n: f64) -> f64 {"), "`add` is typed")?;
    let counter = impl_text(&all, "impl Counter {")?;
    ensure(
        !counter.contains("smelt_this()"),
        "no static method reads the dynamic `this` channel",
    )
}
