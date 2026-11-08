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
