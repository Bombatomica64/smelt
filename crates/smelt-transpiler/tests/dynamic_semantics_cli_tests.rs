//! Source semantics that depend on run-time values or on a value's real
//! shape, end to end against Node and `CPython`.
//!
//! Each program is accepted source that used to miscompile or be refused, and
//! must now print what the source language prints:
//!
//! - **Dynamic `+`**: JavaScript `+` with an `any` operand concatenates or
//!   adds depending on the RUN-TIME values (`ApplyStringOrNumericBinaryOperator`).
//!   `(u as any) + 5` is `"t5"` for `u = "t"`; it used to be `NaN`.
//! - **Optional builtin calls on any receiver**: `s.match(re)?.join(",")`
//!   evaluates the receiver once and calls the array method only when the
//!   match is present.
//! - **A record member named like a `Map` method** is that member:
//!   `o.has(1)` on `{ has: (a: number) => string }` calls the property.
//! - **Python sets** answer `len`, `in`, `min`/`max`/`sorted`/`sum`, set
//!   comprehensions, the set operators (`|`, `&`, `-`, `^`, `<=`, `<`, ...)
//!   and the in-place operators and `update` methods, which mutate the set
//!   every alias sees.

mod common;

use std::{fs, path::Path};

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, ensure_python_agrees, smelt_in,
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
            "[project]\nname = \"dynamic\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"dynamic\"\nkind = \"program\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// Builds the project at `root` and returns what the generated program prints.
fn build_and_run(root: &Path) -> TestResult<String> {
    smelt_in(root, &["build"])?;
    cargo_run_manifest(&root.join("dist/Cargo.toml"))
}

/// JavaScript `+` over `any` operands, every `ToPrimitive` outcome.
///
/// `f` is ONE function whose result is a string for one argument and a number
/// for another (`typeof f("t")` vs `typeof f(2)`): no concrete Rust type,
/// union of its operand types or generic can represent it, which is the
/// dynamic boundary the erased result stands for. `g` keeps the typed
/// concatenation (a definite string side), `h` returns at its declared
/// `number`, and `n += 2` on a `number` stays the typed addition.
const DYNAMIC_ADD_TS: &str = r#"function f(u: unknown): unknown { return (u as any) + 5; }
function g(u: any, s: string) { return u + s; }
function h(u: any): number { return u + 1; }
function k(a: any, b: any) { return a + b; }
function big(u: any) { return u + 10n; }
const arr: any = [1, 2];
const obj: any = { a: 1 };
console.log(f("t"), f(2), f(true), f(null), f(undefined), g(3, "x"), String(h(4)));
console.log(k(1, 2), k("1", 2), k(1, "2"), k(arr, 1), k(obj, "!"), k(null, null), k(true, true));
console.log(big(1n), big("a"), typeof f("t"), typeof f(2));
try { big(1); } catch (e) { console.log((e as Error).name, (e as Error).message); }
let acc: any = 0;
for (const v of [1, "b", 2]) { acc = acc + v; }
let acc2: any = 1;
acc2 += "z";
acc2 += 2;
let n: number = 1;
n += 2;
console.log(acc, acc2, n);
"#;

/// What Node prints for [`DYNAMIC_ADD_TS`].
const DYNAMIC_ADD_OUTPUT: &str = "t5 7 6 5 NaN 3x 5\n3 12 12 1,21 [object Object]! 0 2\n11n a10 string number\n\
TypeError Cannot mix BigInt and other types, use explicit conversions\n1b2 1z2 3\n";

/// `any + x` follows `ApplyStringOrNumericBinaryOperator` at run time, and
/// only a genuinely dynamic `+` is erased.
#[test]
fn dynamic_addition_follows_javascript() -> TestResult {
    let sources = [("main.ts", DYNAMIC_ADD_TS)];
    ensure_node_agrees(&sources, "main.ts", DYNAMIC_ADD_OUTPUT, "dynamic addition")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &DYNAMIC_ADD_OUTPUT.to_owned(), "the dynamic-addition program's output")?;
    // Type preservation: the erased carrier only where `+` is decided at run
    // time; a definite string side and a declared `number` keep their types.
    let generated = fs::read_to_string(root.join("dist/src/source_main.rs"))?;
    for signature in [
        "fn f(u: SmeltUnknown) -> SmeltUnknown",
        "fn g(u: SmeltUnknown, s: JsString) -> JsString",
        "fn h(u: SmeltUnknown) -> f64",
        "fn k(a: SmeltUnknown, b: SmeltUnknown) -> SmeltUnknown",
    ] {
        ensure(generated.contains(signature), format!("generated Rust keeps `{signature}`"))?;
    }
    Ok(())
}

/// Optional calls of builtin methods on a receiver that is not a bare local:
/// a regex match (`RegExpMatchArray | null`), a call returning
/// `string[] | undefined` or `string | undefined`. The receiver runs once (a
/// global regex keeps its `lastIndex` right) and the call only when present.
const OPTIONAL_RECEIVER_TS: &str = r#"const re = /a(b)/;
console.log("xab".match(re)?.join(","), "xyz".match(new RegExp("q"))?.join(","));
const g = new RegExp("b", "g");
console.log("abab".match(g)?.join("-") ?? "none", "zz".match(g)?.length ?? "none");
let calls = 0;
function getArr(f: boolean): string[] | undefined { calls++; return f ? ["a", "b"] : undefined; }
function getStr(f: boolean): string | undefined { calls++; return f ? "abc" : undefined; }
console.log(getArr(true)?.join(","), getArr(false)?.join(","), getStr(true)?.toUpperCase(), getStr(false)?.at(0), calls);
const m = "xab".match(/a(b)/);
console.log(m?.join(","), m?.length, m?.[1], "q".match(/z/)?.[0]);
"#;

