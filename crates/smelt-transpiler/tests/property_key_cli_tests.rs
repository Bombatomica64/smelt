//! JavaScript property keys and symbols, end to end.
//!
//! Each program builds through the CLI and must print what Node prints for the
//! same source (`node --experimental-transform-types`; the oracle is re-run by
//! [`ensure_node_agrees`] when a TypeScript-capable `node` is present):
//!
//! - **Property keys.** A record whose key is a type parameter or holds
//!   symbols stores `ToPropertyKey` of every key: `r[1]` and `r['1']` reach one
//!   entry, a symbol key never meets the string of its description, and
//!   `Object.keys`/`values`/`entries`/`for...in` follow Node's own-key order
//!   (integer-like keys ascending, then strings, then symbols).
//! - **`invert` and `groupBy`.** es-toolkit's `invert` on mixed, numeric,
//!   symbol and empty records, and the generic `groupBy` round trip with
//!   string, number and symbol group keys.
//! - **Symbols.** `typeof`, `Symbol.for` identity, a fresh symbol per
//!   `Symbol()` evaluation, `description`, `String(sym)`, and a symbol in a
//!   `PropertyKey` slot keeping its kind.
//! - **`null` versus `undefined`.** A ternary `c ? null : undefined` (and a
//!   `null | undefined` annotation) keeps which of the two it returned.
//! - **Call shapes.** A `string | number | symbol` key into a property-key
//!   record, a call through callables of different arity, and a call of an
//!   erased value narrowed to `function`.
//! - **Objects and functions.** remeda's `setPath` (object rest and spread
//!   of a property-key record), `clone` (a record filled after it was erased
//!   into a list) and `isProjection` (`f.length` of an erased callable).
//! - **Erased functions.** A value `typeof` proves a function: its calls,
//!   `call`/`apply` with `this`, `length`, and a throw inside `try`.
//! - **Symlinked paths.** Building through a symlink to the project gives the
//!   same crate as building through the real directory, except for the
//!   project path the build was given, which reported paths keep as spelled.

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in};

/// Writes a one-entry TypeScript project at `root` around `source`.
fn write_project(root: &Path, source: &str) -> TestResult {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src").join("main.ts"), source)?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"keys\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\nroots = [\"src\"]\n\n\
         [output]\ntarget = \"dist\"\ncrate-name = \"keys\"\nkind = \"program\"\nbuild = false\n",
    )?;
    Ok(())
}

/// Checks `source` against Node, then builds it and checks the generated
/// program prints the same.
fn ensure_prints_like_node(source: &str, expected: &str, name: &str) -> TestResult {
    let sources = [("main.ts", source)];
    ensure_node_agrees(&sources, "main.ts", expected, name)?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, source)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &expected.to_owned(), format!("the {name} program's output"))
}

/// Property-key records: one entry per property key, Node's key order, and
/// symbol keys apart from strings. Every access goes through a generic
/// `K extends PropertyKey` helper, so the key reaches the record raw (a
/// number, a string, a symbol) and the record has to convert it.
const PROPERTY_KEYS_TS: &str = r"function put<K extends PropertyKey>(r: Record<K, string>, k: K, v: string): void { r[k] = v; }
function get<K extends PropertyKey>(r: Record<K, string>, k: K): string { return r[k]; }
function has<K extends PropertyKey>(r: Record<K, string>, k: K): boolean { return k in r; }
function drop<K extends PropertyKey>(r: Record<K, string>, k: K): boolean { return delete r[k]; }

// r[1] and r['1'] name one entry.
const r = {} as Record<PropertyKey, string>;
put(r, 1, 'one');
put(r, '1', 'uno');
console.log(get(r, 1), get(r, '1'), has(r, '1'), has(r, 1), Object.keys(r).length);

// Node's own-key order: integer-like keys ascending, then strings, then symbols.
const s = Symbol('k');
put(r, 'b', 'B');
put(r, s, 'sym');
put(r, 2, 'two');
put(r, 'a', 'A');
put(r, 10, 'ten');
put(r, '0', 'zero');
put(r, 'k', 'str');
put(r, '01', 'lead');
put(r, -1, 'neg');
console.log(Object.keys(r).join(','));
console.log(Object.values(r).join(','));
console.log(Object.entries(r).map(([key, value]) => key + '=' + value).join(','));
const forIn: string[] = [];
for (const key in r) { forIn.push(key); }
console.log(forIn.join(','));

