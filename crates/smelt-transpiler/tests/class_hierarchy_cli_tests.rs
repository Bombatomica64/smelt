//! Class hierarchies and class values at run time, end to end against Node.
//!
//! - **Live erased arrays**: a typed array reached through an erased value (a
//!   static `items: number[]` read through `const C: any = Box`, a list passed
//!   as `unknown`) is the typed array itself: `push`, index and `length`
//!   writes, `splice`, `pop`/`shift`/`unshift`, `sort`/`reverse` are seen
//!   through both handles, narrowing back yields the same array, and a copy
//!   (`[...erased]`, `Array.from(erased)`) is independent.
//! - **Subclass static writes**: `Sub.x = v` for a static `x` declared on
//!   `Base` makes `x` an own property of `Sub` (directly, through a typed or an
//!   erased class value, with `++`/`+=`, three levels deep), leaving `Base.x`
//!   alone; before it, `Sub.x` reads `Base.x` live; `Object.keys(Sub)` lists it
//!   once owned.
//! - **Class identity through upcasts**: an instance passed or returned at a
//!   base type keeps the class it was constructed as for `instanceof`, through
//!   arrays, optionals, erasure and narrowing.
//! - **Method generics**: a method's own type parameters are Rust generics on
//!   the method (`fn map<U>`), calls monomorphize at the caller's types, and a
//!   generic class's static methods (including generic ones) compile and run.
//! - **Class expressions as values**: `new` through a class a function returns
//!   (`const make = () => class { .. }`) constructs it.

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
            "[project]\nname = \"hierarchy\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"hierarchy\"\nkind = \"program\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// The generated program's Rust: the entry module and its source module.
fn generated_text(root: &Path) -> TestResult<String> {
    let main = fs::read_to_string(root.join("dist/src/main.rs"))?;
    let source = fs::read_to_string(root.join("dist/src/source_main.rs")).unwrap_or_default();
    Ok(format!("{main}\n{source}"))
}

/// Checks `source` against Node's `expected` output, builds it, runs the
/// generated program and checks it prints the same; answers the generated Rust.
fn agrees_with_node(source: &str, expected: &str, what: &str) -> TestResult<String> {
    let sources = [("main.ts", source)];
    ensure_node_agrees(&sources, "main.ts", expected, what)?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &expected.to_owned(), what)?;
    generated_text(root)
}

/// A static array mutated through an erased class value and through the
/// class, both ways, plus copies that must stay independent.
const LIVE_ARRAYS_TS: &str = r#"class Box {
  static items: number[] = [1];
  static names: string[] = ["x"];
}
const C: any = Box;
C.items.push(2);
console.log(Box.items.length, Box.items[1]);
Box.items.push(3);
console.log(C.items.length, C.items[2]);
C.items[0] = 9;
console.log(Box.items[0]);
C.items.length = 1;
console.log(Box.items.length);
Box.items.push(4, 5, 6);
C.items.splice(1, 1);
console.log(Box.items.join(","), C.items.join(","));
const copy = [...C.items];
copy.push(100);
const copy2 = Array.from(C.items);
copy2.push(200);
console.log(Box.items.length, copy.length, copy2.length);
C.names.push("y");
C.names.unshift("w");
C.names.reverse();
console.log(Box.names.join(","), C.names.pop(), C.names.shift(), Box.names.join(","));
C.items.sort((a: number, b: number) => b - a);
console.log(Box.items.join(","), JSON.stringify(C.items));
function viaUnknown(x: unknown): void { (x as number[]).push(7); (x as any)[0] = 0; }
function roundTrip(): string {
  const local: number[] = [5];
  viaUnknown(local);
  const erased: unknown = local;
  const back = erased as number[];
  back.push(1);
  return `${local.join(",")} ${back === local} ${(erased as any).length}`;
}
console.log(roundTrip());
"#;

/// What Node prints for [`LIVE_ARRAYS_TS`].
const LIVE_ARRAYS_OUTPUT: &str = concat!(
    "2 2\n",
    "3 3\n",
    "9\n",
    "1\n",
    "9,5,6 9,5,6\n",
    "3 4 4\n",
    "y,x,w w y x\n",
    "9,6,5 [9,6,5]\n",
    "0,7,1 true 3\n",
);

