//! Names resolve the way the source language resolves them, end to end.
//!
//! Each program here reaches one of the frontend name-resolution findings of
//! the implicit-invariants audit (section A) from accepted source, builds, and
//! must print what Node or `CPython` prints for the same source:
//!
//! - **A2**: two modules export one name; an importer gets the module its
//!   `from` names, never whichever module published the bare name last.
//! - **A3**: a parameter named like an enum is the parameter, so `Mode.Fast`
//!   is not folded to the enum member.
//! - **A4**: a local named like a module class (or `Date`) is the local.
//! - **A5**: `import type { Map }` from a source module is that module's type,
//!   not the lib `Map`.
//! - **A6**: function hoisting is transitive, and a parameter named like a
//!   later function does not move it.
//! - **A7**: an evolving array holds every element type any branch pushes.
//! - A constructor function declared inside an ordinary function is a class
//!   (`new Foo().a` reads the field, never `NaN`).
//! - **A8** (Python): a declared return type is not replaced by a narrower
//!   inference on a method or a container.

mod common;

use std::{fs, path::Path};

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure_eq, ensure_node_agrees, ensure_python_agrees, smelt_in,
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
            "[project]\nname = \"names\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"names\"\nkind = \"program\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// Builds the project at `root` and returns what the generated program prints.
fn build_and_run(root: &Path) -> TestResult<String> {
    smelt_in(root, &["build"])?;
    cargo_run_manifest(&root.join("dist/Cargo.toml"))
}

/// `a.ts` and `b.ts` export the same three names with different values (A2).
const MODULE_A: &str = r#"export const DEFAULTS = { width: 1, name: "a" };
export enum Mode { Fast = 1, Slow = 2 }
export const LIST = [1, 2, 3];
"#;

/// The other exporter of `DEFAULTS`, `Mode` and `LIST` (A2).
const MODULE_B: &str = r#"export const DEFAULTS = { width: 2, name: "b" };
export enum Mode { Fast = 10, Slow = 20 }
export const LIST = [7, 8];
"#;

/// Imports `b`'s exports; lowered after both `a` and `b` (A2).
const MODULE_C: &str = r#"import { DEFAULTS, Mode, LIST } from "./b.ts";
export function fromB(): string { return `${DEFAULTS.width} ${DEFAULTS.name} ${Mode.Fast} ${LIST.length}`; }
"#;

/// The type-only `Map` another module declares (A5).
const MODULE_GAME: &str = "export interface Map<K, V> { key: K; value: V }\n";

/// The entry: every TypeScript finding, one output line each.
const MAIN_TS: &str = r#"import { DEFAULTS, Mode, LIST } from "./a.ts";
import { fromB } from "./c.ts";
import type { Map } from "./game.ts";

// A2: `a`'s exports here, `b`'s in `c`.
console.log(DEFAULTS.width, DEFAULTS.name, Mode.Fast, LIST.length);
console.log(fromB());

// A3: the parameter shadows the enum.
function fast(Mode: { Fast: number }): number { return Mode.Fast; }
console.log(fast({ Fast: 42 }), Mode.Slow);

// A4: locals shadow a module class and `Date`.
class Item { constructor(public v: number) {} }
function first(xs: number[]): number {
  const Item = xs[0];
  return Item + 1;
}
function stamp(): number {
  const Date = "not a date";
  return Date.length;
}
console.log(first([5, 6]), new Item(3).v, stamp());

// A5: the imported `Map` is the game's.
function show(m: Map<string, number>): string { return m.key + "=" + m.value; }
console.log(show({ key: "a", value: 1 }));

// A6: `f` is hoisted above its use and needs `g`, declared after it; the
// arrow's parameter `g` is not the function `g`, which must stay below `scale`.
function transitive(): number {
  const r = f(2);
  function f(x: number): number { return g(x) * 10; }
  function g(x: number): number { return x + 1; }
  return r;
}
function scoped(): number {
  const apply = (g: number): number => g * 2;
  const scale = 3;
  function g(): number { return scale; }
  return apply(1) + g();
}
console.log(transitive(), scoped());

