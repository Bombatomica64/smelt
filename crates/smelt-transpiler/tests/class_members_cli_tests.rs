//! Class members whose value changes at run time, end to end against Node.
//!
//! - **Live static fields**: a mutable `static` field is storage, not a
//!   constant. `Box.PI = 11`, `Box.PI += 1`, `Box.count++` (in a constructor,
//!   a static method or a closure, through `this` in a static method, and
//!   across modules) and a write through the class used as a value
//!   (`(K: typeof Box) => { K.PI = n }`) all reach the one value every read
//!   sees, through the class or through any value of it. A `static readonly`
//!   field stays a folded constant.
//! - **Self-reading constructor-function fields**: `this.count = this.count +
//!   1` in a prototype method keeps the field the `number` its other writes
//!   give it (a fixed point over the field's own type), instead of erasing it.

mod common;

use std::{fs, path::Path};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in};

/// Writes a one-entry project at `root` around the files of `sources`.
fn write_project(root: &Path, entry: &str, sources: &[(&str, &str)]) -> TestResult {
    fs::create_dir_all(root.join("src"))?;
    for (path, text) in sources {
        fs::write(root.join("src").join(path), text)?;
    }
    fs::write(
        root.join("Smelt.toml"),
        format!(
            "[project]\nname = \"members\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\nroots = [\"src\"]\n\n\
             [output]\ntarget = \"dist\"\ncrate-name = \"members\"\nkind = \"program\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// Builds the project at `root` and returns what the generated program prints.
fn build_and_run(root: &Path) -> TestResult<String> {
    smelt_in(root, &["build"])?;
    cargo_run_manifest(&root.join("dist/Cargo.toml"))
}

/// Static fields read and written directly, through class values (a
/// parameter and a kept `const`), from methods, and through a subclass, plus
/// a parameter named like the class that is the parameter.
const STATICS_TS: &str = r#"class Box {
  static PI = 7;
  static label: string = "box";
  static readonly MAX = 3;
  static count = 0;
  v: number;
  constructor(v: number) { this.v = v; Box.count++; }
  static bump(): number { Box.count += 10; return Box.count; }
}
class Sub extends Box { static extra = 1; }
function read(K: typeof Box) { return K.PI; }
function write(K: typeof Box, n: number) { K.PI = n; }
function relabel(K: typeof Box, s: string) { K.label = s; }
function shadow(Box: { PI: number }) { return Box.PI; }
const kept: typeof Box = Box;
console.log(read(Box), Box.PI, Box.MAX, read(Sub));
write(Box, 9);
console.log(read(Box), Box.PI, kept.PI, Sub.PI);
Box.PI = 11;
Box.PI += 1;
console.log(read(Box), kept.PI, Box.PI, read(Sub));
relabel(kept, "crate");
new Box(1); new Box(2);
console.log(Box.label, kept.label, Box.count, Box.bump(), Sub.extra, shadow({ PI: 99 }));
write(kept, 20);
console.log(Box.PI, read(Box), kept.PI);
"#;

/// What Node prints for [`STATICS_TS`].
const STATICS_OUTPUT: &str = "7 7 3 7\n9 9 9 9\n12 12 12 12\ncrate crate 2 12 1 99\n20 20 20\n";

/// Mutable static fields are live storage shared by the class and its values.
#[test]
fn static_fields_are_live_through_class_values() -> TestResult {
    let sources = [("main.ts", STATICS_TS)];
    ensure_node_agrees(&sources, "main.ts", STATICS_OUTPUT, "live statics")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &STATICS_OUTPUT.to_owned(), "the live-statics program's output")?;
    // Type preservation: the storage keeps the field's own type.
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(generated.contains("::std::cell::Cell<f64> = const { ::std::cell::Cell::new(7.0) }"), "`Box.PI` is an `f64` cell")
}

/// The class a module exports, mutated by its importer: an expression
/// assignment, `++` in a closure, a static method through the class name and
/// through `this`, and a write through a class value.
const COUNTER_TS: &str = r#"export class Counter {
  static total = 0;
  static name2 = "c";
  static add(n: number): number { Counter.total += n; return Counter.total; }
  static selfAdd(n: number): number { this.total += n; return this.total; }
}
"#;

/// The importer of [`COUNTER_TS`].
const COUNTER_MAIN_TS: &str = r#"import { Counter } from "./lib.ts";
const y = (Counter.total = 5);
const inc = () => { Counter.total++; };
inc(); inc();
console.log(y, Counter.total, Counter.add(3), Counter.selfAdd(2));
function viaValue(C: typeof Counter) { C.total = C.total * 2; return C.total; }
console.log(viaValue(Counter), Counter.total, Counter.name2);
"#;

/// What Node prints for [`COUNTER_MAIN_TS`].
const COUNTER_OUTPUT: &str = "5 7 10 12\n24 24 c\n";

/// A static field mutated across modules is one value.
#[test]
fn static_fields_are_shared_across_modules() -> TestResult {
    let sources = [("main.ts", COUNTER_MAIN_TS), ("lib.ts", COUNTER_TS)];
    ensure_node_agrees(&sources, "main.ts", COUNTER_OUTPUT, "cross-module statics")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &COUNTER_OUTPUT.to_owned(), "the cross-module statics program's output")
}

/// A constructor function whose prototype method reads and writes its own
/// field through an untyped `this`.
const SELF_READING_FIELD_TS: &str = r#"function Point(this: any, x: number) {
  this.count = 1;
  if (x > 0) { this.mixed = 2; } else { this.mixed = "neg"; }
  this.fromParam = x;
}
Point.prototype.bump = function (this: any) { this.count = this.count + 1; return this.count; };
// @ts-ignore
const p = new Point(1);
// @ts-ignore
const q = new Point(-1);
console.log(p.count, p.bump(), p.bump(), p.count + 0.5, q.mixed, p.mixed, p.fromParam);
"#;

/// What Node prints for [`SELF_READING_FIELD_TS`].
const SELF_READING_FIELD_OUTPUT: &str = "1 2 3 3.5 neg 2 1\n";

/// A self-reading constructor-function field keeps its `number` type.
#[test]
fn self_reading_constructor_function_field_stays_typed() -> TestResult {
    let sources = [("main.ts", SELF_READING_FIELD_TS)];
    ensure_node_agrees(&sources, "main.ts", SELF_READING_FIELD_OUTPUT, "self-reading field")?;
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, "main.ts", &sources)?;
    let output = build_and_run(root)?;
    ensure_eq(&output, &SELF_READING_FIELD_OUTPUT.to_owned(), "the self-reading-field program's output")?;
    let generated = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(generated.contains("    count: f64,"), "the fixed point keeps `count: f64`")
}
