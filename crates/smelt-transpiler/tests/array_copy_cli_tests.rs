//! Array copies, module-binding identity and array holes, end to end against Node.
//!
//! - **`Array.from` copies, typed**: `Array.from(list)` is a new array with the
//!   list's element type (`SmeltList<f64>` from a `number[]`), and
//!   `Array.from(source, mapper)` applies `mapper(element, index)` into a
//!   `SmeltList<U>` of the mapper's return type, over a list, a `Set`, a `Map`
//!   (its entries), a string (its code points), a generator, a narrowed
//!   optional list and `{ length }`. The copies MIR can prove unobservable
//!   (`ListCopyElision`) are gone without changing what the program prints.
//! - **Module bindings keep one identity**: a module-level object, array, `Map`
//!   or `Set` literal that the program hands out (`const alias = local`,
//!   `const erased: unknown = local`, `return items`) is ONE object for every
//!   reader, instead of a fresh replay of its literal per read.
//! - **Holes in a typed `number[]`**: a write past the end (directly or through
//!   an erased handle) and a `length` grown through an erased handle leave
//!   holes that read as `undefined`, show as `<n empty items>`, serialize as
//!   `null`, are skipped by `forEach`/`map`/`filter` and are not `in` the
//!   array, while the array stays a `SmeltList<f64>`.

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
            "[project]\nname = \"arrays\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"arrays\"\nkind = \"program\"\nbuild = false\n"
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

/// `Array.from` over every iterable kind, with and without a mapper, plus the
/// shapes `ListCopyElision` rewrites.
const ARRAY_FROM_TS: &str = r#"const src: number[] = [1, 2, 3];
const cp = Array.from(src);
cp.push(4);
console.log(src.length, cp.length, cp === src);
const doubled = Array.from([1, 2, 3], (x) => x * 2);
console.log(doubled.join(","));
const labels = Array.from(src, (x, i) => `${x}:${i}`);
console.log(labels.join(","));
const s = new Set<number>([5, 6]);
console.log(Array.from(s).join(","), Array.from(s, (v) => v + 1).join(","));
const m = new Map<string, number>([["a", 1], ["b", 2]]);
const entries = Array.from(m);
console.log(JSON.stringify(entries), Array.from(m, ([k, v]) => k + v).join(","));
console.log(Array.from("abc").join("-"), Array.from("abc", (c) => c.toUpperCase()).join(""));
console.log(Array.from({ length: 3 }, (_, i) => i * 10).join(","));
function* gen(): Generator<number> { yield 1; yield 2; }
console.log(Array.from(gen()).join(","), Array.from(gen(), (x) => x * 3).join(","));
function sum(xs: number[]): number { let t = 0; for (const x of xs) t += x; return t; }
function fromOptional(items: number[] | undefined): number {
  if (items === undefined) return -1;
  const copy = Array.from(items);
  copy.push(100);
  return sum(copy) + items.length;
}
console.log(fromOptional([1, 2]), fromOptional(undefined));
function tail(xs: number[]): number[] { return Array.from(xs).slice(1); }
const t = tail(src);
t.push(9);
console.log(t.join(","), src.join(","));
function values(map: Map<string, number>): number[] { return Array.from(map.values()); }
console.log(values(m).join(","));
const target: number[] = [0];
target.push(...Array.from(src));
target.push(...target);
console.log(target.join(","));
const erased: unknown = src;
const fromErased = Array.from(erased as number[], (x) => x + 0.5);
console.log(fromErased.join(","));
console.log(Array.from(src).push(5), src.length);
"#;

/// What Node prints for [`ARRAY_FROM_TS`].
const ARRAY_FROM_OUTPUT: &str = concat!(
    "3 4 false\n",
    "2,4,6\n",
    "1:0,2:1,3:2\n",
    "5,6 6,7\n",
    "[[\"a\",1],[\"b\",2]] a1,b2\n",
    "a-b-c ABC\n",
    "0,10,20\n",
    "1,2 3,6\n",
    "105 -1\n",
    "2,3,9 1,2,3\n",
    "1,2\n",
    "0,1,2,3,0,1,2,3\n",
    "1.5,2.5,3.5\n",
    "4 3\n",
);

