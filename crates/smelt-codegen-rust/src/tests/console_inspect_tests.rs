//! Emitted Rust for `console.log`'s `util.inspect` route and for the console
//! spelling of Python's `print`.
//!
//! What gets printed is checked against Node by the `console_inspect_shapes`
//! fixture and against `python3` by the `print_spellings` fixture. These tests
//! pin the emitted shape: which arguments take the trait route, that the trait
//! runtime is pay-for-use, and that no value is erased to be formatted.

use super::*;

/// A list argument is borrowed at its own type and renders through its
/// `SmeltInspect` impl.
#[test]
fn console_log_of_a_list_renders_through_inspect() {
    let source = source_for("const values: number[] = [1, 2];\nconsole.log(values);\n");
    assert_eq!(
        rust_fn(&source, "main"),
        r#"fn main() {
let values: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
let _ = { println!("{}", SmeltInspect::smelt_console_text(&values)); };
return;
}"#
    );
}

/// A program whose console arguments are all scalars formats them directly
/// and carries no inspect runtime.
#[test]
fn scalar_console_arguments_emit_no_inspect_runtime() {
    let source = source_for("let count = 42;\nconsole.log(count, \"label\", true);\n");
    assert_eq!(
        rust_fn(&source, "main"),
        r#"fn main() {
let count: f64 = 42.0;
let _ = { println!("{} {} {}", smelt_console_number(count), JsString::from("label"), true); };
return;
}"#
    );
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
    assert!(
        source.contains("smelt_inspect_object(ctx, depth, \"\", &entries)"),
        "{source}"
    );
    assert!(source.contains("impl SmeltInspect for Pair {"), "{source}");
    assert!(
        source.contains("smelt_inspect_object(ctx, depth, \"Pair\", &entries)"),
        "{source}"
    );
}

/// A record holding a function takes the plain rendering: closures implement
/// no `SmeltInspect`, so the record has no impl to call.
#[test]
fn a_record_holding_a_function_does_not_take_the_inspect_route() {
    let source = source_for(
        "interface Handler { name: string; run: (x: number) => number }\n\
         const h: Handler = { name: \"h\", run: (x: number) => x };\n\
         console.log(h);\n",
    );
    assert_eq!(
        rust_fn(&source, "main"),
        r#"fn main() {
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: f64| {
closure_arg_0
});
let h: Handler = Handler { name: JsString::from("h"), run: _smelt_tmp_1.clone() };
let _ = { println!("{}", { let smelt_object_value = h; let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::<(String, _)>::new(); smelt_object_entries.push(("name".to_owned(), SmeltUnknown::String(smelt_object_value.name.into()))); smelt_object_entries.push(("run".to_owned(), { let smelt_function_value = smelt_object_value.run; if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) { smelt_callable_object } else { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function_value); let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Number((smelt_function_value)(match smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }) as f64))); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); smelt_link_function_identity_key(&smelt_erased_function, smelt_origin_identity); smelt_register_function_length_once(&smelt_erased_function, 1.0); SmeltUnknown::Function(smelt_erased_function) } })); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }); };
return;
}"#
    );
    assert!(
        !source.contains("impl SmeltInspect for Handler"),
        "{source}"
    );
}

/// Python's `print` spells `True`/`False` and `None`, not JavaScript's
/// `true`/`null`, and never takes the inspect route.
#[test]
fn python_print_spells_booleans_and_none_like_python() {
    let source = source_for_py(
        "def run() -> None:\n    flag: bool = True\n    print(flag)\n    nothing: None = None\n    print(nothing)\n",
    );
    assert_eq!(
        rust_fn(&source, "run"),
        r#"fn run() -> () {
let nothing: ();
let flag: bool = true;
let _ = { println!("{}", if flag { "True" } else { "False" }); };
nothing = ();
let _ = { println!("{}", "None"); };
return;
}"#
    );
    assert!(!source.contains("SmeltInspect"), "{source}");
}
