//! `null` and `undefined` kept apart, end to end against Node.
//!
//! JavaScript has two absent values and a program can tell them apart. Smelt
//! keeps the one a source type admits in the type (`smelt_hir::Absence`): a
//! `T | null` and a `T | undefined` are both a Rust `Option<T>`, whose `None`
//! is the absent value its type admits; a `T | null | undefined` is
//! `Option<Option<T>>`, `undefined` outside. Each test checks the generated
//! program against Node and the emitted types:
//!
//! - **Seams**: printing, `String`/template/`Number`, `===`/`==`, `typeof`,
//!   `??`/`?.`, JSON, narrowing, erasure to and from `unknown`.
//! - **Generic instantiation**: `null` at `V | null` is `None` for every `V`
//!   (it used to be `V::from_unknown(Null)`, `NaN` at `V = number`).
//! - **Default parameters**: only `undefined` takes the default.
//! - **Containers**: `(T | null)[]`, `Map<K, V | null>`, `Set<T | null>` and
//!   tuples print, serialize and erase their `null`s, and an erased typed list
//!   stays a live view of the typed one.
//! - **Asserted mutation**: `(v as number[]).push(9)` on an `unknown[]` (or on
//!   an `unknown` holding an array) reaches the array itself.

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
        "[project]\nname = \"absence\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\nroots = [\"src\"]\n\n\
         [output]\ntarget = \"dist\"\ncrate-name = \"absence\"\nkind = \"program\"\nbuild = false\n",
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

/// The program of the `seams` test.
const SEAMS_TS: &str = r#"interface Rec { a: number | null; b?: string; c?: string | null; }
function mk(k: number): Rec {
  if (k === 0) return { a: null };
  if (k === 1) return { a: 1, b: "x", c: null };
  return { a: 2, c: "y" };
}
for (const k of [0, 1, 2]) {
  const r = mk(k);
  console.log(r.a, r.b, r.c, JSON.stringify(r));
  console.log(`${r.a}|${r.c}`, String(r.a), Number(r.a), r.a ?? -1, r.c ?? "dflt");
  console.log(r.a === null, r.a === undefined, r.c === null, r.c === undefined, r.c == null, typeof r.a, typeof r.c);
}
function pick(f: boolean): number | null { return f ? 1 : null; }
const a = pick(false);
console.log(a, a === null, a === undefined, a == null, typeof a);
function both(k: number): string | null | undefined { return k === 0 ? null : k === 1 ? undefined : "s"; }
for (const k of [0, 1, 2]) {
  const v = both(k);
  console.log(v, v === null, v === undefined, v ?? "dflt", v?.length, typeof v, String(v));
  if (v !== null) { console.log("not null:", v); }
  if (v !== undefined) { console.log("not undefined:", v); }
}
const erased: unknown = a;
console.log(erased === null, erased === undefined, typeof erased);
const back = erased as number | null;
console.log(back, back === null);
const u2: unknown = undefined;
const both2 = u2 as string | null | undefined;
console.log(both2 === undefined, both2 === null);
function keepUndefined(x: string | null | undefined): string {
  if (x !== null) { return x ?? "U"; }
  return "N";
}
function keepNull(x: string | null | undefined): string {
  if (x !== undefined) { return x === null ? "null" : x; }
  return "U";
}
console.log(keepUndefined(null), keepUndefined(undefined), keepUndefined("s"), keepNull(null), keepNull(undefined), keepNull("t"));
"#;

/// What Node prints for [`SEAMS_TS`].
const SEAMS_OUTPUT: &str = concat!(
    "null undefined undefined {\"a\":null}\n",
    "null|undefined null 0 -1 dflt\n",
    "true false false true true object undefined\n",
    "1 x null {\"a\":1,\"b\":\"x\",\"c\":null}\n",
    "1|null 1 1 1 dflt\n",
    "false false true false true number object\n",
    "2 undefined y {\"a\":2,\"c\":\"y\"}\n",
    "2|y 2 2 2 y\n",
    "false false false false false number string\n",
    "null true false true object\n",
    "null true false dflt undefined object null\n",
    "not undefined: null\n",
    "undefined false true dflt undefined undefined undefined\n",
    "not null: undefined\n",
    "s false false s 1 string s\n",
    "not null: s\n",
    "not undefined: s\n",
    "true false object\n",
    "null true\n",
    "true false\n",
    "N U s null U t\n",
);

