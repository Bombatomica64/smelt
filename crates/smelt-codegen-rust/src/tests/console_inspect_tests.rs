//! Emitted Rust for `console.log`'s `util.inspect` route and for the console
//! spelling of Python's `print`.
//!
//! The runtime behaviour is checked against Node itself by the end-to-end
//! corpus (`06_array_literal`, `07_tuple_literal`, `44_node_http_echo`) and
//! against `python3` by the Python corpus; these tests pin the emitted SHAPE:
//! which arguments take the trait route, that the trait runtime is
//! pay-for-use, and that no value is erased to be formatted.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631): judge compiled output
//! against the JavaScript runtime (`docs/bundler/aot-debugging.mdx`).

use super::*;

/// A list argument renders through the `SmeltInspect` impl of its own Rust
/// type (`SmeltList<f64>`), and the runtime ships ahead of the prelude marker.
#[test]
fn console_log_of_a_list_renders_through_inspect() {
    let source = source_for("const values: number[] = [1, 2];\nconsole.log(values);\n");
    assert!(
        source.contains("println!(\"{}\", SmeltInspect::smelt_console_text(&values))"),
        "{source}"
    );
    assert!(source.contains("impl<T: SmeltInspect> SmeltInspect for SmeltList<T>"), "{source}");
    let marker = source.find(PRELUDE_END_MARKER).unwrap_or(0);
    let runtime = source.find("trait SmeltInspect").unwrap_or(usize::MAX);
    assert!(runtime < marker, "the inspect runtime belongs to the prelude: {source}");
    // Formatting is not an erasure: the list is borrowed at its own type.
    assert!(!source.contains("values.clone().into_smelt_unknown()"), "{source}");
}

/// A program whose console arguments are all scalars carries none of it.
#[test]
fn scalar_console_arguments_emit_no_inspect_runtime() {
    let source = source_for("let count = 42;\nconsole.log(count, \"label\", true);\n");
    assert!(!source.contains("SmeltInspect"), "{source}");
}

/// A record struct gets its own impl, listing its fields under their SOURCE
/// keys, and a class instance carries its constructor name.
#[test]
fn records_and_classes_get_field_listing_impls() {
    let source = source_for(
        "interface Point { x: number; label?: string }\n\
         class Pair { constructor(public left: number, public right: string) {} }\n\
         const p: Point = { x: 1 };\n\
         console.log(p);\n\
         console.log(new Pair(1, \"r\"));\n",
    );
    assert!(source.contains("impl SmeltInspect for Point {"), "{source}");
    // An optional shape property is skipped when absent, as Node shows it.
    assert!(
        source.contains("if let Some(value) = &self.label { entries.push((\"label\", value)); }"),
        "{source}"
    );
    assert!(source.contains("smelt_inspect_object(ctx, depth, \"\", &entries)"), "{source}");
    assert!(source.contains("impl SmeltInspect for Pair {"), "{source}");
    assert!(source.contains("smelt_inspect_object(ctx, depth, \"Pair\", &entries)"), "{source}");
}

/// A record holding a function has no field listing Node would print the same
/// way, so it keeps the previous rendering instead of taking the trait route
/// (which would not compile: closures implement no `SmeltInspect`).
#[test]
fn a_record_holding_a_function_keeps_the_previous_route() {
    let source = source_for(
        "interface Handler { name: string; run: (x: number) => number }\n\
         const h: Handler = { name: \"h\", run: (x: number) => x };\n\
         console.log(h);\n",
    );
    assert!(!source.contains("SmeltInspect::smelt_console_text(&h"), "{source}");
    assert!(!source.contains("impl SmeltInspect for Handler"), "{source}");
}

/// Python's `print` spells `True`/`False` and `None`, not JavaScript's
/// `true`/`null`, and never takes the inspect route.
#[test]
fn python_print_spells_booleans_and_none_like_python() {
    let source = source_for_py("flag: bool = True\nprint(flag)\nnothing: None = None\nprint(nothing)\n");
    assert!(source.contains("if flag { \"True\" } else { \"False\" }"), "{source}");
    assert!(source.contains("println!(\"{}\", \"None\")"), "{source}");
    assert!(!source.contains("SmeltInspect"), "{source}");
}