/// An erased typed array is the typed array itself, and a copy is a copy.
#[test]
fn erased_arrays_are_live_views_of_typed_arrays() -> TestResult {
    let all = agrees_with_node(LIVE_ARRAYS_TS, LIVE_ARRAYS_OUTPUT, "live erased arrays")?;
    // Type preservation: the statics keep their typed lists; only the erasure
    // boundary is the live view.
    ensure(
        all.contains("::std::cell::RefCell<SmeltList<f64>>"),
        "`Box.items` keeps its `SmeltList<f64>` storage",
    )?;
    ensure(
        all.contains("::std::cell::RefCell<SmeltList<JsString>>"),
        "`Box.names` keeps its `SmeltList<JsString>` storage",
    )?;
    ensure(
        all.contains(".into_live_smelt_unknown()"),
        "a typed list erases to its live view",
    )
}

/// Inherited statics through a three-level module-level hierarchy: live reads
/// before the write, own values after it, through every write spelling.
const SUBCLASS_STATICS_TS: &str = r#"class Base { static x = 1; static arr: number[] = []; static label = "base"; static readonly K = 9; }
class Sub extends Base { static own = 0; }
class Leaf extends Sub {}
console.log(Sub.x, Base.x, Leaf.x);
Base.x = 2;
console.log(Sub.x, Base.x, Leaf.x);
Sub.x = 5;
console.log(Sub.x, Base.x, Leaf.x);
Base.x = 7;
console.log(Sub.x, Base.x, Leaf.x);
Leaf.x++;
Leaf.x += 10;
const y = (Leaf.x = Leaf.x * 2);
console.log(Sub.x, Base.x, Leaf.x, y);
Sub.arr.push(1);
console.log(Base.arr.length, Sub.arr.length, Leaf.arr.length);
Leaf.label = "leaf";
console.log(Base.label, Sub.label, Leaf.label, Leaf.K);
class Other extends Base { static z = 3; }
function write(K: typeof Base, v: number): void { K.x = v; }
function read(K: typeof Base): number { return K.x; }
console.log(Object.keys(Other).join(","));
write(Other, 40);
console.log(Base.x, Other.x, read(Other), read(Base), Object.keys(Other).join(","));
const E: any = Other;
E.x = 8;
console.log(Base.x, Other.x, E.x);
class Fresh extends Base {}
const keys: string[] = [];
for (const key in Fresh) keys.push(key);
console.log(Object.keys(Fresh).length, keys.join(","));
"#;

/// What Node prints for [`SUBCLASS_STATICS_TS`].
const SUBCLASS_STATICS_OUTPUT: &str = concat!(
    "1 1 1\n",
    "2 2 2\n",
    "5 2 5\n",
    "5 7 5\n",
    "5 7 32 32\n",
    "1 1 1\n",
    "base base leaf 9\n",
    "z\n",
    "7 40 40 7 z,x\n",
    "7 8 8\n",
    "0 x,arr,label,K\n",
);

/// A write through a subclass makes the static the subclass's own.
#[test]
fn subclass_static_writes_shadow_the_base() -> TestResult {
    let all = agrees_with_node(
        SUBCLASS_STATICS_TS,
        SUBCLASS_STATICS_OUTPUT,
        "subclass static writes",
    )?;
    // Type preservation: the inherited static keeps its type in the
    // subclass's accessor pair and own flag.
    ensure(
        all.contains("fn __smelt_get_x() -> f64 {"),
        "the inherited `x` getter answers `f64`",
    )?;
    ensure(
        all.contains("fn __smelt_set_x(value: f64) -> f64 {"),
        "the inherited `x` setter takes and answers `f64`",
    )?;
    ensure(
        all.contains("fn __smelt_get_arr() -> SmeltList<f64> {"),
        "the inherited `arr` stays a typed list",
    )
}

/// Subclass instances held at their base type, asked for their class.
const UPCAST_IDENTITY_TS: &str = r#"class Base { v = 1; }
class Sub extends Base { w = 2; }
class Leaf extends Sub { z = 3; }
class Other extends Base {}
function f(): Base { return new Sub(); }
const b = f();
console.log(b instanceof Sub, b instanceof Base, b instanceof Leaf, b instanceof Other);
const arr: Base[] = [new Sub(), new Base(), new Leaf(), new Other()];
console.log(arr.map(a => a instanceof Sub).join(","));
console.log(arr.map(a => a instanceof Other).join(","));
const maybe: Base | undefined = arr[2];
console.log(maybe instanceof Leaf);
const erased: unknown = b;
console.log(erased instanceof Sub, erased instanceof Leaf);
function back(x: unknown): Base { return x as Base; }
console.log(back(erased) instanceof Sub);
const s: Sub = new Leaf();
console.log(s instanceof Leaf, s instanceof Sub);
"#;

/// What Node prints for [`UPCAST_IDENTITY_TS`].
const UPCAST_IDENTITY_OUTPUT: &str = concat!(
    "true true false false\n",
    "true,false,true,false\n",
    "false,false,false,true\n",
    "true\n",
    "true false\n",
    "true\n",
    "true true\n",
);