/// The program of the `generic` test.
const GENERIC_TS: &str = r#"class Cache<V> {
  items: V[] = [];
  first(): V | null { return this.items.length ? this.items[0] : null; }
  isEmpty(): boolean { return this.first() === null; }
  show(): string { return `${this.first()}`; }
}
const c = new Cache<number>();
console.log(c.first(), c.isEmpty(), c.show(), c.first() === undefined);
c.items.push(5);
console.log(c.first(), c.isEmpty(), c.show());
function firstOrNull<T>(xs: T[]): T | null { return xs.length > 0 ? xs[0] : null; }
const none = firstOrNull<number>([]);
console.log(none, none === null, typeof none, firstOrNull(["x"]));
function pick<T>(x: T | null | undefined): string { return x === null ? "N" : x === undefined ? "U" : "V"; }
console.log(pick<number>(null), pick<number>(undefined), pick(1));
function isNotNil<T>(x: T | null | undefined): x is T { return x != null; }
const mixed = [1, 2, null, 4, undefined];
console.log(mixed.filter(isNotNil), isNotNil(null), isNotNil(undefined), isNotNil(0));
"#;

/// What Node prints for [`GENERIC_TS`].
const GENERIC_OUTPUT: &str = concat!(
    "null true null false\n",
    "5 false 5\n",
    "null true object x\n",
    "N U V\n",
    "[ 1, 2, 4 ] false false true\n",
);

/// The program of the `defaults` test.
const DEFAULTS_TS: &str = r#"function d(x: number | null = 5) { return x; }
console.log(d(), d(undefined), d(null), d(3));
const arrow = (s: string | null = "def") => s;
console.log(arrow(), arrow(null), arrow("z"));
function opt(x?: number | null) { return x === undefined ? "u" : x === null ? "n" : "v" + x; }
console.log(opt(), opt(undefined), opt(null), opt(4));
"#;

/// What Node prints for [`DEFAULTS_TS`].
const DEFAULTS_OUTPUT: &str = concat!("5 5 null 3\n", "def null z\n", "u u n v4\n",);

/// The program of the `containers` test.
const CONTAINERS_TS: &str = r#"const xs: (number | null)[] = [1, null, 3];
console.log(xs, JSON.stringify(xs), String(xs[1]), xs[1] === null);
const er: unknown = xs;
console.log((er as unknown[])[1] === null, JSON.stringify(er));
(er as unknown[]).push(null);
console.log(xs.length, xs[3] === null);
xs.push(7);
console.log((er as unknown[]).length, er === xs);
const m = new Map<string, string | null>([["a", null], ["b", "x"]]);
console.log(m, m.get("a") === null, m.get("zz") === undefined);
const em: any = m;
console.log(em.get("a") === null);
const st = new Set<string | null>(["q", null]);
console.log(st, st.has(null));
const tup: [number | null, string] = [null, "t"];
console.log(tup, JSON.stringify(tup));
const mixed: (string | null | undefined)[] = ["a", null, undefined];
console.log(mixed, JSON.stringify(mixed));
"#;

/// What Node prints for [`CONTAINERS_TS`].
const CONTAINERS_OUTPUT: &str = concat!(
    "[ 1, null, 3 ] [1,null,3] null true\n",
    "true [1,null,3]\n",
    "4 true\n",
    "5 true\n",
    "Map(2) { 'a' => null, 'b' => 'x' } true true\n",
    "true\n",
    "Set(2) { 'q', null } true\n",
    "[ null, 't' ] [null,\"t\"]\n",
    "[ 'a', null, undefined ] [\"a\",null,null]\n",
);