/// `Array.from` copies and maps with the source's element type.
#[test]
fn array_from_copies_and_maps_typed() -> TestResult {
    let all = agrees_with_node(ARRAY_FROM_TS, ARRAY_FROM_OUTPUT, "Array.from")?;
    // Type preservation: the copy of a `number[]` is a `SmeltList<f64>`, a
    // string-returning mapper gives a `SmeltList<JsString>`, a `Map`'s
    // entries are typed pairs, and none of them goes through `SmeltUnknown`.
    ensure(
        all.contains(
            "let mut cp: SmeltList<f64> = Into::<SmeltList<_>>::into(src.clone().fresh_copy())",
        ),
        "the copy of a `number[]` is a `SmeltList<f64>`",
    )?;
    ensure(
        all.contains("let labels: SmeltList<JsString>"),
        "a string-returning mapper gives a `SmeltList<JsString>`",
    )?;
    ensure(
        all.contains("let entries: SmeltList<(JsString, f64)>"),
        "`Array.from(map)` is the map's typed entries",
    )?;
    ensure(
        all.contains("let doubled: SmeltList<f64>"),
        "a number mapper gives a `SmeltList<f64>`",
    )?;
    ensure(
        all.contains(".fresh_copy()"),
        "a copy that can be observed is still made",
    )?;
    ensure(
        !all.contains("Array.from(iterable)"),
        "no typed source falls back to the erased read",
    )
}

/// Module-level aggregates that are handed out keep one identity.
const MODULE_IDENTITY_TS: &str = r#"const local = [1, 2];
const erased: unknown = local;
const alias = local;
const lobj = { a: 1 };
const eobj: unknown = lobj;
const aobj = lobj;
const lmap = new Map<string, number>([["k", 1]]);
const emap: unknown = lmap;
const amap = lmap;
const lset = new Set<number>([1]);
const aset = lset;
const items = [1, 2];
function get(): number[] { return items; }
const conf = { a: 1 };
function setConf(): void { conf.a = 5; }
function run(): void {
  (erased as number[]).push(3);
  alias.push(4);
  console.log(erased === local, alias === local, local.join(","), (erased as number[]).length);
  aobj.a = 7;
  console.log(eobj === lobj, aobj === lobj, lobj.a);
  amap.set("z", 2);
  (emap as Map<string, number>).set("y", 3);
  console.log(emap === lmap, amap === lmap, lmap.size);
  aset.add(2);
  console.log(aset === lset, lset.size);
}
run();
console.log(local.length, lobj.a, lmap.size, lset.size);
console.log(get() === get());
get().push(3);
console.log(items.length, get().length);
setConf();
console.log(conf.a);
"#;

/// What Node prints for [`MODULE_IDENTITY_TS`].
const MODULE_IDENTITY_OUTPUT: &str = concat!(
    "true true 1,2,3,4 4\n",
    "true true 7\n",
    "true true 3\n",
    "true 2\n",
    "4 7 3 2\n",
    "true\n",
    "3 3\n",
    "5\n",
);

/// A module binding read from several places is one object, not a replay.
#[test]
fn module_aggregates_keep_one_identity() -> TestResult {
    let all = agrees_with_node(
        MODULE_IDENTITY_TS,
        MODULE_IDENTITY_OUTPUT,
        "module aggregate identity",
    )?;
    // Type preservation: each binding is one typed module slot, and the alias
    // of a `Map` is a `Map` (it was typed as a record by the literal replay).
    ensure(
        all.contains("::std::cell::RefCell<SmeltList<f64>>"),
        "the array literal is one typed `SmeltList<f64>` slot",
    )?;
    ensure(
        !all.contains("RefCell<SmeltRecord<JsString, f64>> = ::std::cell::RefCell::new(smelt_global_init__amap"),
        "the alias of a `Map` is not a record",
    )?;
    ensure(
        all.contains("RefCell<SmeltJsMap<JsString, f64>> = ::std::cell::RefCell::new(smelt_global_init__amap"),
        "the alias of a `Map` is a `SmeltJsMap`",
    )
}

