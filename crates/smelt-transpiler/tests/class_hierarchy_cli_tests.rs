//! Class hierarchies and class values at run time, end to end against Node.
//!
//! - **Live erased arrays**: a typed array reached through an erased value (a
//!   static `items: number[]` read through `const C: any = Box`, a list passed
//!   as `unknown`) is the typed array itself: `push`, index and `length`
//!   writes, `splice`, `pop`/`shift`/`unshift`, `sort`/`reverse` are seen
//!   through both handles, narrowing back yields the same array, and a copy
//!   (`[...erased]`, `Array.from(erased)`) is independent. A write the typed
//!   list cannot hold (an extra field, a value of another kind through `any`)
//!   detaches the view into an ordinary erased array holding exactly what was
//!   written.
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
//! - **Shared upcasts**: an instance held at a base type is the same object
//!   (`===`, writes through either handle, virtual dispatch), through a
//!   polymorphic handle over the subclass's own cell
//!   (`codegen::class_polymorphism`).
//! - **Overridden generics**: an overridden generic method stays a Rust
//!   generic; the base forwards to the override.
//! - **Static receivers**: `this` in a static method is the class it was
//!   called on (`Sub.bump()` writes `Sub.count`, `new this()` builds a `Sub`),
//!   also for a base in another module (`codegen::static_receiver`).
//! - **Class captures**: a class declared in a function reads and writes the
//!   function's bindings from its members, per evaluation.

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

/// Checks `source` against Node's `expected` output, builds it, runs the
/// generated program and checks it prints the same; answers the generated Rust.
fn agrees_with_node(source: &str, expected: &str, what: &str) -> TestResult<String> {
    agrees_with_node_in(&[("main.ts", source)], "main.ts", expected, what)
}

/// [`agrees_with_node`] for a project of several modules entered at `entry`;
/// answers the generated Rust of every module.
fn agrees_with_node_in(
    sources: &[(&str, &str)],
    entry: &str,
    expected: &str,
    what: &str,
) -> TestResult<String> {
    ensure_node_agrees(sources, entry, expected, what)?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, entry, sources)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &expected.to_owned(), what)?;
    let mut text = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&fs::read_to_string(path)?);
            text.push('\n');
        }
    }
    Ok(text)
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
function unfaithful(): string {
  const recs: { name: string }[] = [{ name: "a" }];
  const view: any = recs;
  const first = view[0];
  first.extra = 1;
  view[0] = first;
  view.push(5);
  return JSON.stringify(view);
}
console.log(unfaithful());
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
    "[{\"name\":\"a\",\"extra\":1},5]\n",
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