// A symbol key stays apart from the string of its description.
console.log(get(r, s), get(r, 'k'), has(r, s), has(r, 'Symbol(k)'), Object.getOwnPropertySymbols(r).length);
console.log(drop(r, s), has(r, s), get(r, 'k'), drop(r, 1), has(r, '1'), Object.keys(r).length);

// A statically known symbol key in a literal names the symbol.
const K = Symbol('K');
const tagged: Record<PropertyKey, string> = { [K]: 'k', K: 'string K' };
console.log(tagged[K], tagged['K'], Object.keys(tagged).join(','));

// Typed number keys, written and read directly.
const direct: Record<PropertyKey, string> = {};
direct[1] = 'n';
direct[1.5] = 'f';
direct[-0] = 'zero';
console.log(direct['1'], direct['1.5'], direct['0'], Object.keys(direct).join(','), 1 in direct, '1.5' in direct);
";

/// What Node prints for [`PROPERTY_KEYS_TS`].
const PROPERTY_KEYS_OUTPUT: &str = "uno uno true true 1
0,1,2,10,b,a,k,01,-1
zero,uno,two,ten,B,A,str,lead,neg
0=zero,1=uno,2=two,10=ten,b=B,a=A,k=str,01=lead,-1=neg
0,1,2,10,b,a,k,01,-1
sym str true false 1
true false str true false 8
k string K K
n f zero 0,1,1.5 true true
";

/// `r[1]` and `r['1']` are one entry, keys enumerate in Node's order, and a
/// symbol key is not the string of its description.
#[test]
fn property_key_records_convert_every_key() -> TestResult {
    ensure_prints_like_node(PROPERTY_KEYS_TS, PROPERTY_KEYS_OUTPUT, "property-key records")
}

/// es-toolkit's `invert` (verbatim) and the generic `groupBy` shape.
const INVERT_GROUP_BY_TS: &str = r#"function invert<K extends PropertyKey, V extends PropertyKey>(obj: Record<K, V>): Record<V, K> {
  const result = {} as Record<V, K>;
  const keys = Object.keys(obj) as K[];
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    const value = obj[key];
    result[value] = key;
  }
  return result;
}

function groupBy<T, K extends PropertyKey>(items: T[], keyOf: (item: T) => K): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (const item of items) {
    const key = keyOf(item);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[key].push(item);
  }
  return result;
}
function joinGroup<K extends PropertyKey>(groups: Record<K, number[]>, key: K): string {
  return groups[key].join(",");
}

const mixed = invert({ a: 1, 2: 'b', c: 3, 4: 'd' });
console.log(Object.keys(mixed).join(','), Object.values(mixed).join(','), typeof Object.values(mixed)[1]);
const numeric = invert({ 1: 'a', 2: 'b' });
console.log(Object.entries(numeric).map(([k, v]) => k + ':' + typeof v + ':' + v).join(','));
const sym1 = Symbol('sym1');
const sym2 = Symbol('sym2');
const bySymbol = invert({ a: sym1, b: sym2 });
console.log(bySymbol[sym1], bySymbol[sym2], Object.keys(bySymbol).length, Object.getOwnPropertySymbols(bySymbol).length);
console.log(Object.keys(invert({})).length);

const parity = groupBy([1, 2, 3, 4, 5, 6], (item) => (item % 2 === 0 ? 'even' : 'odd'));
console.log(joinGroup(parity, 'odd'), joinGroup(parity, 'even'), Object.keys(parity).join(','));
const mod = groupBy([10, 21, 32, 43, 54], (item) => item % 2);
console.log(joinGroup(mod, 0), joinGroup(mod, 1), Object.keys(mod).join(','));
const bySym = groupBy([1, 2, 3], (item) => (item > 1 ? sym1 : 'small'));
console.log(joinGroup(bySym, sym1), joinGroup(bySym, 'small'), Object.keys(bySym).join(','));
"#;

/// What Node prints for [`INVERT_GROUP_BY_TS`].
const INVERT_GROUP_BY_OUTPUT: &str = "1,3,b,d a,c,2,4 string
a:string:1,b:string:2
a b 0 2
0
1,3,5 2,4,6 odd,even
10,32,54 21,43 0,1
2,3 1 small
";