// A7: one branch pushes a number, the other a string.
function build(flag: boolean): string {
  const xs = [];
  if (flag) { xs.push(1); return typeof xs[0] + xs.length; }
  xs.push("a");
  return xs.join(",");
}
console.log(build(true), build(false));

// A constructor function nested in an ordinary function.
export function make(): number {
  function Foo(this: { a: number }) { this.a = 7; }
  // @ts-expect-error a function declaration is constructed as a class
  return new Foo().a;
}
console.log(make());
"#;

/// What Node prints for [`MAIN_TS`] (checked with
/// `node --experimental-transform-types`).
const MAIN_TS_OUTPUT: &str = "1 a 1 3\n2 b 10 2\n42 2\n6 3 10\na=1\n30 5\nnumber1 a\n7\n";

/// Every source file of the TypeScript program.
const TS_SOURCES: [(&str, &str); 5] = [
    ("main.ts", MAIN_TS),
    ("a.ts", MODULE_A),
    ("b.ts", MODULE_B),
    ("c.ts", MODULE_C),
    ("game.ts", MODULE_GAME),
];

/// The TypeScript name-resolution findings build and print what Node prints.
#[test]
fn typescript_names_resolve_like_node() -> TestResult {
    ensure_node_agrees(&TS_SOURCES, "main.ts", MAIN_TS_OUTPUT, "name resolution")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &TS_SOURCES)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &MAIN_TS_OUTPUT.to_owned(), "the name-resolution program's output")
}

/// A Python method and a container keep their declared return types (A8).
///
/// `Base.value` is declared `-> float` and returns `0`; inferring `int` for it
/// would diverge from the `float` its override returns. `ratios` is declared
/// `-> list[float]` and returns `[1]`; inferring `list[int]` would reject the
/// caller's `append(2.5)`.
const MAIN_PY: &str = "class Base:
    def value(self) -> float:
        return 0


class Child(Base):
    def value(self) -> float:
        return 1.5


def ratios() -> list[float]:
    return [1]


xs = ratios()
xs.append(2.5)
print(Base().value() + 0.25, Child().value() + 0.25, sum(xs))
";

/// What `CPython` prints for [`MAIN_PY`].
const MAIN_PY_OUTPUT: &str = "0.25 1.75 3.5\n";

/// Python declared return contracts survive inference and print what `CPython` prints.
#[test]
fn python_declared_returns_are_kept() -> TestResult {
    ensure_python_agrees(MAIN_PY, MAIN_PY_OUTPUT, "declared returns")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.py", &[("main.py", MAIN_PY)])?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &MAIN_PY_OUTPUT.to_owned(), "the declared-returns program's output")
}

/// `size` and `length` count elements only on a receiver whose TYPE is a
/// collection: a `Map`/`Set` for `size`, an array or string for `length`. On
/// an object constant, an annotated object or a `Record`, the member is the
/// source's own property, even though records and maps share one HIR shape.
const COUNT_MEMBERS_TS: &str = r#"const DEFAULTS = { size: 3, length: 9, name: "d" };
const box: { size: number; length: number } = { size: 5, length: 6 };
const scores: Record<string, number> = { size: 11, a: 1 };
const m = new Map<string, number>([["a", 1], ["b", 2]]);
const s = new Set<number>([1, 2, 3, 4]);
const { size, length } = DEFAULTS;
function sizeOf<T extends { size: number }>(value: T): number { return value.size; }
console.log(DEFAULTS.size, DEFAULTS.length, box.size, box.length, scores.size);
console.log(m.size, s.size, [1, 2].length, "abc".length);
console.log(size, length, sizeOf(box));
"#;

/// What Node prints for [`COUNT_MEMBERS_TS`].
const COUNT_MEMBERS_OUTPUT: &str = "3 9 5 6 11\n2 4 2 3\n3 9 5\n";

/// `size`/`length` read a user object's own fields and count only real collections.
#[test]
fn count_members_are_resolved_by_receiver_type() -> TestResult {
    let sources = [("main.ts", COUNT_MEMBERS_TS)];
    ensure_node_agrees(&sources, "main.ts", COUNT_MEMBERS_OUTPUT, "count members")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &COUNT_MEMBERS_OUTPUT.to_owned(), "the count-members program's output")
}