/// Instances of a class hierarchy held at a base type: identity, writes
/// through either handle, virtual dispatch, arrays of the base type,
/// `instanceof` narrowing with subclass field writes, an erased round trip,
/// a three-level chain, and a generic base (`Box<number>`, `Box<string[]>`).
const UPCAST_SHARING_TS: &str = r#"class Base {
  count = 0;
  bump(): void { this.count++; }
  describe(): string { return "base " + this.count; }
}
class Sub extends Base {
  extra = 10;
  describe(): string { return "sub " + this.count + " " + this.extra; }
  grow(): void { this.extra++; }
}
const l = new Sub();
const pb: Base = l;
console.log(l === pb);
pb.bump();
console.log(l.count, pb.count);
l.grow();
l.bump();
console.log(pb.describe(), l.describe());
const arr: Base[] = [new Base(), l];
for (const x of arr) x.bump();
console.log(arr.map(x => x.describe()).join("|"), l.count);
abstract class Shape {
  name: string;
  hits = 0;
  constructor(name: string) { this.name = name; }
  abstract area(): number;
  describe(): string { this.hits++; return `${this.name}:${this.area()}`; }
  get label(): string { return "<" + this.name + ">"; }
}
class Rect extends Shape {
  w: number; h: number;
  constructor(w: number, h: number) { super("rect"); this.w = w; this.h = h; }
  area(): number { return this.w * this.h; }
}
class Square extends Rect {
  constructor(s: number) { super(s, s); this.name = "square"; }
  describe(): string { return "sq " + super.describe(); }
  onClick = () => { this.hits += 100; };
}
class Circle extends Shape {
  r = 1;
  constructor() { super("circle"); }
  area(): number { return 3 * this.r * this.r; }
}
function grow(s: Shape): Shape { s.hits += 10; return s; }
const sq = new Square(2);
const shapes: Shape[] = [new Rect(1, 2), sq, new Circle()];
console.log(shapes.map(s => s.describe()).join(" "));
console.log(sq.hits, shapes[1] === sq, grow(sq) === sq, sq.hits);
const r: Rect = sq;
r.w = 5;
console.log(sq.area(), shapes[1].area(), r.describe(), sq.label, r.label);
sq.onClick();
console.log(r.hits, shapes[1].hits);
for (const s of shapes) {
  if (s instanceof Rect) { s.h = 7; console.log("rect-ish", s.name, s.area(), s instanceof Square); }
  else if (s instanceof Circle) { s.r = 2; console.log("circle", s.area()); }
}
console.log(sq.area());
const erased: unknown = r;
const back = erased as Shape;
console.log(back === sq, (erased as Square).w, back instanceof Square, JSON.stringify(erased));
const anyS: any = shapes[1];
anyS.w = 1;
console.log(sq.w, anyS.describe());
function pick(s: Shape): string { return s instanceof Square ? "S" : s instanceof Rect ? "R" : "O"; }
console.log(shapes.map(pick).join(""));
class Box<T> { v: T; constructor(v: T) { this.v = v; } get(): T { return this.v; } set(v: T) { this.v = v; } }
class NumBox extends Box<number> { inc() { this.v++; } get(): number { return this.v * 10; } }
class ListBox<U> extends Box<U[]> { push(u: U) { this.v.push(u); } }
const nb = new NumBox(1);
const b: Box<number> = nb;
nb.inc(); b.set(b.get() + 1);
console.log(nb.v, b.v, b.get(), nb === b);
const lb = new ListBox<string>(["a"]);
const bl: Box<string[]> = lb;
lb.push("b");
console.log(bl.get().join(","), bl === lb);
"#;

/// What Node prints for [`UPCAST_SHARING_TS`].
const UPCAST_SHARING_OUTPUT: &str = "true\n1 1\nsub 2 11 sub 2 11\nbase 1|sub 3 11 3\nrect:2 sq square:4 circle:3\n1 true true 11\n10 10 sq square:10 <square> <square>\n112 112\nrect-ish rect 7 false\nrect-ish square 35 true\ncircle 12\n35\ntrue 5 true {\"name\":\"square\",\"hits\":112,\"w\":5,\"h\":7}\n1 sq square:7\nRSO\n21 21 210 true\na,b true\n";

/// An upcast is the same object: one shared cell behind every static type.
#[test]
fn upcasts_share_one_object() -> TestResult {
    let all = agrees_with_node(UPCAST_SHARING_TS, UPCAST_SHARING_OUTPUT, "upcast sharing")?;
    // Type preservation: a class with subclasses is a polymorphic handle
    // over its subclasses' cells; a leaf stays a concrete cell nesting its
    // base's typed record; a generic base keeps its arguments.
    for (needle, what) in [
        (
            "struct Base(::std::rc::Rc<dyn BaseSmeltCell>);",
            "`Base` is a polymorphic handle",
        ),
        (
            "struct Rect(::std::rc::Rc<dyn RectSmeltCell>);",
            "an intermediate class is polymorphic too",
        ),
        (
            "struct Square(::std::rc::Rc<::std::cell::RefCell<SquareInner>>);",
            "a leaf class stays a concrete cell",
        ),
        (
            "__smelt_base: RectInner,",
            "`Square` nests `Rect`'s typed record",
        ),
        (
            "struct Box_<T>(::std::rc::Rc<dyn Box_SmeltCell<T>>);",
            "a generic base keeps its type parameter",
        ),
        (
            "__smelt_base: Box_Inner<f64>,",
            "`NumBox` nests `Box<number>`'s record",
        ),
        (
            "__smelt_base: Box_Inner<SmeltList<U>>,",
            "`ListBox<U>` nests `Box<U[]>`'s record",
        ),
        (
            "pb: Base = Base(_smelt_tmp_12.0.clone());",
            "the upcast re-views the same cell",
        ),
    ] {
        ensure(all.contains(needle), what)?;
    }
    ensure(
        !all.contains("RefCell::new(BaseInner { count: smelt_struct_value"),
        "an upcast never copies the instance into a new base cell",
    )
}

