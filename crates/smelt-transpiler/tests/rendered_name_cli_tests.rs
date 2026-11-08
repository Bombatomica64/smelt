//! Source names that render alike, or like a name the generated crate already
//! uses, still build and behave as the source does.
//!
//! Rust identifiers are minted from source names by one allocator
//! (`smelt_codegen_rust`'s `names` module): uniqueness is decided on the final
//! rendered string, per Rust namespace, against a reserved set that holds the
//! emitter's prefixes and the trait methods it calls on user types. Each
//! program here collides on purpose; it must build, print what Node or
//! `CPython` prints, and spell the colliding names apart.

mod common;

use std::{fs, path::Path};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, smelt_in};

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

/// Every generated Rust file of the crate at `root/dist`, concatenated.
fn generated_rust(root: &Path) -> TestResult<String> {
    let mut paths = fs::read_dir(root.join("dist/src"))?
        .map(|entry| Ok(entry?.path()))
        .collect::<TestResult<Vec<_>>>()?;
    paths.sort();
    let mut text = String::new();
    for path in paths {
        text.push_str(&fs::read_to_string(path)?);
    }
    Ok(text)
}

/// Every TypeScript collision of the audit in one program, with the output
/// Node prints for it (checked with `node --experimental-strip-types`).
const COLLIDING_TS: &str = r#"class Counter {
  n: number
  constructor(n: number) { this.n = n }
  clone(): Counter { return new Counter(this.n + 100) }
}
const c = new Counter(1)
const d = c.clone()
const pair = [c, d]
console.log(pair[0].n, pair[1].n, d.n)
function fooBar(): number { return 1 }
function foo_bar(): number { return 2 }
console.log(fooBar(), foo_bar())
class A {
  static make(): number { return 3 }
  make(): number { return 4 }
}
console.log(A.make(), new A().make())
class Maker {
  v: number
  constructor(v: number) { this.v = v }
  static new(): Maker { return new Maker(42) }
}
console.log(Maker.new().v, new Maker(5).v)
class Box { v: number = 6 }
class Box_ { w: string = "w" }
console.log(new Box().v, new Box_().w)
interface Shape { kind: string }
class Shape { area: number = 7 }
const s = new Shape()
s.kind = "sq"
console.log(s.area, s.kind)
const $ = (x: string): string => x + "!"
function _(): number { return 8 }
console.log($("x"), _())
class SmeltList { size: number = 9 }
function smelt_typeof(x: number): string { return "mine" + x }
const xs: number[] = [1, 2]
xs.push(3)
console.log(new SmeltList().size, smelt_typeof(1), xs.length)
function call(fooBar: number): number { return fooBar + foo_bar() }
const smelt_needle = 2
const smelt_list_items: number[] = [5]
console.log(call(10), [smelt_needle, 5].includes(smelt_needle), smelt_list_items[0])
interface Pair { fooBar: number; foo_bar: number }
const p: Pair = { fooBar: 1, foo_bar: 2 }
class G<T> {
  _smelt_phantom: number = 3
  smelt_x: number = 4
  v: T
  constructor(v: T) { this.v = v }
}
const g = new G<string>("s")
console.log(p.fooBar, p.foo_bar, g._smelt_phantom, g.smelt_x, g.v, JSON.stringify(p))
"#;

/// What Node prints for [`COLLIDING_TS`], one line per collision:
///
/// 1. a user `clone()` that builds a new instance must not replace the
///    emitter's handle copy (`pair[0]` is `c` itself, not a fresh `Counter`);
/// 2. `fooBar`/`foo_bar` are two functions;
/// 3. a static and an instance `make` share one `impl`;
/// 4. a static `new` shares the `impl` with the constructor `new`;
/// 5. `Box` escapes to `Box_`, which a source `Box_` already spells;
/// 6. `interface Shape` merges into `class Shape` (one struct, both members);
/// 7. `$` and `_` both sanitize to the wildcard;
/// 8. a user `SmeltList`/`smelt_typeof` beside the runtime's own;
/// 9. a parameter `fooBar` beside the function `foo_bar` it calls, and user
///    locals spelled like the emitter's `smelt_*` template temporaries;
/// 10. two fields of one struct that render alike (`fooBar`/`foo_bar`), and
///     source fields spelled like the emitter's own (`_smelt_phantom`, which a
///     generic struct already declares) -- with JSON keeping the source keys.
const COLLIDING_TS_OUTPUT: &str = "1 101 101\n1 2\n3 4\n42 5\n6 w\n7 sq\nx! 8\n9 mine1 3\n12 true 5\n\
1 2 3 4 s {\"fooBar\":1,\"foo_bar\":2}\n";