/// `invert` on mixed, numeric, symbol and empty records keeps every key a
/// property key (the inverted values are the original keys, as strings), and
/// `groupBy` round-trips number, string and symbol group keys.
#[test]
fn invert_and_group_by_follow_node() -> TestResult {
    ensure_prints_like_node(INVERT_GROUP_BY_TS, INVERT_GROUP_BY_OUTPUT, "invert and groupBy")
}

/// Symbol values and the `null`/`undefined` distinction.
const SYMBOLS_TS: &str = r"const a = Symbol('a');
const b = Symbol('a');
const r1 = Symbol.for('reg');
const r2 = Symbol.for('reg');
const plain = Symbol();
function same(x: symbol, y: symbol): boolean { return x === y; }
function kind(x: PropertyKey): string { return typeof x; }
console.log(typeof a, typeof r1, kind(a), kind('a'), kind(1));
console.log(same(a, a), same(a, b), same(r1, r2), same(r1, a));
console.log(a.description, r1.description, plain.description === undefined, String(a), String(r1));
console.log(Symbol.iterator === Symbol.iterator, typeof Symbol.iterator);
const keys: PropertyKey[] = [a, 'a', 1, r1];
console.log(keys.map((key) => typeof key).join(','));

// Every `Symbol()` evaluation is a new symbol; a constant is one evaluation.
function mk(): symbol { return Symbol('x'); }
const m1 = mk();
const m2 = mk();
function f(): number { return 1; }
f.placeholder = Symbol('placeholder');
console.log(m1 === m2, m1 === m1, String(m2), f.placeholder === f.placeholder);

// `c ? null : undefined` keeps which of the two it is.
const pick = (c: boolean) => (c ? null : undefined);
const n = pick(true);
const u = pick(false);
console.log(n, u, n === null, u === undefined, n === undefined, u === null, n == undefined, typeof n, typeof u);
function nil(c: boolean): null | undefined { return c ? null : undefined; }
console.log(nil(true), nil(false), [pick(true), pick(false), nil(true)]);
const erased: unknown = pick(true);
console.log(erased === null, erased === undefined);
";

/// What Node prints for [`SYMBOLS_TS`].
const SYMBOLS_OUTPUT: &str = "symbol symbol symbol string number
true false true false
a reg true Symbol(a) Symbol(reg)
true symbol
symbol,string,number,symbol
false true Symbol(x) true
null undefined true true false false true object undefined
null undefined [ null, undefined, null ]
true false
";

/// `typeof`, identity (`Symbol.for` shared, `Symbol()` fresh), `description`
/// and `String`, and a ternary's `null` apart from its `undefined`.
#[test]
fn symbols_and_nullish_ternaries_follow_node() -> TestResult {
    ensure_prints_like_node(SYMBOLS_TS, SYMBOLS_OUTPUT, "symbols")
}

/// Three shapes CI's library gates surfaced, each from accepted source:
/// radash's `unique` (a `string | number | symbol` key, read through a
/// `K extends PropertyKey` callback or the item itself, into a property-key
/// record); Hono's `cors` (calling a union of callables whose arities differ,
/// which must pass every argument); and calling an erased value narrowed by
/// `typeof f === 'function'`, whose arity is unknown.
const CALL_SHAPES_TS: &str = r"// radash's `unique`: a `string | number | symbol` key into a property-key record.
function uniqueBy<T, K extends string | number | symbol>(array: readonly T[], toKey?: (item: T) => K): T[] {
  const valueMap = array.reduce((acc, item) => {
    const key = toKey ? toKey(item) : (item as any as string | number | symbol);
    if (acc[key]) return acc;
    acc[key] = item;
    return acc;
  }, {} as Record<string | number | symbol, T>);
  return Object.values(valueMap);
}
console.log(uniqueBy([3, 1, 1, 2, 3]).join(','), uniqueBy(['b', 'a', 'b']).join(','), uniqueBy([{ id: 1 }, { id: 1 }], (x) => x.id).length);

