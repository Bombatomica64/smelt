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
//! - **Symlinked paths.** Building through a symlink to the project gives the
//!   same crate, byte for byte, as building through the real directory.

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
const PROPERTY_KEYS_TS: &str = r#"function put<K extends PropertyKey>(r: Record<K, string>, k: K, v: string): void { r[k] = v; }
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
"#;

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
const SYMBOLS_TS: &str = r#"const a = Symbol('a');
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
"#;

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

/// A project with a test file (a `test-globs` source) whose callback
/// parameters only the TypeScript checker types. A build that spells the
/// project through the symlink named the test file and its checker facts
/// through two different paths, so it typed fewer of them and wrote a
/// different crate.
const SYMLINK_MAIN_TS: &str = r#"import { apply, collect } from './lib.ts';
const doubled = apply([1, 2, 3], (x) => x * 2);
const words = collect(['a', 'b']);
console.log(doubled.join(','), words.join(','));
"#;

/// The imported module of [`SYMLINK_MAIN_TS`].
const SYMLINK_LIB_TS: &str = r#"export function apply<T, U>(xs: T[], f: (x: T) => U): U[] { return xs.map(f); }
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
"#;

/// The test file of the symlinked project.
const SYMLINK_SPEC_TS: &str = r#"import { describe, expect, it } from 'vitest';
import { maxBy } from './lib.ts';

describe('maxBy', () => {
  it('picks the highest score', () => {
    const people = [{ name: 'a', age: 1 }, { name: 'b', age: 3 }];
    expect(maxBy(people, person => person.age)).toEqual({ name: 'b', age: 3 });
  });
});
"#;

/// Every file under `root`, relative to it, with its bytes.
fn tree(root: &Path) -> TestResult<Vec<(PathBuf, Vec<u8>)>> {
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

/// Building through `--manifest-path <symlink>/Smelt.toml` emits the crate the
/// real directory does, byte for byte (sources, erasure and typedness
/// sidecars), and that crate runs.
#[cfg(unix)]
#[test]
fn a_symlinked_project_builds_like_its_real_directory() -> TestResult {
    let temp = TempProject::new()?;
    let real = temp.path().join("real");
    fs::create_dir_all(real.join("src"))?;
    write_project(&real, SYMLINK_MAIN_TS)?;
    fs::write(real.join("src").join("lib.ts"), SYMLINK_LIB_TS)?;
    fs::write(real.join("src").join("lib.spec.ts"), SYMLINK_SPEC_TS)?;
    let manifest = fs::read_to_string(real.join("Smelt.toml"))?
        .replace("roots = [\"src\"]", "roots = [\"src\"]\ntest-globs = [\"*.spec.ts\"]");
    fs::write(real.join("Smelt.toml"), manifest)?;
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link)?;
    let outside = temp.path();

    smelt_in(outside, &["--manifest-path", "real/Smelt.toml", "build"])?;
    let through_real = tree(&real.join("dist"))?;
    fs::remove_dir_all(real.join("dist"))?;
    smelt_in(outside, &["--manifest-path", "link/Smelt.toml", "build"])?;
    let through_link = tree(&real.join("dist"))?;

    ensure(!through_real.is_empty(), "the real-directory build wrote no files")?;
    for ((real_path, real_bytes), (link_path, link_bytes)) in through_real.iter().zip(&through_link) {
        ensure_eq(link_path, real_path, "the symlinked build's file list")?;
        ensure(
            real_bytes == link_bytes,
            format!("{} differs between the real and the symlinked build", real_path.display()),
        )?;
    }
    ensure_eq(&through_link.len(), &through_real.len(), "the number of files each build wrote")?;
    let output = cargo_run_manifest(&real.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &"2,4,6 a!,b!\n".to_owned(), "the symlinked build's output")
}