/// Holes left in a typed `number[]`, through an erased handle and directly.
const HOLES_TS: &str = r#"const a: number[] = [1, 2, 3, 4];
const v: any = a;
v[6] = 9;
console.log(a, a[5], a.length, JSON.stringify(a), 5 in a, 6 in a);
console.log(v[5], v.length, String(a[5]), a.join("-"));
let count = 0;
a.forEach(() => { count++; });
console.log(count, a.map((x) => x * 2), a.filter(() => true).length, a.some((x) => Number.isNaN(x)));
const b: number[] = [1];
const w: any = b;
w.length = 3;
console.log(b, b.length, 1 in b);
const c: number[] = [1];
c[3] = 2;
console.log(c, c[1], c[2] + 1);
"#;

/// What Node prints for [`HOLES_TS`].
const HOLES_OUTPUT: &str = concat!(
    "[ 1, 2, 3, 4, <2 empty items>, 9 ] undefined 7 [1,2,3,4,null,null,9] false true\n",
    "undefined 7 undefined 1-2-3-4---9\n",
    "5 [ 2, 4, 6, 8, <2 empty items>, 18 ] 5 false\n",
    "[ 1, <2 empty items> ] 3 false\n",
    "[ 1, <2 empty items>, 2 ] undefined NaN\n",
);

/// Holes in a typed `number[]` behave as JavaScript's.
#[test]
fn number_array_holes_read_as_javascript() -> TestResult {
    let all = agrees_with_node(HOLES_TS, HOLES_OUTPUT, "number array holes")?;
    // Type preservation: the arrays stay `SmeltList<f64>`; a hole is a stored
    // `f64`, not a change of element type.
    ensure(
        all.contains("let a: SmeltList<f64>") || all.contains("let mut a: SmeltList<f64>"),
        "`a` stays a `SmeltList<f64>`",
    )?;
    ensure(
        all.contains("let c: SmeltList<f64>") || all.contains("let mut c: SmeltList<f64>"),
        "`c` stays a `SmeltList<f64>`",
    )?;
    ensure(
        all.contains("smelt_number_hole()"),
        "a direct write past the end stores the hole",
    )
}

/// One function per `ListCopyElision` rule, plus copies that must stay.
const ELISION_TS: &str = r"export function freshSource(m: Map<string, number>): number[] { return Array.from(m.values()); }
export function elementConsumer(xs: number[]): number[] { return Array.from(xs).slice(1); }
export function coercionSource(items: number[] | undefined): number[] {
  if (items === undefined) return [];
  return Array.from(items);
}
export function observedCopy(xs: number[]): number[] { const c = Array.from(xs); c.push(1); return c; }
export function pushedCopy(xs: number[]): number { return Array.from(xs).push(1); }
export function escapingCopy(xs: number[], keep: (ys: number[]) => void): void { keep(Array.from(xs)); }
";

/// The MIR body of the function `name` in a `dump-mir` listing.
fn mir_body<'a>(mir: &'a str, name: &str) -> &'a str {
    let header = format!("fn {name} ");
    let start = mir.find(&header).unwrap_or(mir.len());
    let rest = &mir[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    &rest[..end]
}