// A call through callables of different arity passes every argument (Hono's cors).
function chooser(mode: string | string[] | ((origin: string, tag: string) => string)) {
  return ((m) => {
    if (typeof m === 'string') {
      if (m === '*') {
        return () => m;
      }
      return (origin: string) => (m === origin ? origin : 'no');
    } else if (typeof m === 'function') {
      return m;
    }
    return (origin: string) => (m.includes(origin) ? origin : 'no');
  })(mode);
}
console.log(chooser('*')('x', 't'), chooser('a')('a', 't'), chooser(['b'])('b', 't'), chooser((o, t) => o + t)('x', 'y'));

// `typeof f === 'function'` on an erased value does not know its arity.
function callAny(f: unknown, a: string, b: string): unknown {
  if (typeof f === 'function') {
    return f(a, b);
  }
  return null;
}
console.log(callAny((x: string, y: string) => x + y, 'p', 'q'), callAny(1, 'p', 'q'));
";

/// What Node prints for [`CALL_SHAPES_TS`].
const CALL_SHAPES_OUTPUT: &str = "1,2,3 b,a 1\n* a b xy\npq null\n";

/// Union keys reach the record as property keys, and calls through mixed-arity
/// or erased callables pass every argument.
#[test]
fn key_unions_and_mixed_arity_calls_follow_node() -> TestResult {
    ensure_prints_like_node(CALL_SHAPES_TS, CALL_SHAPES_OUTPUT, "call shapes")
}

/// A project with a test file (a `test-globs` source) whose callback
/// parameters only the TypeScript checker types. A build that spells the
/// project through the symlink named the test file and its checker facts
/// through two different paths, so it typed fewer of them and wrote a
/// different crate.
const SYMLINK_MAIN_TS: &str = r"import { apply, collect } from './lib.ts';
const doubled = apply([1, 2, 3], (x) => x * 2);
const words = collect(['a', 'b']);
console.log(doubled.join(','), words.join(','));
";

/// The imported module of [`SYMLINK_MAIN_TS`].
const SYMLINK_LIB_TS: &str = r"export function apply<T, U>(xs: T[], f: (x: T) => U): U[] { return xs.map(f); }
export function collect(xs: string[]) {
  const out = [];
  for (const x of xs) { out.push(x + '!'); }
  return out;
}
export function maxBy<T>(items: T[], score: (item: T) => number): T | undefined {
  let best: T | undefined = undefined;
  let bestScore = -Infinity;
  for (const item of items) {
    const value = score(item);
    if (value > bestScore) { best = item; bestScore = value; }
  }
  return best;
}
";

/// The test file of the symlinked project.
const SYMLINK_SPEC_TS: &str = r"import { describe, expect, it } from 'vitest';
import { maxBy } from './lib.ts';

describe('maxBy', () => {
  it('picks the highest score', () => {
    const people = [{ name: 'a', age: 1 }, { name: 'b', age: 3 }];
    expect(maxBy(people, person => person.age)).toEqual({ name: 'b', age: 3 });
  });
});
";

/// Each file of a tree: its path relative to the root, and its bytes.
type TreeFile = (PathBuf, Vec<u8>);

/// Every file under `root`, relative to it, with its bytes.
fn tree(root: &Path) -> TestResult<Vec<TreeFile>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((path.strip_prefix(root)?.to_path_buf(), fs::read(&path)?));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// remeda's `setPath`, `clone` and `isProjection` shapes.
const OBJECT_IDENTITY_TS: &str = r#"// remeda's `setPath`: an object rest of a property-key record, then a spread of it.
function setPath(data: unknown, path: readonly PropertyKey[], value: unknown): unknown {
  const [pivot, ...rest] = path;
  if (pivot === undefined) {
    return value;
  }
  if (Array.isArray(data)) {
    const copy = [...data];
    copy[pivot as number] = setPath(data[pivot as number], rest, value);
    return copy;
  }
  const { [pivot]: currentValue, ...remaining } = data as Record<PropertyKey, unknown>;
  return { ...remaining, [pivot]: setPath(currentValue, rest, value) };
}
const source: unknown = { a: { b: { c: 1 }, e: [1, { f: 2 }] }, x: 3 };
console.log(JSON.stringify(setPath(source, ["a", "b", "c"], 2)), JSON.stringify(source));
console.log(JSON.stringify(setPath(source, ["a", "e", 1, "f"], 9)));
const tag = Symbol("tag");
const tagged = setPath({ q: 1 }, [tag], 5) as Record<PropertyKey, unknown>;
console.log(tagged[tag], Object.keys(tagged).join(","));