/// A generic method overridden down a hierarchy, called at two
/// instantiations through the base type, and a generic method of a generic
/// class whose signature spells the class's own parameter.
const OVERRIDE_GENERICS_TS: &str = r#"class A {
  vals: number[] = [1, 2];
  map<U>(f: (x: number) => U): U[] { return this.vals.map(f); }
  first<T>(xs: T[]): T { return xs[0]; }
  tag(): string { return "A"; }
}
class B extends A {
  map<U>(f: (x: number) => U): U[] { return this.vals.map(x => f(x * 10)); }
  first<T>(xs: T[]): T { return xs[xs.length - 1]; }
  tag(): string { return "B"; }
}
class C extends B {
  first<T>(xs: T[]): T { return xs[1]; }
}
const items: A[] = [new A(), new B(), new C()];
for (const it of items) {
  console.log(it.tag(), it.map(x => x + 1).join(","), it.map(x => "s" + x).join(","), it.first(["p", "q", "r"]), it.first([7, 8, 9]));
}
const b: B = new C();
console.log(b.first([1, 2, 3]), b.map(x => x * 2).join(","));
class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  wrap<U>(f: (v: T) => U): U { return f(this.v); }
}
class Box2<T> extends Box<T> {
  wrap<U>(f: (v: T) => U): U { return f(this.v); }
}
const bx: Box<number> = new Box2(5);
console.log(bx.wrap(v => v * 3), bx.wrap(v => "x" + v));
"#;

/// What Node prints for [`OVERRIDE_GENERICS_TS`].
const OVERRIDE_GENERICS_OUTPUT: &str =
    "A 2,3 s1,s2 p 7\nB 11,21 s10,s20 r 9\nB 11,21 s10,s20 q 8\n2 20,40\n15 x5\n";

/// An overridden generic method stays a Rust generic and dispatches to the
/// override.
#[test]
fn overridden_methods_keep_their_generics() -> TestResult {
    let all = agrees_with_node(
        OVERRIDE_GENERICS_TS,
        OVERRIDE_GENERICS_OUTPUT,
        "overridden generic methods",
    )?;
    // Type preservation: every `map`/`first`/`wrap` is generic over its own
    // parameter, none erases it, and the base forwards to the override
    // through a checked downcast of the shared cell.
    ensure(
        all.matches("fn map<U: ").count() >= 3,
        "`map` is generic in `A`, `B` and `C`",
    )?;
    ensure(
        all.matches("fn first<T: ").count() >= 3,
        "`first` is generic in `A`, `B` and `C`",
    )?;
    ensure(
        all.contains("fn wrap<U: ") && all.contains("f: &dyn Fn(&T) -> U) -> U"),
        "`Box<T>.wrap<U>` keeps both parameters",
    )?;
    ensure(
        all.contains("downcast::<::std::cell::RefCell<BInner>>()"),
        "`A.map` forwards to `B`'s override",
    )?;
    ensure(
        !all.contains("fn map(&self, f: &dyn Fn(f64) -> SmeltUnknown)")
            && !all.contains("fn first(&self, xs: SmeltList<SmeltUnknown>) -> SmeltUnknown"),
        "no override erases its type parameter",
    )
}

/// `this` in inherited static methods: field reads and writes, a static
/// redeclared by the subclass, `new this()`, a typed class value.
const STATIC_RECEIVER_TS: &str = r#"class Base {
  static count = 0;
  static label = "base";
  static bump(): number { this.count++; return this.count; }
  static make(): Base { return new this(); }
  static describe(): string { return this.label + ":" + this.count; }
  who(): string { return "Base"; }
}
class Sub extends Base {
  static label = "sub";
  who(): string { return "Sub"; }
}
console.log(Base.bump(), Sub.bump(), Sub.bump(), Base.count, Sub.count);
console.log(Base.describe(), Sub.describe());
console.log(Base.make().who(), Sub.make().who(), Sub.make() instanceof Sub);
const K: typeof Base = Sub;
console.log(K.bump(), Sub.count, K.describe());
"#;

/// What Node prints for [`STATIC_RECEIVER_TS`].
const STATIC_RECEIVER_OUTPUT: &str = "1 2 3 1 3\nbase:1 sub:3\nBase Sub true\n4 4 sub:4\n";