/// What Node prints for [`OPTIONAL_RECEIVER_TS`].
const OPTIONAL_RECEIVER_OUTPUT: &str = "ab,b undefined\nb-b none\na,b undefined ABC undefined 4\nab,b 2 b undefined\n";

/// `receiver?.method()` lowers for any receiver whose present type has builtin methods.
#[test]
fn optional_builtin_calls_on_any_receiver() -> TestResult {
    let sources = [("main.ts", OPTIONAL_RECEIVER_TS)];
    ensure_node_agrees(&sources, "main.ts", OPTIONAL_RECEIVER_OUTPUT, "optional receiver")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &OPTIONAL_RECEIVER_OUTPUT.to_owned(), "the optional-receiver program's output")
}

/// A record whose members are named like `Map` methods (`has`, `get`,
/// `keys`), as one uniform-member object type and as a `Record` of
/// functions: each call is the member's own function.
const RECORD_MEMBERS_TS: &str = r#"function x(o: { has: (a: number) => string }) { return o.has(1); }
function y(r: Record<string, (k: number) => number>) { return r.get(2) + r.keys(3); }
const m = new Map<string, number>([["a", 1]]);
console.log(x({ has: (a) => "H" + a }), y({ get: (k) => k * 10, keys: (k) => k }), m.has("a"), m.get("a"));
"#;

/// What Node prints for [`RECORD_MEMBERS_TS`].
const RECORD_MEMBERS_OUTPUT: &str = "H1 23 true 1\n";

/// A callable record member named like a `Map` method calls the member.
#[test]
fn record_members_named_like_map_methods_are_members() -> TestResult {
    let sources = [("main.ts", RECORD_MEMBERS_TS)];
    ensure_node_agrees(&sources, "main.ts", RECORD_MEMBERS_OUTPUT, "record members")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &RECORD_MEMBERS_OUTPUT.to_owned(), "the record-members program's output")
}

/// Every set builtin and operator `CPython` gives a `set`, including the
/// in-place forms, whose mutation every alias of the set observes.
const PYTHON_SETS: &str = r#"s = {3, 1, 2}
t = {2, 5}
print(len(s), 1 in s, 9 in s, 9 not in s, min(s), max(s), sorted(s), sum(s))
print(sorted(s | t), sorted(s & t), sorted(s - t), sorted(s ^ t))
print({1, 2} <= s, s >= {1}, {1} < s, s < s, s > {1, 2}, s <= s, t <= s)
sq = {x * x for x in s}
print(sorted(sq), len(sq), sorted({x for x in s if x > 1}))
alias = s
u = s | {7}
print(len(alias), len(u), len(s))
words = {"pear", "fig", "apple"}
empty: set[int] = set()
print(min(words), max(words), sorted(words, reverse=True), len(empty))
print(any({False, True}), all({False, True}), min([4.5, 2.5, 3.0]), max([1, 9, 9]))
fs = {1.5, 2.5}
print(sum(fs), s == {1, 2, 3}, s != t)
try:
    min(empty)
except ValueError as e:
    print("ValueError", e)
v = {1, 2}
w = v
v |= {3}
v -= {1}
print(sorted(v), sorted(w), v is w)
v &= {2, 3, 4}
v ^= {9}
print(sorted(v), sorted(w))
g = {1}
def grow() -> None:
    global g
    g |= {5}
grow()
d = {"k": {1, 2}}
d["k"] -= {1}
inner = d["k"]
inner.update({8})
inner.difference_update({2})
p = {1, 2, 3}
q = p
p.intersection_update({2, 3})
p.symmetric_difference_update({3, 4})
p |= p
print(sorted(g), sorted(d["k"]), sorted(q))
"#;

/// What `CPython` prints for [`PYTHON_SETS`].
const PYTHON_SETS_OUTPUT: &str = "3 True False True 1 3 [1, 2, 3] 6\n[1, 2, 3, 5] [2] [1, 3] [1, 3, 5]\n\
True True True False True True False\n[1, 4, 9] 3 [2, 3]\n3 4 3\napple pear ['pear', 'fig', 'apple'] 0\n\
True False 2.5 9\n4.0 True True\nValueError min() iterable argument is empty\n[2, 3] [2, 3] True\n\
[2, 3, 9] [2, 3, 9]\n[1, 5] [8] [2, 4]\n";

/// Python set builtins and operators print what `CPython` prints.
#[test]
fn python_sets_match_cpython() -> TestResult {
    ensure_python_agrees(PYTHON_SETS, PYTHON_SETS_OUTPUT, "python sets")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.py", &[("main.py", PYTHON_SETS)])?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &PYTHON_SETS_OUTPUT.to_owned(), "the python-sets program's output")
}