// remeda's `clone`: a record joins the reference list before it is filled.
function cloneObject(value: Record<string, unknown>, refTo: unknown[]): unknown {
  const copied: Record<PropertyKey, unknown> = {};
  refTo.push(copied);
  for (const [k, v] of Object.entries(value)) {
    copied[k] = v;
  }
  return copied;
}
const refs: unknown[] = [];
const cloned = cloneObject({ m: 1, n: 2 }, refs);
console.log(JSON.stringify(refs), refs[0] === cloned);

// remeda's `isProjection`: an erased callable keeps the arity it was declared with.
type Identity = <T>(first: T, ...rest: any) => T;
const identityImpl: Identity = (first) => first;
function identity(): Identity {
  return identityImpl;
}
const isProjection = (x: unknown): boolean => typeof x === "function" && x.length === 1;
const pair = (a: number, b: number): number => a + b;
const rules: unknown[] = [identity(), pair, (x: number) => x, "asc"];
console.log(rules.map(isProjection).join(","));
"#;

/// What Node prints for [`OBJECT_IDENTITY_TS`].
const OBJECT_IDENTITY_OUTPUT: &str = concat!(
    "{\"x\":3,\"a\":{\"e\":[1,{\"f\":2}],\"b\":{\"c\":2}}} {\"a\":{\"b\":{\"c\":1},\"e\":[1,{\"f\":2}]},\"x\":3}\n",
    "{\"x\":3,\"a\":{\"b\":{\"c\":1},\"e\":[1,{\"f\":9}]}}\n",
    "5 q\n",
    "[{\"m\":1,\"n\":2}] true\n",
    "true,false,true,false\n",
);

/// A property-key record is an object and an erased callable is a function:
///
/// - an object rest (`const { [k]: v, ...rest } = record`) is a new object,
///   so the source keeps `k`, and spreading it into an object literal copies
///   every own key (it used to be dropped when the key types differed);
/// - an `unknown`-valued record and its erased form are ONE object, so a
///   write after it was pushed into an `unknown[]` is seen there;
/// - a callable erased at a `(first, ...rest)` type reports `length` 1, and
///   one pulled back out of an `unknown` keeps the arity it was erased with.
#[test]
fn records_stay_objects_and_callables_keep_their_arity() -> TestResult {
    ensure_prints_like_node(OBJECT_IDENTITY_TS, OBJECT_IDENTITY_OUTPUT, "object identity")
}

/// es-toolkit's `intersectionBy`, `intersectionWith` and `result` shapes: an
/// erased value proven a function by `typeof`.
const ERASED_FUNCTIONS_TS: &str = r#"// es-toolkit's `intersectionBy`: `last(values)` is `unknown | undefined`, and a
// callback calls it after a `typeof` guard.
function lastOf<T>(arr: readonly T[]): T | undefined {
  return arr[arr.length - 1];
}
function mapAll(xs: number[], f: (x: number) => unknown): unknown[] {
  return xs.map(f);
}
function run(...values: any[]): unknown[] {
  const lastValue = lastOf(values);
  if (typeof lastValue === "function") {
    const direct = lastValue(5, 1);
    return mapAll([1, 2], (value) => lastValue(value, 1)).concat([direct]);
  }
  return [];
}
console.log(JSON.stringify(run((x: number, y: number) => x * 10 + y)));

// es-toolkit's `intersectionWith`: the guarded value becomes a typed comparator.
function pick(...others: any[]): (x: number, y: number) => boolean {
  const last = lastOf(others);
  let comparator = (x: number, y: number) => x === y;
  if (typeof last === "function") {
    comparator = last;
  }
  return comparator;
}
console.log(pick(1, (a: number, b: number) => a < b)(1, 2), pick(1)(1, 2));

// es-toolkit's `result`: `value.call(object)` runs `value` with `object` as `this`.
function viaCall(obj: unknown, value: unknown): unknown {
  return typeof value === "function" ? value.call(obj) : value;
}
console.log(viaCall({ k: 4 }, function (this: { k: number }) { return this.k; }), viaCall(1, 7));
const viaApply: unknown = function (this: { k: number }, a: number, b: number) { return this.k * 100 + a * 10 + b; };
console.log((viaApply as any).apply({ k: 1 }, [2, 3]));