/// The program of the `casts` test.
const CASTS_TS: &str = r"function addNine(v: unknown[]) { (v as number[]).push(9); }
const u: unknown[] = [1, 2];
const alias = u;
addNine(u);
console.log(u.length, alias.length, u[2]);
const nums: number[] = [1, 2, 3];
const erased: unknown = nums;
(erased as unknown[]).push(4);
console.log(nums.length);
(erased as number[]).push(5);
console.log(nums.length, erased === nums);
";

/// What Node prints for [`CASTS_TS`].
const CASTS_OUTPUT: &str = concat!("3 3 9\n", "4\n", "5 true\n",);

/// Every seam keeps `null` apart from `undefined`, and the types say which
/// one an `Option`'s `None` is.
#[test]
fn absent_values_keep_their_spelling_through_every_seam() -> TestResult {
    let all = agrees_with_node(SEAMS_TS, SEAMS_OUTPUT, "null/undefined seams")?;
    // Type preservation: one absence is one `Option`, both are two, no
    // `SmeltUnknown` stands in for either.
    ensure(
        all.contains("fn pick(f: bool) -> Option<f64>"),
        "`number | null` is `Option<f64>`",
    )?;
    ensure(
        all.contains("fn both(k: f64) -> Option<Option<JsString>>"),
        "`string | null | undefined` is `Option<Option<JsString>>`",
    )?;
    ensure(
        all.contains("c: Option<Option<JsString>>"),
        "`c?: string | null` keeps both absences",
    )
}

/// `null` at `V | null` is the `None` of `Option<V>` for every `V`.
#[test]
fn null_at_a_type_parameter_is_absent_at_every_instantiation() -> TestResult {
    let all = agrees_with_node(GENERIC_TS, GENERIC_OUTPUT, "null at V | null")?;
    ensure(
        all.contains("fn first(&self) -> Option<V>"),
        "the method keeps its generic `Option<V>`",
    )?;
    ensure(
        !all.contains("smelt_from_unknown((SmeltUnknown::Null)"),
        "`null` is never converted into a `V`",
    )
}

/// A default parameter replaces `undefined` only; `null` is a value.
#[test]
fn defaults_apply_to_undefined_and_not_to_null() -> TestResult {
    let all = agrees_with_node(DEFAULTS_TS, DEFAULTS_OUTPUT, "default parameters")?;
    ensure(
        all.contains("fn d(x: Option<Option<f64>>) -> Option<f64>"),
        "`x: number | null = 5` takes `undefined` on top of its own type",
    )?;
    ensure(
        all.contains("fn opt(x: Option<Option<f64>>)"),
        "`x?: number | null` keeps both absences",
    )
}

/// Containers of `T | null` print, serialize and erase their `null`s, and the
/// erased list is still the typed one.
#[test]
fn containers_keep_their_null_elements() -> TestResult {
    let all = agrees_with_node(CONTAINERS_TS, CONTAINERS_OUTPUT, "nullable containers")?;
    ensure(
        all.contains("xs: SmeltList<Option<f64>>"),
        "`(number | null)[]` stays a typed list",
    )?;
    ensure(
        all.contains(".into_live_smelt_unknown_with("),
        "the typed list erases to a live view that reads `null`",
    )
}

/// A mutation through an assertion at another element type reaches the
/// array itself.
#[test]
fn asserted_mutations_reach_the_array_itself() -> TestResult {
    let all = agrees_with_node(CASTS_TS, CASTS_OUTPUT, "asserted list mutation")?;
    // The parameter keeps its `unknown[]` type; the in-place write back
    // makes it a mutated list argument (`&mut`).
    ensure(
        all.contains("v: &mut SmeltList<SmeltUnknown>"),
        "the parameter keeps its `unknown[]` type",
    )?;
    ensure(
        !all.contains("v = Into::<SmeltList<_>>::into"),
        "the parameter is not rebound to a converted copy",
    )?;
    ensure(
        all.contains("smelt_erased_list_write_back("),
        "an erased receiver takes the elements back in place",
    )
}