/// Generic record helpers instantiate from their arguments, the way tsc does,
/// never from another module's signature of the same name.
///
/// - An object literal argument keeps its own shape; it is not typed by the
///   callee's uninstantiated `Record<K, V>`, so `K`/`V` bind from it.
/// - A record with a typed non-string key (`Record<V, K>` with `V := number`)
///   reads its own keys (`Object.keys`) and prints as the object JavaScript
///   prints.
/// - `K extends keyof T`, unbound by the value arguments, is a string, so a
///   callback's `key` and the result's keys are typed.
const GENERIC_RECORDS_TS: &str = r#"function invert<K extends PropertyKey, V extends PropertyKey>(obj: Record<K, V>): Record<V, K> {
  const result = {} as Record<V, K>;
  const keys = Object.keys(obj) as K[];
  for (let i = 0; i < keys.length; i++) { const key = keys[i]; const value = obj[key]; result[value] = key; }
  return result;
}
function mapValues<T extends object, K extends keyof T, V>(object: T, getNewValue: (value: T[K], key: K, object: T) => V): Record<K, V> {
  const result = {} as Record<K, V>;
  const keys = Object.keys(object) as K[];
  for (let i = 0; i < keys.length; i++) { const key = keys[i]; result[key] = getNewValue(object[key], key, object); }
  return result;
}
const r = invert({ a: 1, b: 2, c: 3 });
const s = invert({ x: "p", y: "q" });
console.log(r, s, Object.keys(r).length, r[2], s["q"]);
console.log(mapValues({ a: 1, b: 2 }, (_, key) => key + "!"), mapValues({ a: 1, b: 2 }, value => value * 10));
"#;

/// What Node prints for [`GENERIC_RECORDS_TS`].
const GENERIC_RECORDS_OUTPUT: &str =
    "{ '1': 'a', '2': 'b', '3': 'c' } { p: 'x', q: 'y' } 3 b y\n{ a: 'a!', b: 'b!' } { a: 10, b: 20 }\n";

/// Generic record helpers instantiate concretely and behave like Node.
#[test]
fn generic_record_helpers_instantiate_from_their_arguments() -> TestResult {
    let sources = [("main.ts", GENERIC_RECORDS_TS)];
    ensure_node_agrees(&sources, "main.ts", GENERIC_RECORDS_OUTPUT, "generic records")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &GENERIC_RECORDS_OUTPUT.to_owned(), "the generic-records program's output")?;
    let rust = fs::read_dir(root.join("dist/src"))?
        .map(|entry| fs::read_to_string(entry?.path()))
        .collect::<Result<String, _>>()?;
    // Both results are `Record<V, K>` with a type-parameter key, so both are
    // property-key records (`SmeltPropKeyRecord<_>`, an object store keyed by
    // the sealed `SmeltPropKey`; it was `SmeltJsMap<SmeltPropKey, _>` until a
    // record had to share its store with its erased form): whatever `V` a
    // call binds, the record stores `ToPropertyKey` of each key. The values
    // are the inverted KEYS, bound to `K`'s constraint `PropertyKey`, a
    // concrete union, never erased.
    //
    // Old requirement: `let s: SmeltRecord<JsString, JsString>` and
    // `let r: SmeltJsMap<f64, JsString>`. The second stored the raw number key
    // (`r[1]` and `r['1']` were two entries), which is the defect property-key
    // records fix, and `K := string` held the inverted keys only because this
    // program's keys happen to be names: `invert({ 1: 'a' })` makes TypeScript
    // bind `K := 1` while the value is the string `'1'` (`Object.keys(obj) as
    // K[]`), which no `f64` holds. `property_key_cli_tests` runs that case.
    for typed in [
        "let s: SmeltPropKeyRecord<SmeltUnion",
        "let r: SmeltPropKeyRecord<SmeltUnion",
    ] {
        common::ensure(rust.contains(typed), format!("generated crate must spell `{typed}`"))?;
    }
    Ok(())
}