/// `ListCopyElision` drops exactly the unobservable copies.
#[test]
fn unobservable_list_copies_are_elided() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &[("main.ts", ELISION_TS)])?;
    let mir = smelt_in(root, &["dump-mir", "src/main.ts"])?;
    // Fresh source: the projection is the result, no copy of it.
    ensure(
        !mir_body(&mir, "fresh_source").contains("list_copy"),
        "a copy of a freshly built list is elided",
    )?;
    // Element-only consumer: `slice` reads the parameter itself.
    let consumer = mir_body(&mir, "element_consumer");
    ensure(
        !consumer.contains("list_copy") && consumer.contains("list_slice copy %0"),
        "a copy only sliced is elided",
    )?;
    // Coercion source: the copy reads the narrowed optional directly, with no
    // narrowing temporary in between.
    ensure(
        mir_body(&mir, "coercion_source").contains("list_copy copy %0"),
        "the copy reads the optional parameter itself",
    )?;
    // Observable copies stay: one is mutated, one escapes into a callback.
    ensure(
        mir_body(&mir, "observed_copy").contains("list_copy"),
        "a mutated copy is kept",
    )?;
    ensure(
        mir_body(&mir, "pushed_copy").contains("list_copy"),
        "a copy pushed to in place is kept",
    )?;
    ensure(
        mir_body(&mir, "escaping_copy").contains("list_copy"),
        "a copy handed to unknown code is kept",
    )
}

/// A tuple element whose union has a `symbol` arm, tested against `null`
/// (es-toolkit's `zipObjectDeep` shape once `Array.from` typed its keys).
const UNION_NULLISH_TS: &str = r#"type Key = string | number | symbol | readonly (string | number)[];
function zip2<A, B>(a: A[], b: B[]): [A, B][] { return a.map((x, i) => [x, b[i]]); }
function total(keys: Key[], values: number[]): number {
  const zipped = zip2(keys, values);
  let n = 0;
  for (let i = 0; i < zipped.length; i++) {
    const [key, value] = zipped[i];
    if (key != null) { n += value; }
  }
  return n;
}
console.log(total(["a", 1], [2, 3]));
"#;

/// A nullish test of a dynamically shaped union compiles and answers JS's.
#[test]
fn nullish_test_of_a_symbol_union_compiles() -> TestResult {
    let all = agrees_with_node(UNION_NULLISH_TS, "5\n", "union nullish test")?;
    // Type preservation: the key stays its generated union, erased only for
    // the test itself.
    ensure(
        all.contains("let mut key: SmeltUnion") || all.contains("let key: SmeltUnion"),
        "the key keeps its union type",
    )
}

/// A generic function whose `T` is only moved and copied, beside erased
/// values that never mention it: an `unknown` presence test, an `any[]`
/// result, a spread into an erased callback, a mapped `Array.from` copy.
const LIFTED_COPIES_TS: &str = r#"function isArrayLikeValue(value: unknown): boolean {
  return value != null && typeof value === "object";
}
function rows<T>(array: T[][] | null | undefined, iteratee?: ((...args: any[]) => unknown) | null): any[] {
  if (!isArrayLikeValue(array) || !array || !array.length) {
    return [];
  }
  const copied = Array.from(array, row => Array.from(row));
  const result: any[] = [];
  for (let i = 0; i < copied.length; i++) {
    result.push(iteratee ? iteratee(...copied[i]) : copied[i].length);
  }
  return result;
}
const source = [[1, 2], [3, 4, 5]];
const out = rows(source, (...xs: number[]) => xs.reduce((a, b) => a + b, 0));
source[0].push(100);
console.log(out.join(","), rows(source).join(","), rows(null).length, source[0].length);
"#;

/// What Node prints for [`LIFTED_COPIES_TS`].
const LIFTED_COPIES_OUTPUT: &str = "3,12 3,3 0 3\n";

/// Erasure that never touches `T` leaves the function generic, so its copies
/// are typed `SmeltList<SmeltList<T>>` rather than erased.
#[test]
fn copies_keep_the_lifted_type_parameter() -> TestResult {
    let all = agrees_with_node(LIFTED_COPIES_TS, LIFTED_COPIES_OUTPUT, "lifted copies")?;
    ensure(
        // `T[][] | null | undefined` keeps both absences: `Option<Option<..>>`
        // (it was one `Option` while `null` and `undefined` shared it).
        all.contains("fn rows<T: ")
            && all.contains("array: Option<Option<SmeltList<SmeltList<T>>>>"),
        "`rows` keeps its `T` through the copies",
    )
}