// `length` of an erased function, and a throw from a call inside `try`.
function len(value: unknown): number {
  return typeof value === "function" ? value.length : -1;
}
console.log(len((a: number, b: number) => a + b), len(3));
function guarded(value: unknown): string {
  try {
    return String(typeof value === "function" ? value.call(null) : value);
  } catch (error) {
    return "caught " + (error as Error).message;
  }
}
console.log(guarded(() => { throw new Error("boom"); }), guarded(2));

// remeda's `isCase`: `length` of an element destructured out of an erased array.
function isCase(maybeCase: unknown): boolean {
  if (!Array.isArray(maybeCase)) {
    return false;
  }
  const [when, then, ...rest] = maybeCase as readonly unknown[];
  return typeof when === "function" && when.length <= 1 && typeof then === "function" && then.length <= 1 && rest.length === 0;
}
console.log(isCase([(x: number) => x, () => 1]), isCase([(x: number, y: number) => x + y, () => 1]), isCase(3));
"#;

/// What Node prints for [`ERASED_FUNCTIONS_TS`].
const ERASED_FUNCTIONS_OUTPUT: &str = "[11,21,51]\ntrue false\n4 7\n123\n2 -1\ncaught boom 2\ntrue false false\n";

/// A value `typeof` proves a function, still erased:
///
/// - called through `unknown | undefined` (directly, and captured by a
///   callback) it is called with every argument, where it used to answer
///   `null` without being called;
/// - assigned to a typed function local it is that function;
/// - `f.call(t)` and `f.apply(t, args)` run it with `t` as its `this` (the
///   leading operand used to become its first argument), and its throw
///   reaches an enclosing `try`;
/// - `f.length` is the arity it was declared with, also for an element
///   destructured out of an erased array (an `unknown | undefined` local).
#[test]
fn erased_functions_are_called_like_javascript() -> TestResult {
    ensure_prints_like_node(ERASED_FUNCTIONS_TS, ERASED_FUNCTIONS_OUTPUT, "erased functions")
}

/// Building through `--manifest-path <symlink>/Smelt.toml` emits the crate the
/// real directory does (sources, erasure and typedness sidecars), and that
/// crate runs. The only difference allowed is the project directory as the
/// command spelled it, which reported paths (a test file's `// source:`, an
/// assertion message) keep: identity is canonical, output is not.
#[cfg(unix)]
#[test]
fn a_symlinked_project_builds_like_its_real_directory() -> TestResult {
    let temp = TempProject::new()?;
    let real = temp.path().join("real_project");
    fs::create_dir_all(real.join("src"))?;
    write_project(&real, SYMLINK_MAIN_TS)?;
    fs::write(real.join("src").join("lib.ts"), SYMLINK_LIB_TS)?;
    fs::write(real.join("src").join("lib.spec.ts"), SYMLINK_SPEC_TS)?;
    let manifest = fs::read_to_string(real.join("Smelt.toml"))?
        .replace("roots = [\"src\"]", "roots = [\"src\"]\ntest-globs = [\"*.spec.ts\"]");
    fs::write(real.join("Smelt.toml"), manifest)?;
    let link = temp.path().join("linked_project");
    std::os::unix::fs::symlink(&real, &link)?;
    let outside = temp.path();

    smelt_in(outside, &["--manifest-path", "real_project/Smelt.toml", "build"])?;
    let through_real = tree(&real.join("dist"))?;
    fs::remove_dir_all(real.join("dist"))?;
    smelt_in(outside, &["--manifest-path", "linked_project/Smelt.toml", "build"])?;
    let through_link = tree(&real.join("dist"))?;

    ensure(!through_real.is_empty(), "the real-directory build wrote no files")?;
    for ((real_path, real_bytes), (link_path, link_bytes)) in through_real.iter().zip(&through_link) {
        ensure_eq(link_path, real_path, "the symlinked build's file list")?;
        let link_text = String::from_utf8_lossy(link_bytes).replace("linked_project/", "real_project/");
        ensure(
            *real_bytes == link_text.into_bytes(),
            format!("{} differs between the real and the symlinked build", real_path.display()),
        )?;
    }
    ensure_eq(&through_link.len(), &through_real.len(), "the number of files each build wrote")?;
    let output = cargo_run_manifest(&real.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &"2,4,6 a!,b!\n".to_owned(), "the symlinked build's output")
}