/// A generic function that returns its callback's `any` result as its `T`
/// makes a promise its body never checks, so a call keeps the value the
/// callback produced instead of converting it to the instantiation (es-toolkit
/// `cloneDeepWith` with a customizer that returns `null`). The rule follows a
/// wrapper that forwards its own callback, and a call that passes no callback
/// keeps its instantiation.
const UNCHECKED_RETURNS_TS: &str = r"function withDefault<T>(value: T, replace?: (v: T) => any): T {
  const replaced = replace?.(value);
  if (replaced !== undefined) {
    return replaced;
  }
  return value;
}
function apply<T>(value: T, replace: (v: T) => any): T { return withDefault(value, replace); }
function keep<T>(value: T): T { return withDefault(value, undefined); }
const rec = { a: 1, b: 2 };
const out = apply(rec, (v: any) => {
  if (v.a === 1) {
    return null;
  }
});
const same = apply(rec, () => undefined);
const kept = keep(rec);
console.log(out, same, kept.a + kept.b);
";

/// What Node prints for [`UNCHECKED_RETURNS_TS`].
const UNCHECKED_RETURNS_OUTPUT: &str = "null { a: 1, b: 2 } 3\n";

/// A callback's `any` result returned as `T` reaches the caller unchanged.
#[test]
fn a_callback_result_returned_as_t_keeps_its_runtime_value() -> TestResult {
    let sources = [("main.ts", UNCHECKED_RETURNS_TS)];
    ensure_node_agrees(&sources, "main.ts", UNCHECKED_RETURNS_OUTPUT, "unchecked returns")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &UNCHECKED_RETURNS_OUTPUT.to_owned(), "the unchecked-returns program's output")
}