/// The base module of [`STATIC_RECEIVER_MAIN_TS`]: static methods using
/// `this` that a subclass in another module inherits.
const STATIC_RECEIVER_BASE_TS: &str = r#"export class Model {
  static registry: string[] = [];
  static instances = 0;
  static kind(): string { return "model"; }
  static create(): Model { this.instances++; this.registry.push(this.kind()); return new this(); }
  static describe(): string { return `${this.kind()}#${this.instances}`; }
  static later(): () => string { return () => this.kind() + "!" + this.instances; }
  static self(): unknown { return this; }
  static pick<T>(xs: T[]): T { this.instances += 100; return xs[0]; }
  name(): string { return "Model"; }
}
"#;

/// Subclasses overriding a static method `this` dispatches to, a closure
/// capturing `this`, `this` as a value, and a generic static method.
const STATIC_RECEIVER_MAIN_TS: &str = r#"import { Model } from "./base.ts";
class User extends Model {
  static kind(): string { return "user"; }
  name(): string { return "User"; }
}
class Admin extends User {
  static kind(): string { return "admin"; }
}
const u = User.create();
const a = Admin.create();
const m = Model.create();
console.log(u.name(), a.name(), m.name(), u instanceof User, a instanceof Admin);
console.log(Model.instances, User.instances, Admin.instances);
console.log(Model.describe(), User.describe(), Admin.describe());
console.log(Model.registry.join(","), User.registry === Model.registry);
const f = Admin.later();
Admin.instances = 7;
console.log(f());
console.log(User.self() === User, Admin.pick(["x", "y"]), Admin.instances, User.instances);
"#;

/// What Node prints for [`STATIC_RECEIVER_MAIN_TS`].
const STATIC_RECEIVER_MAIN_OUTPUT: &str = "User User Model true true\n1 1 2\nmodel#1 user#1 admin#2\nuser,admin,model true\nadmin!7\ntrue x 107 1\n";

/// A static method's `this` is the class it was called on, through a
/// receiver-class generic.
#[test]
fn static_methods_take_their_receiver_class() -> TestResult {
    let all = agrees_with_node(
        STATIC_RECEIVER_TS,
        STATIC_RECEIVER_OUTPUT,
        "static receivers",
    )?;
    // Type preservation: the receiver is a type parameter bounded by the
    // statics trait; the inherited statics stay typed (`f64`).
    for (needle, what) in [
        (
            "fn bump<SmeltThis: BaseSmeltStatics>(_smelt_this: ::std::marker::PhantomData<SmeltThis>) -> f64 {",
            "`bump` takes its receiver class as a generic",
        ),
        (
            "Base::bump(::std::marker::PhantomData::<Sub>)",
            "`Sub.bump()` passes `Sub`",
        ),
        (
            "impl BaseSmeltStatics for Sub {",
            "`Sub` implements the statics trait",
        ),
        (
            "fn __smelt_static_get_count() -> f64",
            "the receiver's `count` is read as `f64`",
        ),
    ] {
        ensure(all.contains(needle), what)?;
    }
    let sources = [
        ("base.ts", STATIC_RECEIVER_BASE_TS),
        ("main.ts", STATIC_RECEIVER_MAIN_TS),
    ];
    let across = agrees_with_node_in(
        &sources,
        "main.ts",
        STATIC_RECEIVER_MAIN_OUTPUT,
        "static receivers across modules",
    )?;
    ensure(
        across.contains("impl ModelSmeltStatics for Admin {"),
        "a subclass in another module implements the base's statics trait",
    )?;
    ensure(
        across.contains("fn pick<SmeltThis: ModelSmeltStatics, T: "),
        "a generic static method keeps its own parameter beside the receiver",
    )
}

/// Instance and static members of classes declared in functions reading and
/// writing the function's locals and parameters, per evaluation, with the
/// enclosing function and its closures sharing the writes.
const CLASS_CAPTURES_TS: &str = r#"function mk(n: number, label: string) {
  let calls = 0;
  const K = class {
    v = n * 2;
    get(): number { calls++; return n + calls; }
    name(): string { return label + ":" + this.v; }
    static peek(): number { return calls * 1000 + n; }
  };
  calls += 10;
  return K;
}
const A = mk(1, "a");
const B = mk(10, "b");
const a = new A();
const b = new B();
const a2 = new A();
console.log(a.get(), a.get(), b.get(), a2.get(), a.name(), b.name(), A.peek(), B.peek());
function counter(start: number) {
  let total = start;
  class Acc {
    add(x: number): number { total += x; return total; }
    peek(): number { return total; }
  }
  const acc = new Acc();
  const read = () => total;
  acc.add(1);
  total += 100;
  return [acc.peek(), read(), acc.add(1)];
}
console.log(counter(5).join(","), counter(50).join(","));
"#;

