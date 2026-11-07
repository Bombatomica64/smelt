//! Valid TypeScript whose generated Rust must print what Node prints.
//!
//! Each `fixtures/node-parity/<name>.ts` runs through a full `smelt build` and a
//! compile of the generated crate, and its stdout is compared with the committed
//! `<name>.stdout` and with Node itself. These programs pin a language rule rather
//! than a corpus shape, so they sit outside the example corpus the typedness and
//! `SmeltUnknown` baselines measure: their `unknown` parameters are the dynamic
//! boundary the rule is about.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure_eq, ensure_node_agrees, smelt_in,
    workspace_root,
};

/// Build fixture `name` and check its stdout against the golden and Node.
fn verify_node_parity(name: &str) -> TestResult {
    verify_node_parity_with(name, &[])
}

/// [`verify_node_parity`] for a program that imports `modules`, the sibling
/// files of `fixtures/node-parity/<name>/`.
fn verify_node_parity_with(name: &str, modules: &[&str]) -> TestResult {
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/node-parity");
    let source = fs::read_to_string(fixtures.join(format!("{name}.ts")))?;
    let expected = fs::read_to_string(fixtures.join(format!("{name}.stdout")))?;
    let module_sources = modules
        .iter()
        .map(|module| fs::read_to_string(fixtures.join(name).join(module)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut files = vec![("main.ts", source.as_str())];
    files.extend(modules.iter().copied().zip(module_sources.iter().map(String::as_str)));
    ensure_node_agrees(&files, "main.ts", &expected, name)?;
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    for (file, text) in &files {
        fs::write(root.join("src").join(file), text)?;
    }
    fs::write(
        root.join("Smelt.toml"),
        r#"[project]
name = "node-parity"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "node_parity"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    smelt_in(root, &["build"])?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &expected, format!("{name}: generated Rust stdout"))
}

/// A copy keeps its value when a closure writes the source afterwards, whether the
/// copy sits in a function or a closure and however deep the writing closure is.
#[test]
fn copy_before_write() -> TestResult {
    verify_node_parity("copy_before_write")
}

/// An operand keeps the value it had where it was evaluated.
#[test]
fn evaluation_order() -> TestResult {
    verify_node_parity("evaluation_order")
}

/// Rest arguments are packed on every kind of callee.
#[test]
fn rest_arguments() -> TestResult {
    verify_node_parity("rest_arguments")
}

/// Rest arguments are packed by the frontend on every route through a function value.
#[test]
fn rest_value_calls() -> TestResult {
    verify_node_parity("rest_value_calls")
}

/// A list packed for a callee that is all rest goes inside the call, in order.
#[test]
fn rest_call_fusion() -> TestResult {
    verify_node_parity("rest_call_fusion")
}

/// An imported `value is T` narrows where it answers true, and only there.
#[test]
fn imported_type_predicate() -> TestResult {
    verify_node_parity_with("imported_type_predicate", &["predicates.ts"])
}

/// A throw from a `map` or `sort` callback reaches the enclosing `try`.
#[test]
fn try_around_raising_callback() -> TestResult {
    verify_node_parity("try_around_raising_callback")
}

/// `+` on erased operands concatenates where the checker says it does.
#[test]
fn heterogeneous_object_constant() -> TestResult {
    verify_node_parity("heterogeneous_object_constant")
}

/// A run-time key reads the methods of a class instance too.
#[test]
fn class_function_field_by_key() -> TestResult {
    verify_node_parity("class_function_field_by_key")
}

/// A class reads its own static fields from its method bodies.
#[test]
fn local_class_static_self_read() -> TestResult {
    verify_node_parity("local_class_static_self_read")
}

/// Calling what a value does not hold throws `TypeError`; reading it does not.
#[test]
fn call_of_what_a_value_lacks() -> TestResult {
    verify_node_parity("call_of_what_a_value_lacks")
}

/// `new value.constructor()` builds the value's kind, and prints as one.
#[test]
fn construct_through_constructor() -> TestResult {
    verify_node_parity("construct_through_constructor")
}

/// A generic called through a namespace import types as a direct call does.
#[test]
fn namespace_generic_call() -> TestResult {
    verify_node_parity_with("namespace_generic_call", &["lib.ts"])
}

/// A field typed `T | boolean` is stored erased at every instantiation.
#[test]
fn generic_union_field() -> TestResult {
    verify_node_parity("generic_union_field")
}

/// `super.method()` runs the base implementation, through every level.
#[test]
fn super_method_call() -> TestResult {
    verify_node_parity("super_method_call")
}

/// A subclass reads and calls its ancestors' statics.
#[test]
fn inherited_statics() -> TestResult {
    verify_node_parity("inherited_statics")
}

/// A class whose base is chosen at run time is specialised per arm.
#[test]
fn conditional_base_class() -> TestResult {
    verify_node_parity("conditional_base_class")
}

/// A type alias another module declares is in scope only where it is imported.
#[test]
fn type_alias_scope() -> TestResult {
    verify_node_parity_with("type_alias_scope", &["helpers.ts"])
}

/// A needle typed by a parameter bounded by `string` is a string.
#[test]
fn string_bound_needle() -> TestResult {
    verify_node_parity("string_bound_needle")
}

/// A receiver's own `startsWith`/`includes` wins over the string methods.
#[test]
fn member_named_like_string_method() -> TestResult {
    verify_node_parity_with("member_named_like_string_method", &["pattern.ts"])
}

/// A parameter does not hide a module constant from other functions.
#[test]
fn parameter_shadows_module_const() -> TestResult {
    verify_node_parity("parameter_shadows_module_const")
}

/// `replace` substitutes ToString of the replacer's answer.
#[test]
fn replace_callback_tostring() -> TestResult {
    verify_node_parity("replace_callback_tostring")
}

/// A class satisfies `implements` through its base chain.
#[test]
fn implements_inherited_members() -> TestResult {
    verify_node_parity("implements_inherited_members")
}

/// A container of a type parameter prints through `util.inspect`, never Rust `Debug`.
#[test]
fn console_erased_generic() -> TestResult {
    verify_node_parity("console_erased_generic")
}

/// A class stored in a value keeps its identity for `instanceof` and `new`.
#[test]
fn class_values() -> TestResult {
    verify_node_parity("class_values")
}

/// `f(a, (a = 2))`: an assignment as an argument evaluates in place.
#[test]
fn assignment_argument() -> TestResult {
    verify_node_parity("assignment_argument")
}

/// A nested closure writes the enclosing closure's parameter.
#[test]
fn closure_param_written_by_closure() -> TestResult {
    verify_node_parity("closure_param_written_by_closure")
}

/// A `Set` that crosses `unknown` keeps its members and insertion order.
#[test]
fn erased_set_order() -> TestResult {
    verify_node_parity("erased_set_order")
}

/// Same-named classes in different scopes stay different classes.
#[test]
fn block_local_classes() -> TestResult {
    verify_node_parity("block_local_classes")
}

/// A string key is a property name on whatever the erased receiver holds.
#[test]
fn string_key_on_erased_receiver() -> TestResult {
    verify_node_parity("string_key_on_erased_receiver")
}

/// An interface with a numeric index signature admits a list.
#[test]
fn array_like_interface() -> TestResult {
    verify_node_parity("array_like_interface")
}

/// A method called on an erased value is what the value holds.
#[test]
fn erased_member_calls() -> TestResult {
    verify_node_parity("erased_member_calls")
}

/// A generator's `T` reachable only through a union stays what the definition yields.
#[test]
fn generator_type_through_union() -> TestResult {
    verify_node_parity("generator_type_through_union")
}

/// Array-valued ternary arms join instead of one adopting the other's type.
#[test]
fn conditional_list_arms() -> TestResult {
    verify_node_parity("conditional_list_arms")
}

/// A declared `any` return is not inferred over.
#[test]
fn declared_erased_return() -> TestResult {
    verify_node_parity("declared_erased_return")
}

/// `typeof` of an optional is runtime, and its `case "object"` keeps dynamic members.
#[test]
fn typeof_of_optional() -> TestResult {
    verify_node_parity("typeof_of_optional")
}

/// `DOMException` is an `Error`, `File` a `Blob`, a `TypeError` an `Error`, and a
/// union arm answers for its base classes.
#[test]
fn builtin_prototype_chains() -> TestResult {
    verify_node_parity("builtin_prototype_chains")
}

/// A declared member beats the builtin of that name on every receiver shape.
#[test]
fn declared_members_win() -> TestResult {
    verify_node_parity("declared_members_win")
}

/// A value thrown from a callback keeps its class, not just a message.
#[test]
fn thrown_from_callback() -> TestResult {
    verify_node_parity("thrown_from_callback")
}

/// A closure's omitted argument takes its default, per call and in the callee.
#[test]
fn closure_parameter_defaults() -> TestResult {
    verify_node_parity("closure_parameter_defaults")
}

/// Statement and expression forms in a callback body mean what they mean anywhere.
#[test]
fn callback_bodies() -> TestResult {
    verify_node_parity("callback_bodies")
}

/// `T` is what TypeScript infers across every candidate position of a call.
#[test]
fn generic_call_candidates() -> TestResult {
    verify_node_parity("generic_call_candidates")
}

/// A declared function type is what a call through the binding sees.
#[test]
fn declared_function_types() -> TestResult {
    verify_node_parity("declared_function_types")
}

/// `value` and `done` are ordinary fields except on an iterator result.
#[test]
fn iterator_results_and_fields() -> TestResult {
    verify_node_parity("iterator_results_and_fields")
}

/// An operation after narrowing a union reads the real value.
#[test]
fn narrowed_union_receivers() -> TestResult {
    verify_node_parity("narrowed_union_receivers")
}

/// `_.iterate(..)` answers what `iterate(..)` answers.
#[test]
fn namespace_generic_calls() -> TestResult {
    verify_node_parity_with("namespace_generic_calls", &["lib.ts"])
}

/// A spread fills a rest parameter element by element through a function value.
#[test]
fn spread_into_rest() -> TestResult {
    verify_node_parity("spread_into_rest")
}

/// An uppercasing replacer is an ordinary callback, for each kind of match.
#[test]
fn regex_replace_uppercase_callback() -> TestResult {
    verify_node_parity("regex_replace_uppercase_callback")
}

/// A push through a dict entry appends in place and inserts a missing key.
#[test]
fn dict_entry_push_in_place() -> TestResult {
    verify_node_parity("dict_entry_push_in_place")
}
