//! The two conversions of a record that still copied, end to end against Node.
//!
//! 1. **A wider property type.** `{ n: number }` handed to a slot typed
//!    `{ n: number | string }`, `{ n: unknown }` or `{ n?: number }` was
//!    converted field by field, so a write through the slot never reached the
//!    caller's object. A structural view (`smelt_codegen_rust::structural_views`)
//!    now reaches such a property through converting guards
//!    (`SmeltFieldRead`/`SmeltFieldWrite`): a read answers the stored value at
//!    the slot's type, a write of a value the stored type can hold goes into
//!    the field in place.
//! 2. **A second record type.** A literal (or a literal kept as a dictionary)
//!    converted to record type `A` and then to an unrelated record type `B`
//!    was adopted by the `A` record and copied for `B`. It is now built as (or
//!    adopted by) its own record, which `A` and `B` both view.
//!
//! Next to them: `typeof l.n === "number"` now narrows the property read
//! `l.n` (the checker's flow type at the read), so `l.n + 1` adds.
//!
//! Each test checks the program against Node, builds and runs the generated
//! crate, and asserts the Rust types it emits: every record keeps its own
//! struct, the slot types are views (`Rc<dyn ..SmeltView>`), and no
//! field-wise copy of the converted record is built.

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
        "[project]\nname = \"views\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\nroots = [\"src\"]\n\n\
         [output]\ntarget = \"dist\"\ncrate-name = \"views\"\nkind = \"program\"\nbuild = false\n",
    )?;
    Ok(())
}