/// What Node prints for [`CLASS_CAPTURES_TS`].
const CLASS_CAPTURES_OUTPUT: &str = "12 13 21 14 a:2 b:20 13001 11010\n106,106,107 151,151,152\n";

/// A class body reads and writes the enclosing function's bindings.
#[test]
fn class_bodies_capture_enclosing_bindings() -> TestResult {
    let all = agrees_with_node(CLASS_CAPTURES_TS, CLASS_CAPTURES_OUTPUT, "class captures")?;
    // Type preservation: each captured binding is a typed field of the
    // evaluation's frame.
    ensure(
        all.contains("__smelt_capture_calls: f64,"),
        "`calls` is a typed frame field",
    )?;
    ensure(
        all.contains("__smelt_capture_label: JsString,"),
        "`label` is a typed frame field",
    )?;
    ensure(
        all.contains("__smelt_capture_total: f64,"),
        "`total` is a typed frame field",
    )
}

/// Adjacent shapes fixed beside the hierarchy work: a subclass field written
/// through an `instanceof`-narrowed base local, a field-less base class
/// returned from a subclass instance, a generic hierarchy erased and narrowed
/// back, a `Parser<number>` stored as a `Parser<any>`, and an abstract
/// generic method.
const ADJACENT_HIERARCHY_TS: &str = r#"class NShape { n = 0; }
class NRect extends NShape { h = 1; }
function f(s: NShape): void {
  if (s instanceof NRect) { s.h = 7; console.log(s.h); }
}
const r = new NRect();
f(r);
const list: NShape[] = [new NShape(), r];
for (const s of list) { if (s instanceof NRect) { s.h = 9; } }
console.log(r.h);
class FModel { static n = 0; name(): string { return "FModel"; } }
class FUser extends FModel { }
function mk(): FModel { return new FUser(); }
const fm: FModel = new FUser();
console.log(fm.name(), mk().name(), fm instanceof FUser);
class GBase<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  show(x: T): string { return "base " + String(x); }
  set(v: T): void { this.v = v; }
}
class GSub<T> extends GBase<T> {
  show(x: T): string { return "sub " + String(x); }
}
const gb: GBase<number> = new GSub<number>(1);
gb.set(5);
console.log(gb.show(2), gb.v);
const ge: unknown = gb;
console.log(JSON.stringify(ge), (ge as GBase<number>).show(3));
type PResult<T> = { value: T };

abstract class PParser<T> {
  abstract parse(input: string): PResult<T>;
}

class ForwardingPParser<T> extends PParser<T> {
  delegate: PParser<T>;
  constructor(delegate: PParser<T>) {
    super();
    this.delegate = delegate;
  }

  parse(input: string): PResult<T> {
    return this.delegate.parse(input);
  }
}

class YearPParser extends PParser<number> {
  parse(input: string): PResult<number> {
    return { value: 2017 };
  }
}

const pparser: PParser<any> = new ForwardingPParser(new YearPParser());
console.log(pparser.parse("x").value);
abstract class ABox {
  abstract pick<T>(x: T): T;
}
class ASub extends ABox {
  pick<T>(x: T): T {
    return x;
  }
}
const ab: ABox = new ASub();
console.log(ab.pick(1), ab.pick("s"));
"#;

/// What Node prints for [`ADJACENT_HIERARCHY_TS`].
const ADJACENT_HIERARCHY_OUTPUT: &str =
    "7\n9\nFModel FModel true\nsub 2 5\n{\"v\":5} sub 3\n2017\n1 s\n";

/// Narrowed writes, field-less upcasts, generic erasure and `any`-typed slots.
#[test]
fn adjacent_hierarchy_shapes_run() -> TestResult {
    let all = agrees_with_node(
        ADJACENT_HIERARCHY_TS,
        ADJACENT_HIERARCHY_OUTPUT,
        "adjacent hierarchy shapes",
    )?;
    // Type preservation: the narrowed local is viewed as the subclass, and
    // the generic hierarchy keeps its parameter.
    ensure(
        all.contains("struct GBase<T>(::std::rc::Rc<dyn GBaseSmeltCell<T>>);"),
        "the generic base is a polymorphic handle over `T`",
    )?;
    ensure(
        all.contains("fn f(s: NShape) -> ()"),
        "`f` keeps its typed parameter",
    )
}