/// A parameter named like a host global is the parameter, on every path where
/// a host name could be asked first: `new` of a host constructor (`Map`,
/// `Set`, `Promise`, `Date`, `URL`, typed arrays), a namespace call
/// (`Math.max`, `JSON.parse`, `Object.assign`, `Reflect.ownKeys`, `console.log`),
/// a global function call (`String(x)`, `parseInt`, `Symbol`, `isNaN`), a
/// static member read (`Number.MAX_SAFE_INTEGER`, `Symbol.iterator`), `typeof`,
/// and a bare value (`Infinity`, `NaN`, `undefined`, `RegExp`).
const HOST_SHADOW_TS: &str = r#"class Box { v: number; constructor(v: number) { this.v = v; } static of(v: number): Box { return new Box(v * 2); } }
type Ctor = typeof Box;
function a1(Map: Ctor) { return new Map(5).v; }
function a2(Set: Ctor) { return new Set(4).v; }
function a3(Promise: Ctor) { return new Promise(3).v; }
function a4(Date: Ctor) { return new Date(2).v; }
function a5(Error: Ctor) { return new Error(1).v; }
function a6(URL: Ctor) { return new URL(6).v; }
function a7(Array: Ctor) { return new Array(8).v; }
function a8(WeakMap: Ctor) { return new WeakMap(9).v; }
function a9(Uint8Array: Ctor) { return new Uint8Array(10).v; }
function b1(console: { log: (x: string) => number }) { return console.log("z"); }
function b2(Promise: Ctor) { return Promise.of(7).v; }
function b4(Number: { MAX_SAFE_INTEGER: string }) { return Number.MAX_SAFE_INTEGER; }
function b5(Symbol: { iterator: string }) { return Symbol.iterator; }
function b6(Object: { assign: (a: number, b: number) => number }) { return Object.assign(1, 2); }
function b7(Array: { from: (a: string) => number }) { return Array.from("ab"); }
function b8(String: { fromCharCode: (a: number) => number }) { return String.fromCharCode(65); }
function b9(Date: { now: () => string; UTC: (y: number) => string }) { return Date.now() + Date.UTC(1); }
function c1(JSON: { parse: (s: string) => number }) { return JSON.parse("1") + 1; }
function c2(Symbol: { for: (s: string) => string }) { return Symbol.for("k"); }
function c3(String: (x: number) => number) { return String(4) + 1; }
function c4(Number: (x: string) => string) { return Number("8") + "?"; }
function c5(Boolean: (x: number) => number) { return Boolean(0) + 10; }
function c6(parseFloat: (x: string) => string) { return parseFloat("1.5") + "f"; }
function c7(structuredClone: (x: number) => number) { return structuredClone(3) * 2; }
function c8(BigInt: (x: number) => string) { return BigInt(3); }
function c9(Array: (n: number) => string) { return Array(3); }
function d1(Date: string) { return typeof Date; }
function d2(Math: number) { return Math + 1; }
function d3(globalThis: { x: number }) { return globalThis.x; }
function d4(Infinity: string) { return Infinity + "!"; }
function d5(process: { env: string }) { return process.env; }
function d6(Buffer: { from: (s: string) => number }) { return Buffer.from("x"); }
function d7(Intl: { x: number }) { return Intl.x; }
function d8(Reflect: { ownKeys: (x: number) => number }) { return Reflect.ownKeys(1); }
function d9(Object: { keys: (x: number) => number }) { return Object.keys(4); }
function e1(Error: (m: string) => string) { return Error("m"); }
function e2(Symbol: (s: string) => string) { return Symbol("s"); }
function e3(RegExp: (s: string) => number) { return RegExp("s") + 1; }
function e4(String: { raw: (s: string) => string }) { return String.raw("q"); }
function e5(Number: { isInteger: (x: number) => string }) { return Number.isInteger(4); }
function e6(Math: { max: (...xs: number[]) => string }) { return Math.max(1, 2); }
function e7(isNaN: (x: number) => string) { return isNaN(1); }
function e8(encodeURIComponent: (x: string) => number) { return encodeURIComponent("a b") + 1; }
function e9(Promise: { resolve: (x: number) => number }) { return Promise.resolve(1) + 1; }
function f1(RegExp: string) { return RegExp; }
const f2 = (Date: number) => Date + 1;
function f3(NaN: number) { return NaN * 2; }
function f4(undefined: number) { return undefined + 1; }
function f5(Set: Ctor) { return typeof Set; }
console.log(a1(Box), a2(Box), a3(Box), a4(Box), a5(Box), a6(Box), a7(Box), a8(Box), a9(Box));
console.log(b1({ log: (x) => 42 }), b2(Box), b4({ MAX_SAFE_INTEGER: "m" }), b5({ iterator: "it" }), b6({ assign: (a, b) => a + b }), b7({ from: (a) => a.length }), b8({ fromCharCode: (a) => a + 1 }), b9({ now: () => "N", UTC: (y) => "U" + y }));
console.log(c1({ parse: (s) => 10 }), c2({ for: (s) => "F" + s }), c3((x) => x * 10), c4((x) => x + x), c5((x) => x + 1), c6((x) => x + x), c7((x) => x + 1), c8((x) => "B" + x), c9((n) => "A" + n));
console.log(d1("d"), d2(1), d3({ x: 3 }), d4("i"), d5({ env: "E" }), d6({ from: (s) => 5 }), d7({ x: 6 }), d8({ ownKeys: (x) => x + 7 }), d9({ keys: (x) => x * 3 }));
console.log(e1((m) => m + "E"), e2((s) => s + "y"), e3((s) => 2), e4({ raw: (s) => "R" + s }), e5({ isInteger: (x) => "I" + x }), e6({ max: (...xs) => "M" + xs.length }), e7((x) => "N" + x), e8((x) => 3), e9({ resolve: (x) => x + 5 }));
console.log(f1("r"), f2(1), f3(3), f4(4), f5(Box), new Map([[1, 2]]).size, typeof Date, Math.max(1, 2));
"#;

/// What Node prints for [`HOST_SHADOW_TS`].
const HOST_SHADOW_OUTPUT: &str = "5 4 3 2 1 6 8 9 10\n42 14 m it 3 2 66 NU1\n11 Fk 41 88? 11 1.51.5f 8 B3 A3\n\
string 2 3 i! E 5 6 8 12\nmE sy 3 Rq I4 M2 N1 4 7\nr 2 6 5 function 1 function 2\n";

/// Every host-global path resolves a shadowing parameter to the parameter.
#[test]
fn parameters_shadow_host_globals_everywhere() -> TestResult {
    let sources = [("main.ts", HOST_SHADOW_TS)];
    ensure_node_agrees(&sources, "main.ts", HOST_SHADOW_OUTPUT, "host shadowing")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &HOST_SHADOW_OUTPUT.to_owned(), "the host-shadowing program's output")
}