/// Builds `source`, runs the generated program and checks it prints
/// `expected`; answers the generated Rust of every module.
fn generated_output(source: &str, expected: &str, what: &str) -> TestResult<String> {
    let temp = TempProject::new()?;
    let root = temp.path();
    write_project(root, source)?;
    smelt_in(root, &["build"])?;
    let output = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(
        &output,
        &expected.to_owned(),
        format!("{what}: generated Rust stdout\n--- generated\n{output}--- expected\n{expected}"),
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

/// Checks `source` against Node's `expected` output, then
/// [`generated_output`].
fn agrees_with_node(source: &str, expected: &str, what: &str) -> TestResult<String> {
    ensure_node_agrees(&[("main.ts", source)], "main.ts", expected, what)?;
    generated_output(source, expected, what)
}

/// Fails with `what` unless `rust` contains `needle`.
fn has(rust: &str, needle: &str, what: &str) -> TestResult {
    ensure(
        rust.contains(needle),
        format!("{what}: expected `{needle}` in the generated Rust"),
    )
}

/// Fails with `what` when `rust` contains `needle`.
fn lacks(rust: &str, needle: &str, what: &str) -> TestResult {
    ensure(
        !rust.contains(needle),
        format!("{what}: unexpected `{needle}` in the generated Rust"),
    )
}

/// The program of [`a_record_seen_at_a_wider_property_type_is_the_same_object`].
const WIDE_TS: &str = r#"type Box = { n: number; label: string };
interface Loose { n: number | string; label: string }
interface Open { n: unknown; label: string }
interface Maybe { n?: number; label: string }
function bump(l: Loose): void { if (typeof l.n === "number") { l.n = l.n + 1; } }
function readOpen(o: Open): unknown { return o.n; }
function writeOpen(o: Open, v: unknown): void { o.n = v; }
function setMaybe(m: Maybe, v: number): void { m.n = v; }
const b: Box = { n: 1, label: "b" };
bump(b);
console.log(b.n);
const kept: Loose = b;
console.log(kept === b, kept.n, typeof kept.n);
b.n = 10;
console.log(kept.n, readOpen(b));
writeOpen(b, 42);
console.log(b.n, JSON.stringify(kept), Object.keys(kept).join(","));
const m: Maybe = b;
setMaybe(m, 7);
console.log(b.n, m.n, m === b, JSON.stringify(m));
kept.label = "renamed";
console.log(b.label, (kept as Box) === b, (m as Box).n);
"#;

/// What Node prints for [`WIDE_TS`].
const WIDE_OUTPUT: &str = concat!(
    "2\n",
    "true 2 number\n",
    "10 10\n",
    "42 {\"n\":42,\"label\":\"b\"} n,label\n",
    "7 7 true {\"n\":7,\"label\":\"b\"}\n",
    "renamed true 7\n",
);

/// A record passed where the slot declares a property at a wider type
/// (`number | string`, `unknown`, `n?: number` over `n: number`) is the
/// caller's object: writes in both directions, identity, keys and JSON.
#[test]
fn a_record_seen_at_a_wider_property_type_is_the_same_object() -> TestResult {
    let rust = agrees_with_node(WIDE_TS, WIDE_OUTPUT, "wide property views")?;
    let what = "wide property views";
    // Every handle keeps its own type; the slots are views of `Box`'s cell.
    has(&rust, " b: Box_", what)?;
    has(&rust, " kept: Loose", what)?;
    has(&rust, " m: Maybe", what)?;
    has(
        &rust,
        "struct Loose(::std::rc::Rc<dyn LooseSmeltView>)",
        what,
    )?;
    has(
        &rust,
        "impl LooseSmeltView for ::std::cell::RefCell<Box_Inner>",
        what,
    )?;
    has(
        &rust,
        "impl OpenSmeltView for ::std::cell::RefCell<Box_Inner>",
        what,
    )?;
    has(
        &rust,
        "impl MaybeSmeltView for ::std::cell::RefCell<Box_Inner>",
        what,
    )?;
    // The wide property is answered through converting guards at the slot's
    // own type; `Box` keeps `n: f64`.
    has(&rust, "fn n(&self) -> SmeltFieldRead<'_, SmeltUnion", what)?;
    has(
        &rust,
        "fn n(&self) -> SmeltFieldRead<'_, SmeltUnknown>",
        what,
    )?;
    has(
        &rust,
        "fn n(&self) -> SmeltFieldRead<'_, Option<f64>>",
        what,
    )?;
    has(&rust, "    n: f64,", what)?;
    // No slot record is built from the `Box`: no copy.
    lacks(&rust, "RefCell::new(LooseInner {", what)?;
    lacks(&rust, "RefCell::new(OpenInner {", what)?;
    lacks(&rust, "RefCell::new(MaybeInner {", what)?;
    Ok(())
}

/// The program of [`a_write_the_stored_type_cannot_hold_is_converted_to_it`].
const UNSOUND_TS: &str = r#"type Box = { n: number; label: string };
interface Loose { n: number | string; label: string }
function setText(l: Loose): void { l.n = "text"; }
function setNumber(l: Loose): void { l.n = 5; }
const b: Box = { n: 1, label: "b" };
const kept: Loose = b;
setNumber(b);
console.log(b.n, kept.n, kept === b);
setText(b);
console.log(typeof b.n, kept === b);
"#;

/// A write through a wider view of a value the object's own type cannot
/// hold (`"text"` into `n: number`, accepted by TypeScript through
/// `n: number | string`).
///
/// This is the type-unsound case `#507` met for widened class handles, and it
/// takes the same answer: the stored field keeps its declared type and the
/// value is converted to it (`smelt_view_store`, the erased carrier's checked
/// extraction, `NaN` for this string). The original field is not erased.
/// Node keeps the string (`typeof b.n` is `"string"`), so this output is
/// asserted on its own, not against Node; every other line agrees with Node.
#[test]
fn a_write_the_stored_type_cannot_hold_is_converted_to_it() -> TestResult {
    let what = "unsound write through a wide view";
    let rust = generated_output(UNSOUND_TS, "5 5 true\nnumber true\n", what)?;
    has(&rust, "    n: f64,", what)?;
    has(&rust, "smelt_view_store::<f64>(", what)?;
    lacks(&rust, "RefCell::new(LooseInner {", what)?;
    Ok(())
}

/// The program of [`a_literal_converted_to_two_record_types_is_one_object`].
const TWO_TYPES_TS: &str = r#"interface A { x: number; y: number }
interface B { x: number; z: number }
function touchA(a: A): void { a.x = a.x + 1; }
function touchB(b: B): void { b.x = b.x * 10; b.z = 7; }
const lit = { x: 1, y: 2, z: 3 };
touchA(lit);
touchB(lit);
console.log(lit.x, lit.z, JSON.stringify(lit));
const la: A = lit;
const lb: B = lit;
lb.x = 100;
la.y = 9;
console.log(la.x, lb.x, lit.y, la === (lit as A), JSON.stringify(lb), Object.keys(lb).join(","));
const kept = { x: 1, y: 2, z: 3 };
for (const key in kept) { console.log(key); }
touchA(kept);
touchB(kept);
console.log(kept.x, kept.z, JSON.stringify(kept));
const stash: A[] = [];
function keep(a: A): void { stash.push(a); }
keep(kept);
const kb: B = kept;
kb.x = 99;
console.log(stash[0].x, kept.x, stash[0] === (kept as A), JSON.stringify(stash[0]));
"#;

/// What Node prints for [`TWO_TYPES_TS`].
const TWO_TYPES_OUTPUT: &str = concat!(
    "20 7 {\"x\":20,\"y\":2,\"z\":7}\n",
    "100 100 9 true {\"x\":100,\"y\":9,\"z\":7} x,y,z\n",
    "x\n",
    "y\n",
    "z\n",
    "20 7 {\"x\":20,\"y\":2,\"z\":7}\n",
    "99 99 true {\"x\":99,\"y\":2,\"z\":7}\n",
);

/// A literal converted to record type `A` and to an unrelated record type
/// `B` is one object behind every handle, whether the literal is a record
/// (its values share one type, so it used to stay a dictionary) or must
/// stay a dictionary (its keys are enumerated with `for..in`).
#[test]
fn a_literal_converted_to_two_record_types_is_one_object() -> TestResult {
    let what = "a literal at two record types";
    let rust = agrees_with_node(TWO_TYPES_TS, TWO_TYPES_OUTPUT, what)?;
    // `lit` is its own record, viewed as `A` and `B`; `kept` stays the
    // dictionary its key loop needs, adopted by that same record shape.
    lacks(&rust, "let lit: SmeltRecord<", what)?;
    has(&rust, "let lit: SmeltRecord", what)?;
    has(&rust, " la: A", what)?;
    has(&rust, " lb: B", what)?;
    has(&rust, " kept: SmeltRecord<JsString, f64>", what)?;
    // The dictionary is adopted by the literal's own record (one shape for
    // every slot), never by `A` or `B`.
    has(&rust, "smelt_origin::<SmeltRecord", what)?;
    lacks(&rust, "smelt_origin::<A>", what)?;
    lacks(&rust, "smelt_origin::<B>", what)?;
    has(&rust, "struct A(::std::rc::Rc<dyn ASmeltView>)", what)?;
    has(&rust, "struct B(::std::rc::Rc<dyn BSmeltView>)", what)?;
    // Neither `A` nor `B` is ever built from the literal: no copy.
    lacks(&rust, "RefCell::new(AInner {", what)?;
    lacks(&rust, "RefCell::new(BInner {", what)?;
    Ok(())
}

/// The program of [`a_typeof_test_narrows_a_property_read`].
const NARROW_TS: &str = r#"interface Loose { n: number | string; label: string }
function bump(l: Loose): void { if (typeof l.n === "number") { l.n = l.n + 1; } }
function shout(l: Loose): string { return typeof l.n === "string" ? l.n.toUpperCase() : `#${l.n.toFixed(1)}`; }
const l: Loose = { n: 1, label: "x" };
bump(l);
console.log(l.n, shout(l));
l.n = "abc";
bump(l);
console.log(l.n, shout(l));
"#;

/// What Node prints for [`NARROW_TS`].
const NARROW_OUTPUT: &str = "2 #2.0\nabc ABC\n";

/// `typeof l.n === "number"` narrows the property read `l.n` as TypeScript
/// does, so arithmetic adds and string methods apply to the narrowed arm.
#[test]
fn a_typeof_test_narrows_a_property_read() -> TestResult {
    let what = "typeof narrows a property read";
    let rust = agrees_with_node(NARROW_TS, NARROW_OUTPUT, what)?;
    has(&rust, " l: Loose", what)?;
    has(&rust, "    n: SmeltUnion", what)?;
    Ok(())
}