/// Seams where a lifted `T` would meet the erased carrier. Erasing a
/// `SmeltList<T>` builds a new buffer that keeps the array's identity, so a
/// lifted function may hand its array only to a callee that just reads it.
/// `mutating` hands it to a callee that pushes, and `forward` returns it as
/// `unknown`, so both stay erased and share the caller's buffer. `reading`
/// only feeds a predicate and keeps its `T`. `share` recovers its erased
/// parameter as a `T[]` for a callback that pushes, so it stays erased too:
/// the recovered copy would hide the push. `retained` and
/// `grownThroughResult` hand their array to generic callees whose call sites
/// demote (a packed rest): `keep` keeps a handle on it and `passBack` returns
/// it to be pushed through, so a copy there would hide the later push from
/// one side; both stay erased. `orDefault` reads an absent
/// optional `D` as `D`, which only the erased carrier can hold (`undefined`).
const ERASED_SEAMS_TS: &str = r"function grow(value: unknown): void {
  (value as unknown[]).push(9);
}
function peek(value: unknown): boolean {
  return Array.isArray(value);
}
function mutating<T>(xs: T[]): number {
  grow(xs);
  return xs.length;
}
function reading<T>(xs: T[]): number {
  return peek(xs) ? xs.length : -1;
}
function forward<T>(xs: T[]): unknown {
  return xs;
}
function share<T>(u: unknown[], seed: T[], cb: (xs: T[]) => void): number {
  cb(seed);
  cb(u as T[]);
  return u.length;
}
let kept: unknown[] = [];
function keep<U>(arr: U[], ...rest: U[]): number {
  kept = arr as unknown[];
  return rest.length;
}
function retained<T>(xs: T[], v: T): number {
  keep(xs, v);
  xs.push(v);
  return kept.length;
}
function passBack<U>(x: U[], ...rest: U[]): U[] {
  return rest.length > 0 ? x : x;
}
function grownThroughResult<T>(xs: T[], v: T): number {
  const back = passBack(xs, v);
  back.push(v);
  return xs.length;
}
function orDefault<D = unknown>(value: any, fallback?: D): D {
  if (value === null) return fallback as D;
  return value;
}
const u: unknown[] = [1, 2];
console.log(mutating(u), u.length);
console.log(reading(u), reading([1, 2, 3, 4]));
const f = forward(u) as unknown[];
f.push(4);
console.log(u.length, f === u);
console.log(String(orDefault(null)), String(orDefault(null, 7)));
const shared: unknown[] = [1];
const seed: unknown[] = [];
console.log(share(shared, seed, (xs: unknown[]) => { xs.push(2); }), shared.length, seed.length);
const held: unknown[] = [1];
console.log(retained(held, 2), grownThroughResult(held, 3));
";

/// What Node prints for [`ERASED_SEAMS_TS`].
const ERASED_SEAMS_OUTPUT: &str = "3 3\n3 4\n4 true\nundefined 7\n2 2 1\n2 3\n";

/// A lifted function keeps `T` only where no seam with the erased carrier
/// copies its array or needs `undefined` in a `T` slot.
#[test]
fn erased_seams_keep_shared_storage() -> TestResult {
    let all = agrees_with_node(ERASED_SEAMS_TS, ERASED_SEAMS_OUTPUT, "erased seams")?;
    ensure(
        all.contains("fn reading<T: ") && all.contains("(xs: SmeltList<T>) -> f64"),
        "`reading` only hands its array to a read-only predicate and keeps `T`",
    )?;
    ensure(
        all.contains("fn mutating(xs: SmeltList<SmeltUnknown>)"),
        "`mutating` hands its array to a callee that pushes, so it shares the erased buffer",
    )?;
    ensure(
        all.contains("fn forward(xs: SmeltList<SmeltUnknown>)"),
        "`forward` returns its array as `unknown`, so it shares the erased buffer",
    )?;
    ensure(
        all.contains("fn retained(") && !all.contains("fn retained<"),
        "`retained` hands its array to a callee that keeps it, so it stays erased",
    )?;
    ensure(
        all.contains("fn grown_through_result(") && !all.contains("fn grown_through_result<"),
        "`grownThroughResult` pushes through the array a callee handed back, so it stays erased",
    )?;
    ensure(
        all.contains("fn share(") && !all.contains("fn share<"),
        "`share` recovers a shared erased list as `T[]`, so it stays erased",
    )?;
    ensure(
        all.contains(
            "fn or_default(value: SmeltUnknown, fallback: Option<SmeltUnknown>) -> SmeltUnknown",
        ),
        "`orDefault` returns an absent optional as `undefined`, which a real `D` cannot hold",
    )
}