/// An upcast keeps the class an instance was constructed as.
#[test]
fn upcast_instances_keep_their_class() -> TestResult {
    let all = agrees_with_node(
        UPCAST_IDENTITY_TS,
        UPCAST_IDENTITY_OUTPUT,
        "upcast identity",
    )?;
    // Type preservation: the base type stays the typed `Base` struct, which
    // carries the tag; the function still returns `Base`.
    ensure(
        all.contains("_smelt_class: &'static str,"),
        "the hierarchy's structs carry the class tag",
    )?;
    ensure(
        all.contains("fn f() -> Base {"),
        "`f` returns the typed `Base`",
    )
}

/// Generic instance and static methods, and static methods of a generic class.
const METHOD_GENERICS_TS: &str = r#"class A {
  vals: number[] = [1, 2];
  map<U>(f: (x: number) => U): U[] { return this.vals.map(f); }
  pick<T>(x: T, y: T): T { return this.vals.length > 1 ? y : x; }
  static wrap<T>(x: T): T[] { return [x]; }
}
class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  static create<T>(v: T): Box<T> { return new Box(v); }
  static of(n: number): Box<number> { return new Box(n); }
}
const a = new A();
console.log(a.map(x => x * 2).join(","), a.map(x => "s" + x).join(","), a.pick("p", "q"), A.wrap(3).length);
console.log(Box.create<string>("hi").v, Box.create(3).v, Box.of(4).v);
const erased: any = a;
console.log(erased.map((x: number) => x + 1).join(","), erased.pick(1, 2));
"#;

/// What Node prints for [`METHOD_GENERICS_TS`].
const METHOD_GENERICS_OUTPUT: &str = "2,4 s1,s2 q 1\nhi 3 4\n2,3 2\n";

/// A method's own type parameters are Rust generics, monomorphized per call.
#[test]
fn method_type_parameters_are_rust_generics() -> TestResult {
    let all = agrees_with_node(
        METHOD_GENERICS_TS,
        METHOD_GENERICS_OUTPUT,
        "method generics",
    )?;
    // Type preservation: the methods are generic, and each call site is typed
    // at the caller's instantiation rather than erased.
    for (needle, what) in [
        ("fn map<U: ", "`map` declares its own `U`"),
        ("fn pick<T: ", "`pick` declares its own `T`"),
        ("fn wrap<T: ", "the static `wrap` declares its own `T`"),
        (
            "fn create<T: ",
            "the generic class's static `create` declares its own `T`",
        ),
        (
            ": SmeltList<f64> = a.map(",
            "`a.map(x => x * 2)` is a `SmeltList<f64>`",
        ),
        (
            ": SmeltList<JsString> = a.map(",
            "`a.map(x => \"s\" + x)` is a `SmeltList<JsString>`",
        ),
        (
            ": JsString = a.pick(",
            "`a.pick(\"p\", \"q\")` is a `JsString`",
        ),
    ] {
        ensure(all.contains(needle), what)?;
    }
    ensure(
        !all.contains("fn map(&self, f: &dyn Fn(f64) -> SmeltUnknown)"),
        "`map` does not erase its type parameter",
    )
}

/// Classes produced by functions and constructed through the returned value.
const CLASS_EXPRESSION_VALUES_TS: &str = r"const make = () => class { v = 3; get() { return this.v; } };
const K = make();
const k = new K();
console.log(k.v, k.get());
function build() { return class { w = 4; twice(): number { return this.w * 2; } }; }
const K2 = build();
console.log(new K2().w, new K2().twice());
const make3 = () => { return class Named { z = 5; }; };
const K3 = make3();
const n = new K3();
console.log(n.z, n instanceof K3, typeof K3);
";

/// What Node prints for [`CLASS_EXPRESSION_VALUES_TS`].
const CLASS_EXPRESSION_VALUES_OUTPUT: &str = "3 3\n4 8\n5 true function\n";

/// `new` through a class value a function returned constructs that class.
#[test]
fn class_expressions_returned_from_functions_construct() -> TestResult {
    let all = agrees_with_node(
        CLASS_EXPRESSION_VALUES_TS,
        CLASS_EXPRESSION_VALUES_OUTPUT,
        "class expression values",
    )?;
    // Type preservation: the class expression is still a typed struct with
    // its typed field; only the class VALUE crosses the dynamic boundary.
    ensure(
        all.contains("v: f64,") && all.contains("w: f64,"),
        "the anonymous classes keep their typed fields",
    )
}