/// The TypeScript collisions build, run as Node runs them, and render apart.
#[test]
fn colliding_typescript_names_build_and_behave() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &[("main.ts", COLLIDING_TS)])?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &COLLIDING_TS_OUTPUT.to_owned(), "the colliding program's output")?;
    let rust = generated_rust(root)?;
    for spelled in [
        // A user method never takes a trait method's name.
        "fn clone_(&self)",
        // The runtime's prefixes never come out of a source name.
        "struct SrcSmeltList",
        "fn src_smelt_typeof(",
        "let src_smelt_needle",
        // Two renderings of one string are two names.
        "struct Box_ ",
        "struct Box__1 ",
        // One struct's fields are renamed apart in that struct's scope.
        "foo_bar_1: f64",
        "src_smelt_phantom: f64",
        // A name the compiler minted keeps its reserved spelling.
        "fn smelt_global_init__c__",
    ] {
        ensure(rust.contains(spelled), format!("generated crate must spell `{spelled}`"))?;
    }
    ensure(!rust.contains("fn smelt_typeof("), "a user function took the runtime's `smelt_typeof`")?;
    ensure(!rust.contains("src_smelt_global_init"), "a compiler-minted name was moved out of its namespace")?;
    Ok(())
}

/// The same collisions reach the Python frontend and are resolved the same way.
#[test]
fn colliding_python_names_build_and_behave() -> TestResult {
    let source = r#"class Counter:
    def __init__(self, n: int) -> None:
        self.n = n

    def clone(self) -> "Counter":
        return Counter(self.n + 100)


def fooBar() -> int:
    return 1


def foo_bar() -> int:
    return 2


def smelt_typeof(x: int) -> str:
    return "mine" + str(x)


def call(fooBar: int) -> int:
    return fooBar + foo_bar()


c = Counter(1)
d = c.clone()
pair = [c, d]
print(pair[0].n, pair[1].n, d.n)
print(fooBar(), foo_bar())
print(smelt_typeof(1), call(10))
"#;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.py", &[("main.py", source)])?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    // What CPython prints.
    ensure_eq(&output, &"1 101 101\n1 2\nmine1 12\n".to_owned(), "the colliding Python program's output")?;
    let rust = generated_rust(root)?;
    ensure(rust.contains("fn clone_(&self)"), "a Python `clone` method must not shadow `Clone::clone`")?;
    ensure(!rust.contains("fn smelt_typeof("), "a Python function took the runtime's `smelt_typeof`")
}

/// Module files are unique after sanitizing and case-insensitively, and a
/// rebuild removes the generated module files it no longer writes while
/// keeping files it did not generate.
#[test]
fn module_files_are_unique_and_stale_ones_are_removed() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(
        root,
        "main.ts",
        &[
            (
                "main.ts",
                "import { a } from './foo-bar'\nimport { b } from './foo_bar'\nimport { c } from './Foo'\nimport { d } from './foo'\nconsole.log(a(), b(), c(), d())\n",
            ),
            ("foo-bar.ts", "export function a(): number { return 1 }\n"),
            ("foo_bar.ts", "export function b(): number { return 2 }\n"),
            ("Foo.ts", "export function c(): number { return 3 }\n"),
            // `Foo.ts` and `foo.ts` side by side are rejected by tsc (TS1149,
            // names differing only in casing); a Python `foo` beside the
            // TypeScript `Foo` is valid and collides the same way here.
            ("foo.py", "def d() -> int:\n    return 4\n"),
        ],
    )?;
    smelt_in(root, &["build"])?;
    let src = root.join("dist/src");
    let mut stems = fs::read_dir(&src)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().to_lowercase()))
        .collect::<TestResult<Vec<_>>>()?;
    stems.sort();
    let mut unique = stems.clone();
    unique.dedup();
    ensure_eq(&stems.len(), &5, "the root and four module files")?;
    ensure_eq(&unique, &stems, "module files compared case-insensitively")?;
    // A generated module of an earlier build, and a hand-written file.
    fs::write(src.join("stale.rs"), "// @generated by smelt. Do not edit by hand.\nfn gone() {}\n")?;
    fs::write(src.join("keep.rs"), "// hand-written\n")?;
    smelt_in(root, &["build"])?;
    ensure(!src.join("stale.rs").exists(), "a stale generated module must be removed")?;
    ensure(src.join("keep.rs").exists(), "a file without the generated header must be kept")?;
    fs::remove_file(src.join("keep.rs"))?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&output, &"1 2 3 4\n".to_owned(), "each module's function")
}