/// A `&mut` list argument that monomorphizes its callee binds the callee's
/// type parameter for the other arguments too: `ys: T[]` of `addAll(nums,
/// [7, 8])` is passed as the caller's `SmeltList<f64>`, not erased to the
/// unbound `T[]` (which was E0308 against the instantiated parameter).
const MUT_LIST_BINDING_TS: &str = r#"function addAll<T>(xs: T[], ys: T[]): number {
  for (const y of ys) xs.push(y);
  return xs.length;
}
const nums: number[] = [1];
const alias = nums;
console.log(addAll(nums, [7, 8]), alias.length, nums.join(","));
"#;

/// What Node prints for [`MUT_LIST_BINDING_TS`].
const MUT_LIST_BINDING_OUTPUT: &str = "3 3 1,7,8\n";

/// The mutable-list call adapter renders every argument under the bindings
/// its monomorphized list argument fixes.
#[test]
fn mut_list_argument_binds_the_other_arguments() -> TestResult {
    let all = agrees_with_node(
        MUT_LIST_BINDING_TS,
        MUT_LIST_BINDING_OUTPUT,
        "mut list binding",
    )?;
    ensure(
        all.contains("fn add_all<T: ") && all.contains("ys: SmeltList<T>"),
        "`addAll` keeps its `T`",
    )?;
    ensure(
        !all.contains("SmeltUnknown::Number(smelt_value as f64)"),
        "`ys` reaches the instantiated `SmeltList<f64>` without being erased",
    )
}

/// Shapes from the library gates (date-fns, ts-pattern, radash, remeda):
/// - `new Holder<V>(this.validate, value)` hands a bounded generic method,
///   inherited by `NumberParser` (no slot holds it), to a generic class's
///   callback slot: the method value is bound to the receiver's copy at the
///   receiver's type arguments (`validate` takes `f64` there);
/// - `every<T>` reads `record[key]` at its lifted `T`, whose miss is the
///   generic's default, not the erased `undefined`;
/// - `draw<T>` returns `null` at `T | null`, which only the erased carrier
///   keeps apart from `undefined`, so it stays erased;
/// - `orFallback<T>` asks `data ?? fallback` of a bare `T`, which inspects
///   it, so it stays erased;
/// - `first in RULES` probes a module record with an `unknown` key, which
///   goes through `ToPropertyKey`.
const LIBRARY_SHAPES_TS: &str = r#"class Holder<V> {
  check: <K extends string>(key: K, value: V) => boolean;
  value: V;
  constructor(check: <K extends string>(key: K, value: V) => boolean, value: V) {
    this.check = check;
    this.value = value;
  }
  ok(): boolean {
    return this.check("k", this.value);
  }
}
abstract class Parser<V> {
  run(value: V): boolean {
    return new Holder<V>(this.validate, value).ok();
  }
  protected validate<K extends string>(key: K, value: V): boolean {
    return key.length > 0 && value !== undefined;
  }
}
class NumberParser extends Parser<number> {}
class TextParser extends Parser<string> {
  protected validate<K extends string>(key: K, value: string): boolean {
    return key.length > 0 && value.length > 0;
  }
}
function every<T>(record: Record<string, T>, predicate: (key: string, value: T) => boolean): boolean {
  for (const key of Object.keys(record)) {
    if (!predicate(key, record[key])) return false;
  }
  return true;
}
function draw<T>(array: readonly T[]): T | null {
  if (array.length === 0) return null;
  return array[0];
}
function orFallback<T>(data: T, fallback: T): T {
  return data ?? fallback;
}
const RULES = { asc: 1, desc: -1 } as const;
function isRule(x: unknown): boolean {
  const [first, ...rest] = [x];
  return typeof first === "string" && first in RULES && rest.length === 0;
}
const rules: unknown = RULES;
console.log(new NumberParser().run(3), new TextParser().run(""), new TextParser().run("a"));
console.log(every({ a: 1, b: 2 }, (k, v) => v > 0), every({ a: 1 }, (k, v) => v > 1));
console.log(draw([]) === null, draw([true]), draw<boolean>([]) === null);
console.log(orFallback<number | null>(null, 5), orFallback(0, 5));
console.log(isRule("desc"), isRule("up"), rules !== undefined);
"#;

/// What Node prints for [`LIBRARY_SHAPES_TS`].
const LIBRARY_SHAPES_OUTPUT: &str =
    "true false true\ntrue false\ntrue true true\n5 0\ntrue false true\n";

/// The library-gate shapes agree with Node and keep the types above.
#[test]
fn library_gate_shapes_agree_with_node() -> TestResult {
    let all = agrees_with_node(LIBRARY_SHAPES_TS, LIBRARY_SHAPES_OUTPUT, "library shapes")?;
    ensure(
        all.contains("fn every<T: ") && all.contains("record: SmeltRecord<JsString, T>"),
        "`every` keeps its `T` through the record read",
    )?;
    ensure(
        all.contains("fn draw(array: SmeltList<SmeltUnknown>) -> Option<SmeltUnknown>"),
        "`draw` returns `null` at `T | null`, so it stays erased",
    )?;
    ensure(
        all.contains("fn or_fallback(data: SmeltUnknown, fallback: SmeltUnknown)"),
        "`orFallback` inspects a bare `T` with `??`, so it stays erased",
    )
}

/// Module `a`: a private `patterns` handed out by `return`, so it is one module
/// slot.
const SHADOW_A_TS: &str = r"const patterns: Record<string, number> = { x: 1 };
export function getA(): Record<string, number> {
  return patterns;
}
";

/// Module `b`: its OWN private `patterns`, read inside an object literal
/// argument as date-fns' locales read `matchMonthPatterns`. It is not handed
/// out, so it stays a folded constant.
const SHADOW_B_TS: &str = r"const patterns: Record<string, number> = { x: 2 };
function pick(o: { p: Record<string, number> }): number {
  return o.p.x;
}
export function getB(): number {
  return pick({ p: patterns });
}
";

/// The entry: lowers `a` before `b`.
const SHADOW_MAIN_TS: &str = r#"import { getA } from "./a.ts";
import { getB } from "./b.ts";
console.log(getA().x, getB());
"#;

/// A module's own binding shadows a same-named private slot of another module.
///
/// Requirement (behaviour): `b`'s `patterns` is `b`'s object, as JavaScript's
/// module scope has it; Node prints `1 2`. Every builder sees earlier modules'
/// items by bare name, and `b` used to read `a`'s slot (printing `1 1`): the
/// date-fns `eo`/`fr`/`zh-*` locales matched with `en-US`'s patterns.
/// Requirement (types): `a`'s slot stays a typed `SmeltRecord<JsString, f64>`.
#[test]
fn module_bindings_shadow_another_modules_private_slot() -> TestResult {
    let sources = [
        ("a.ts", SHADOW_A_TS),
        ("b.ts", SHADOW_B_TS),
        ("main.ts", SHADOW_MAIN_TS),
    ];
    let expected = "1 2\n";
    let what = "module binding shadows another module's slot";
    ensure_node_agrees(&sources, "main.ts", expected, what)?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &expected.to_owned(), what)?;
    let mut all = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        all.push_str(&fs::read_to_string(entry?.path())?);
    }
    ensure(
        all.contains("::std::cell::RefCell<SmeltRecord<JsString, f64>>"),
        "`a`'s `patterns` is one typed `SmeltRecord<JsString, f64>` slot",
    )
}
