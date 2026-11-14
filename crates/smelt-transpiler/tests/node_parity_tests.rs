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
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt_in,
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
    files.extend(
        modules
            .iter()
            .copied()
            .zip(module_sources.iter().map(String::as_str)),
    );
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

/// Truthiness of every value kind, nullish fallbacks, non-null forwarding, by-reference
/// list aliasing, method-reference identity and absent keyed reads.
#[test]
fn truthiness_absence_identity() -> TestResult {
    verify_node_parity("truthiness_absence_identity")
}

/// `??` whose fallback is an erased `?.` chain treats a present `undefined` or
/// `null` as absent and keeps a present `0`, so the chain's own last fallback
/// still applies (it was `NaN`).
#[test]
fn erased_nullish_chain() -> TestResult {
    verify_node_parity("erased_nullish_chain")
}

/// A `let`/`const` read in its temporal dead zone throws a catchable
/// `ReferenceError`, through a closure called early or a `switch` case that
/// skipped the declaration; a hoisted function or a recursive arrow that runs
/// after the binding it reads is initialized is unaffected (C8).
#[test]
fn temporal_dead_zone() -> TestResult {
    verify_node_parity("temporal_dead_zone")
}

/// Shapes that contain themselves by value (mutual and self recursion through
/// optional fields, union arms, generic nodes, a by-value generic wrapper, a
/// class with an optional parameter property) compile as reference handles and
/// keep JavaScript's sharing; a cycle through an array stays a value.
#[test]
fn recursive_shapes() -> TestResult {
    verify_node_parity("recursive_shapes")
}

/// bigint arithmetic, its tag behind `unknown`, 64-bit views and its catchable errors.
#[test]
fn bigint_boundaries() -> TestResult {
    verify_node_parity("bigint_boundaries")
}

/// Destructuring defaults: a default replaces `undefined` only (a present
/// `null`, `0`, `false` or `""` is kept), an `unknown` source keeps its
/// binding `unknown` instead of taking the default's type, and the default is
/// evaluated only when it applies.
#[test]
fn destructuring_defaults() -> TestResult {
    verify_node_parity("destructuring_defaults")
}

/// `console.log` renders lists, records and class instances as `util.inspect` does.
#[test]
fn console_inspect_shapes() -> TestResult {
    verify_node_parity("console_inspect_shapes")
}

/// `.match`/`.search` on an `any` receiver are resolved by its run-time value:
/// a string answers `String.prototype`, an object its own member, and a value
/// without one throws a catchable `TypeError`; a match array prints with its
/// `index`, `input` and `groups`.
#[test]
fn erased_pattern_methods() -> TestResult {
    verify_node_parity("erased_pattern_methods")
}

/// String methods on concrete and erased receivers, and strings in boolean position.
#[test]
fn string_method_semantics() -> TestResult {
    verify_node_parity("string_method_semantics")
}

/// Replacement callbacks, flags, match values, named groups and `lastIndex`.
#[test]
fn regexp_semantics() -> TestResult {
    verify_node_parity("regexp_semantics")
}

/// Date parsing, setters and ISO output, and URL host accessors.
#[test]
fn date_semantics() -> TestResult {
    verify_node_parity("date_semantics")
}

/// Captures share one cell, a method call supplies its receiver, a function value
/// is an object, and every call shape honours the callee's argument ABI.
#[test]
fn function_value_semantics() -> TestResult {
    verify_node_parity("function_value_semantics")
}

/// A function keeps one identity across its representations: erased into
/// `unknown`, read at a typed slot, stored in a list, a `Set` or a `Map`, and
/// found again with `===`, `indexOf`, `includes`, `has` and `get`.
#[test]
fn function_identity() -> TestResult {
    verify_node_parity("function_identity")
}

/// A `catch` runs wherever its `try` sits: a loop, a closure, an `if` arm, a callback.
#[test]
fn throw_and_catch_forms() -> TestResult {
    verify_node_parity("throw_and_catch_forms")
}

/// Callback ABI forms: a callback writing its parameter's field reaches the
/// caller's object, rest callbacks in `forEach`/`map` get the tuple of their
/// arguments, `===` on borrowed callbacks compares identity, a borrowed
/// erased-rest callback forwards to a typed slot, a `switch` with no default
/// returns from an arrow, and a method fits a generic interface field.
#[test]
fn closure_abi_forms() -> TestResult {
    verify_node_parity("closure_abi_forms")
}

/// Async closures, generators and timers behave as Node runs them.
#[test]
fn async_forms() -> TestResult {
    verify_node_parity("async_forms")
}

/// `setTimeout`, `setInterval` and `clearInterval` used as values schedule
/// and cancel like direct calls, every pending timer runs before the program
/// exits, and an interval's callback can cancel the interval it belongs to.
#[test]
fn timer_values() -> TestResult {
    verify_node_parity("timer_values")
}

/// A throw from a generator body reaches the `try` around whatever resumed
/// it (`.next()`, `for...of`, a spread, `Array.from`, a `yield*` in another
/// generator), whatever value is thrown, and the generator is done afterwards.
#[test]
fn generator_throws() -> TestResult {
    verify_node_parity("generator_throws")
}

/// A throw delivers its operand whole through one channel, whatever it is, and
/// a `try` around a call that cannot throw still builds and runs.
#[test]
fn thrown_payload_channel() -> TestResult {
    verify_node_parity("thrown_payload_channel")
}

/// Loops, switch fallthrough, rest packing, parameter defaults and optional
/// callbacks evaluate the way the language defines them.
#[test]
fn frontend_control_flow() -> TestResult {
    verify_node_parity("frontend_control_flow")
}

/// A computed key read from an `any` receiver names the property at run time.
#[test]
fn computed_member_object_keys() -> TestResult {
    verify_node_parity("computed_member_object_keys")
}

/// An escaping closure keeps its body when it captures a callback parameter: a
/// body-local sharing a parameter's name owns its callback, so the capture is
/// judged by the local's identity (radash `series`).
#[test]
fn closure_over_defaulted_callback() -> TestResult {
    verify_node_parity("closure_over_defaulted_callback")
}

/// The expression of an `export default` runs, in source order.
#[test]
fn export_default_effects() -> TestResult {
    verify_node_parity("export_default_effects")
}

/// `Object` and `Symbol` are values `.constructor` compares equal to, a module
/// const aliasing a builtin is callable from any body, and a list callback picked
/// by a conditional is not replaced by one of its arms.
#[test]
fn builtin_constructor_values() -> TestResult {
    verify_node_parity("builtin_constructor_values")
}

/// A self-reaching class or list erases to one cycle, `c ? [] : {}` keeps the array,
/// and a callback selected at run time is the one that runs.
#[test]
fn cyclic_erasure_and_selected_values() -> TestResult {
    verify_node_parity("cyclic_erasure_and_selected_values")
}

/// An erased primitive `Set` keeps the `Set` shape: `instanceof`, `toString`,
/// iteration and `console.log` match Node.
#[test]
fn erased_set_view() -> TestResult {
    verify_node_parity("erased_set_view")
}

/// A nested `async function` without a return annotation still returns a Promise.
#[test]
fn nested_async_declaration_return() -> TestResult {
    verify_node_parity("nested_async_declaration_return")
}

/// A keyed write through an erased view reaches the instance, an implementation
/// whose overloads return an array or an object returns what it built, and
/// `Object.fromEntries` accepts `[k, v]` entries spelled as plain arrays.
#[test]
fn keyed_view_write_and_shape_overloads() -> TestResult {
    verify_node_parity("keyed_view_write_and_shape_overloads")
}

/// A Proxy answers through its handler's traps on object, function and callable
/// object targets, and `f(x)(...args)` spreads into the returned function.
#[test]
fn proxy_traps() -> TestResult {
    verify_node_parity("proxy_traps")
}

/// `+` on contextually typed parameters concatenates, and symbol keys survive a
/// `keyof` array assertion.
#[test]
fn contextual_add_and_symbol_keys() -> TestResult {
    verify_node_parity("contextual_add_and_symbol_keys")
}

/// An uncaught throw exits as Node does: status 1, with the thrown class and
/// message on stderr. Covers a throw returned out of `main` and one that crosses
/// a callback through the panic route.
#[test]
fn uncaught_throw_exits_one_with_its_message() -> TestResult {
    let programs = [
        (
            "returned out of main",
            "throw new TypeError('top level');\n",
            "TypeError: top level",
        ),
        (
            "through a callback",
            "function boom(n: number): number { throw new RangeError('nope ' + n); }\n[1].map(boom);\n",
            "RangeError: nope 1",
        ),
        (
            "floating rejection",
            "async function fail(): Promise<void> { throw new Error('floating'); }\nfail();\nsetTimeout(() => console.log('never'), 50);\n",
            "Error: floating",
        ),
    ];
    for (name, source, message) in programs {
        let project = TempProject::new()?;
        let root = project.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/main.ts"), source)?;
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
"#,
        )?;
        smelt_in(root, &["build"])?;
        let output = std::process::Command::new("cargo")
            .args(["run", "--quiet", "--manifest-path", "dist/Cargo.toml"])
            .current_dir(root)
            .env("RUSTFLAGS", "-Awarnings")
            .output()?;
        ensure_eq(
            &output.status.code(),
            &Some(1_i32),
            format!("{name}: exit status"),
        )?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure_eq(
            &stderr.contains(message),
            &true,
            format!("{name}: stderr was {stderr}"),
        )?;
    }
    Ok(())
}

/// The rest of a Proxy's observable behaviour: `deleteProperty`, a bare read that
/// runs its `get` trap, and enumeration following direct writes to the target.
#[test]
fn proxy_completion() -> TestResult {
    verify_node_parity("proxy_completion")
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

/// `replace` substitutes `ToString` of the replacer's answer.
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

/// `console.log` follows `util.inspect`: quoting, depth, grouping, line breaks,
/// absent optional properties, class prefixes, tuples and collections.
#[test]
fn console_inspect_layout() -> TestResult {
    verify_node_parity("console_inspect_layout")
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

/// A block-local alias redeclared later is the later block's type, and
/// `new Map(..)` takes the map arm of a union contextual type.
#[test]
fn block_local_type_alias_redeclaration() -> TestResult {
    verify_node_parity("block_local_type_alias_redeclaration")
}

/// Source items named like Rust prelude items (`Fn`, `Iterator`, `Clone`,
/// `Default`, `drop`) neither shadow the prelude nor lose their own meaning.
#[test]
fn prelude_named_items() -> TestResult {
    verify_node_parity("prelude_named_items")
}

/// Names that render alike stay distinct: same-named static and instance
/// members, a module body function and the module's own export, a `_` key.
#[test]
fn rendered_name_collisions() -> TestResult {
    verify_node_parity_with("rendered_name_collisions", &["is-even.ts"])
}

/// Enum members auto-increment, string members fold, numeric members map back
/// to their names, and an enum may be declared inside a function.
#[test]
fn enums() -> TestResult {
    verify_node_parity("enums")
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

/// A namespace re-exported under a name is the module's namespace in the
/// importer, and a class may name itself from its own members.
#[test]
fn namespace_reexports() -> TestResult {
    verify_node_parity_with(
        "namespace_reexports",
        &["index.ts", "patterns.ts", "symbols.ts"],
    )
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

/// `parseInt` and `parseFloat` read the longest valid prefix, `parseInt`
/// detects `0x` without a radix, and `xs.map(parseInt)` parses in the index radix.
#[test]
fn parse_number_prefix() -> TestResult {
    verify_node_parity("parse_number_prefix")
}

/// Every string index counts UTF-16 code units: the ones `indexOf`, `search`,
/// a match's `index` and `lastIndex` answer and the ones `slice`, `substring`,
/// `substr`, `charAt`, `s[i]` and `padEnd` take, for non-ASCII and astral text.
#[test]
fn string_utf16_indices() -> TestResult {
    verify_node_parity("string_utf16_indices")
}

/// A list or typed-array element read past the end is `undefined` wherever
/// the reader's type can hold `undefined`, and in range it is the element.
#[test]
fn element_read_misses() -> TestResult {
    verify_node_parity("element_read_misses")
}

/// A regexp's absent values: a non-participating group is `undefined`,
/// `indices` exists with the `d` flag, a missing property is `undefined`, and
/// `RegExpMatchArray`/`RegExpExecArray` annotations name the match value.
#[test]
fn regexp_absent_values() -> TestResult {
    verify_node_parity("regexp_absent_values")
}

/// Environment variable names are case-sensitive: only `TZ` is the zone.
#[test]
fn env_names_case_sensitive() -> TestResult {
    verify_node_parity("env_names_case_sensitive")
}

/// A class the program declares under a host constructor's name is that class,
/// for `new` and for `instanceof`.
#[test]
fn declared_class_shadows_host() -> TestResult {
    verify_node_parity("declared_class_shadows_host")
}

/// `Reflect.has(target, key)` answers like `key in target`.
#[test]
fn reflect_has_is_in() -> TestResult {
    verify_node_parity("reflect_has_is_in")
}

/// An object literal's getter runs on every read, at its point of evaluation,
/// whatever spells its key.
#[test]
fn object_literal_getters() -> TestResult {
    verify_node_parity("object_literal_getters")
}

/// `String` of an erased array joins its elements.
#[test]
fn erased_string_joins_arrays() -> TestResult {
    verify_node_parity("erased_string_joins_arrays")
}

/// A field read keeps its value when a later argument writes the field through
/// a computed key or `Object.assign`.
#[test]
fn field_read_before_keyed_write() -> TestResult {
    verify_node_parity("field_read_before_keyed_write")
}

/// `a ?? b!` evaluates the asserted fallback only when it is selected.
#[test]
fn coalesce_asserted_fallback_is_lazy() -> TestResult {
    verify_node_parity("coalesce_asserted_fallback_is_lazy")
}

/// A primitive consequent does not convert a different primitive alternate.
#[test]
fn conditional_primitive_arms() -> TestResult {
    verify_node_parity("conditional_primitive_arms")
}

/// An argument is spread only when the call writes `...arg`.
#[test]
fn spread_is_syntax() -> TestResult {
    verify_node_parity("spread_is_syntax")
}

/// A `throw` inside an `if` in a `try` runs the `catch` before the code after it.
#[test]
fn throw_in_branch_reaches_catch() -> TestResult {
    verify_node_parity("throw_in_branch_reaches_catch")
}

/// The code after a `try` whose `catch` may rethrow runs, a later loop included.
#[test]
fn code_after_nested_try_with_loop() -> TestResult {
    verify_node_parity("code_after_nested_try_with_loop")
}

/// A record is an object: an alias, an array element and a parameter all
/// reach the same object.
#[test]
fn record_aliasing() -> TestResult {
    verify_node_parity("record_aliasing")
}

/// A lone surrogate is a string value of its own (`"\ud83d"`, a cut through a
/// surrogate pair), and strings sort by UTF-16 code unit.
#[test]
fn lone_surrogate_strings() -> TestResult {
    verify_node_parity("lone_surrogate_strings")
}

/// A read of an object's state happens where it is evaluated, ahead of a later
/// argument that changes it: `re.test(s)` moving `lastIndex`, and a method
/// writing its own field.
#[test]
fn sibling_reads_before_a_state_change() -> TestResult {
    verify_node_parity("sibling_reads_before_a_state_change")
}

/// `Object.assign` writes onto its target and returns it, for an interface
/// record, a member of an object and a parameter, across interface heritage.
#[test]
fn object_assign_write_through() -> TestResult {
    verify_node_parity("object_assign_write_through")
}

/// A generic record rebuilt from the erased instantiation a generic function
/// returns converts its callable fields across the type argument, a nested
/// callback included, and a callback handed to a slot meets the slot's
/// declared ABI (ts-pattern's `Match<i>`).
#[test]
fn record_callable_type_arguments() -> TestResult {
    verify_node_parity("record_callable_type_arguments")
}

/// A Date setter on an erased receiver (a `D extends Date` generic) borrows
/// the receiver to read its timestamp and writes the new one into the same
/// object, so the receiver is not moved twice and every alias sees the write.
#[test]
fn date_setter_erased_receiver() -> TestResult {
    verify_node_parity("date_setter_erased_receiver")
}

/// A generic alias over a union with an object arm (`{ value: V; rest: string }
/// | null`) lowers to an optional generic record, a generic helper mapping it
/// stays generic, and `if (!r) return r;` returns the narrowed `null`.
#[test]
fn generic_record_result_alias() -> TestResult {
    verify_node_parity("generic_record_result_alias")?;
    // Type preservation, on the same source: `mapValue` keeps both
    // parameters as Rust generics, and each call passes its arrow at the
    // instantiated type (`(n) => n * 2` takes `&f64`), not through an erased
    // adapter. The arrow's parameter type is TypeScript's instantiated one
    // (`callbacks::body_lowering::names_type_param_out_of_scope`); with the
    // callee's bare `A` the call bound `A` to `SmeltUnknown` (E0631).
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/node-parity");
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::copy(
        fixtures.join("generic_record_result_alias.ts"),
        root.join("src/main.ts"),
    )?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"generic-record\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n[output]\ntarget = \"./dist\"\nbuild = false\n",
    )?;
    smelt_in(root, &["build"])?;
    let mut source = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        source.push_str(&fs::read_to_string(entry?.path())?);
    }
    let map_value = source
        .lines()
        .find(|line| line.contains("fn map_value<"))
        .ok_or("map_value did not keep its generics")?;
    ensure(
        !map_value.contains("SmeltUnknown>") && !map_value.contains(": SmeltUnknown"),
        map_value.to_owned(),
    )?;
    for call in source
        .lines()
        .filter(|line| line.contains("map_value(") && !line.contains("fn map_value"))
    {
        ensure(
            !call.contains("&SmeltUnknown"),
            format!("erased callback adapter: {call}"),
        )?;
    }
    Ok(())
}

/// An integer-valued expression fills a union's `number` arm, and
/// `String.prototype.match` with an absent pattern matches the empty pattern.
#[test]
fn number_union_and_optional_pattern() -> TestResult {
    verify_node_parity("number_union_and_optional_pattern")
}

/// A narrowing proved inside one class member (a method, a constructor) does
/// not leak into a sibling member's same-named local, so that member's own
/// null test survives instead of folding to `true` and unwrapping `None`.
#[test]
fn member_narrowing_scope() -> TestResult {
    verify_node_parity("member_narrowing_scope")
}

/// `String(x)` on a value of a real Rust generic `T`, and a concrete Date
/// erased by a callee: the erased view reads and writes the same date.
#[test]
fn generic_string_and_shared_date() -> TestResult {
    verify_node_parity("generic_string_and_shared_date")
}

/// ts-pattern's library shapes: a 3-tuple record field recovered from an
/// erased value, a generic union with a generic class arm, a closure nested in
/// a closure over a multiply-assigned local, and `new` of a later-declared
/// class whose parameter only a `never` argument instantiates.
#[test]
fn ts_pattern_lib_shapes() -> TestResult {
    verify_node_parity("ts_pattern_lib_shapes")
}

/// ts-pattern's test-suite shapes: erased values typed by the checker at their
/// bindings, `new Set(..)` built at its parameter's type, a record keyed by a
/// block-local symbol, a member-less abstract class as its subclasses' union,
/// and classes that hold their own class.
#[test]
fn ts_pattern_test_shapes() -> TestResult {
    verify_node_parity("ts_pattern_test_shapes")?;
    // Type preservation for the symbol-keyed record: it is a property-key
    // record of property-key records of strings, as the literal is, not an
    // erased record (`SmeltRecord<JsString, SmeltUnknown>`).
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/node-parity");
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::copy(
        fixtures.join("ts_pattern_test_shapes.ts"),
        root.join("src/main.ts"),
    )?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"ts-pattern-shapes\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n[output]\ntarget = \"./dist\"\nbuild = false\n",
    )?;
    smelt_in(root, &["build"])?;
    let mut source = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        source.push_str(&fs::read_to_string(entry?.path())?);
    }
    ensure(
        source.contains("closure_arg_0: SmeltPropKeyRecord<SmeltPropKeyRecord<JsString>>"),
        "the symbol-keyed record lost its value types",
    )
}

/// A class value carries its static methods, so a computed-key call finds
/// them, and a block-local class used as a value constructs that class, not
/// another class of the same name.
#[test]
fn class_value_statics() -> TestResult {
    verify_node_parity("class_value_statics")
}

/// A lone surrogate survives in a template literal part and an object key, and
/// `isWellFormed`/`toWellFormed` read a string's UTF-16 code units.
#[test]
fn well_formed_strings_and_lone_keys() -> TestResult {
    verify_node_parity("well_formed_strings_and_lone_keys")
}

/// At the host edges a lone surrogate is what JavaScript makes of it:
/// `JSON.stringify` writes its escape, `JSON.parse` reads it back, the URI
/// encoders throw a `URIError`, and `TextEncoder` writes U+FFFD.
#[test]
fn lone_surrogates_at_host_edges() -> TestResult {
    verify_node_parity("lone_surrogates_at_host_edges")
}

/// A class instance erased with `as any` and read back as an interface keeps
/// its methods that take a callback or a rest parameter.
#[test]
fn erased_instance_callback_methods() -> TestResult {
    verify_node_parity("erased_instance_callback_methods")
}

/// A computed string-literal key is the string's value: escapes decoded and a
/// lone surrogate kept, not the source text between the quotes.
#[test]
fn computed_string_literal_keys() -> TestResult {
    verify_node_parity("computed_string_literal_keys")
}

/// A static string key holding a lone surrogate stays exact wherever it is a
/// runtime key, and two different halves stay two different keys.
#[test]
fn lone_surrogate_static_keys() -> TestResult {
    verify_node_parity("lone_surrogate_static_keys")
}

/// `==`/`!=` are JavaScript's loose equality: nullish, numeric-string,
/// boolean, bigint and object-identity cases.
#[test]
fn loose_equality() -> TestResult {
    verify_node_parity("loose_equality")
}

/// `Pick`, `Omit`, `Partial` and `Required` whose keys the annotation fixes
/// enumerate exactly the properties the utility leaves.
#[test]
fn utility_record_subsets_literal() -> TestResult {
    verify_node_parity("utility_record_subsets_literal")
}

/// A generic `K extends keyof T` is fixed per call, and each call's result
/// enumerates exactly the keys it asked for.
#[test]
fn utility_record_subsets_generic() -> TestResult {
    verify_node_parity("utility_record_subsets_generic")
}

/// Loops inside closure bodies are structured, never a run-time abort: a
/// do/while back edge on the false arm, an inner loop left into the outer one, a
/// `try` around a throwing call in one arm of a rejoining branch, `while (true)`
/// left by `break` and `return`, and a loop headed by a call.
#[test]
fn closure_control_flow() -> TestResult {
    verify_node_parity("closure_control_flow")
}

/// Program locals spelled like the emitter's template temporaries (`value`,
/// `item`, `key`, `index`, `acc`, `smelt_needle`, a generator parameter `co`)
/// keep their own values inside the code those templates splice.
#[test]
fn template_hygiene() -> TestResult {
    verify_node_parity("template_hygiene")
}

/// An erased object projected into an interface reads each property by its
/// source spelling (`isHTML`, `URL`, `snake_case`, `camelCase`), never by a
/// name rebuilt from the Rust field.
#[test]
fn record_source_keys() -> TestResult {
    verify_node_parity("record_source_keys")
}

/// Loosely binding operands spliced into postfix and operator positions keep
/// their meaning: `"" + -n`, `(-n).toFixed(1)`, `s + -1`.
#[test]
fn splice_precedence() -> TestResult {
    verify_node_parity("splice_precedence")
}

/// Labelled statements: `continue outer` and `break search` across nested
/// loops, a labelled block left by `break`, a `break outer` that runs the
/// `finally` it crosses, labels inside a closure body, and a labelled
/// `do`/`while` at module level.
#[test]
fn labeled_statements() -> TestResult {
    verify_node_parity("labeled_statements")
}

/// A loop whose condition is a call or `await`: the loop header ends in a
/// terminator rather than a statement, so the edge back to the decision is not
/// a `Goto`. Bodies with ternaries, joins, `break`/`continue` and `try`; `for`
/// and `do`/`while`; the same loops nested in an outer loop, an `if` and a
/// closure. Call counts pin that nothing in the body runs twice.
#[test]
fn loop_call_condition() -> TestResult {
    verify_node_parity("loop_call_condition")
}

/// `new Set(set)`, `new Map(map)`, `new Map(pairs)` and
/// `new Map(Object.entries(o))` build NEW collections in iteration order: a
/// later write to either side leaves the other unchanged, a repeated key keeps
/// its first position and its last value, and an erased iterable of pairs is
/// read entry by entry.
#[test]
fn collection_copies() -> TestResult {
    verify_node_parity("collection_copies")
}

/// `RegExp(pattern, flags)` called without `new` constructs the same regular
/// expression `new RegExp(pattern, flags)` does, for a literal or a computed
/// pattern.
#[test]
fn regexp_call_form() -> TestResult {
    verify_node_parity("regexp_call_form")
}

/// An erased value converted to a declared record keeps its present fields,
/// leaves optional ones absent, tolerates a guarded `null`, and does not treat
/// a member an instance's view omits as missing.
#[test]
fn erased_record_conversion() -> TestResult {
    verify_node_parity("erased_record_conversion")
}

/// `??` on an optional chain over an erased receiver tests the value read.
#[test]
fn erased_optional_coalesce() -> TestResult {
    verify_node_parity("erased_optional_coalesce")
}

/// A record parameter the body tests for presence accepts an absent value.
#[test]
fn presence_tested_parameters() -> TestResult {
    verify_node_parity("presence_tested_parameters")
}

/// A thrown value reaches its catch whole, through every channel.
#[test]
fn thrown_payload() -> TestResult {
    verify_node_parity("thrown_payload")
}

/// `typeof` of a statically undefined value is `"undefined"`, not `"object"`.
#[test]
fn typeof_undefined() -> TestResult {
    verify_node_parity("typeof_undefined")
}

/// `<=` and `>=` hold at equality for numbers, strings, bigints, dates and erased values.
#[test]
fn ordering_boundaries() -> TestResult {
    verify_node_parity("ordering_boundaries")
}

/// `indexOf` and friends on an erased receiver start at the first element.
#[test]
fn erased_array_search() -> TestResult {
    verify_node_parity("erased_array_search")
}

/// A nested function reads before its declaration; computed member keys name fields.
#[test]
fn forward_references_and_computed_keys() -> TestResult {
    verify_node_parity("forward_references_and_computed_keys")
}

/// Index signatures, erased projection, statics, typed arrays and inheritance behave like TypeScript.
#[test]
fn classes_behaviour() -> TestResult {
    verify_node_parity("classes_behaviour")
}

/// Generic classes, functions and callbacks behave like TypeScript on every lift and demotion path.
#[test]
fn generics_behaviour() -> TestResult {
    verify_node_parity("generics_behaviour")
}

/// `Array.prototype.pop` on an empty array answers `undefined` for every element shape.
#[test]
fn list_pop_empty() -> TestResult {
    verify_node_parity("list_pop_empty")
}

/// Loop shapes, JSON, host values and global numeric helpers behave like Node.
#[test]
fn json_loops_and_globals() -> TestResult {
    verify_node_parity("json_loops_and_globals")
}

/// Callbacks, optional parameters and the builtin calls reached from them behave like TypeScript.
#[test]
fn callback_options_behaviour() -> TestResult {
    verify_node_parity("callback_options_behaviour")
}

/// Closures as values capture their bindings and adapt between call shapes like TypeScript.
#[test]
fn closure_values_behaviour() -> TestResult {
    verify_node_parity("closure_values_behaviour")
}

/// Promises, erased calls and the object runtime they reach behave like TypeScript.
#[test]
fn promise_calls_behaviour() -> TestResult {
    verify_node_parity("promise_calls_behaviour")
}

/// Array methods: every callback spelling, search bounds, slices, in-place mutators and copies.
#[test]
fn collection_array_methods() -> TestResult {
    verify_node_parity("collection_array_methods")
}

/// Record and object key order, reads, writes, rest copies, and interface literals.
#[test]
fn collection_objects_records() -> TestResult {
    verify_node_parity("collection_objects_records")
}

/// Map and Set methods, projections, optional receivers, erasure and reference identity.
#[test]
fn collection_maps_sets() -> TestResult {
    verify_node_parity("collection_maps_sets")
}

/// Typed-array codecs, view/storage construction, own keys and tags; the `arguments` object.
#[test]
fn collection_typed_arrays() -> TestResult {
    verify_node_parity("collection_typed_arrays")
}

/// Callable objects, overload slots, rest adapters, tuples at erased boundaries.
#[test]
fn collection_callable_objects() -> TestResult {
    verify_node_parity("collection_callable_objects")
}

/// Option bags, module slots, IIFE typing, generators, optional sort, statics and boxed primitives run as Node runs them.
#[test]
fn emission_rules_behaviour() -> TestResult {
    verify_node_parity("emission_rules_behaviour")
}

/// Inheritance, reference classes, throwing getters, reassigned methods and dynamic host constructor reads run as Node runs them.
/// A method whose callback parameter is optional (a default, or `?`) is
/// callable through an erased instance: an absent argument takes the
/// default, a given callback is called.
#[test]
fn erased_optional_callback_method() -> TestResult {
    verify_node_parity("erased_optional_callback_method")
}

/// `Object.assign` onto a value of a type parameter writes into that object
/// and returns it, keeping its own symbol-keyed method and gaining the
/// sources' members.
#[test]
fn object_assign_erased_target() -> TestResult {
    verify_node_parity("object_assign_erased_target")
}

/// A parameter the call passes no argument for is `undefined`, never `null`,
/// through a function, a constructor, a static method and a tuple rest element.
#[test]
fn missing_argument_undefined() -> TestResult {
    verify_node_parity("missing_argument_undefined")
}

/// `args.length` of a rest tuple with optional elements counts the arguments
/// given, through erased and typed optional elements.
#[test]
fn optional_tuple_rest_length() -> TestResult {
    verify_node_parity("optional_tuple_rest_length")
}

#[test]
fn class_inheritance_behaviour() -> TestResult {
    verify_node_parity("class_inheritance_behaviour")
}

/// `const view = erased as T` keeps the erased value: a type guard that asserts
/// and then probes a symbol-keyed member answers for arrays, strings, numbers,
/// `null`, booleans and plain objects; a truthiness test and a bracket read
/// probe the value; a dot read and a read of the view as a value use a `T`.
#[test]
fn assertion_view_probes() -> TestResult {
    verify_node_parity("assertion_view_probes")
}

/// A `value is Object` guard leaves an erased value erased, as tsc does, so
/// `Array.isArray` still answers for it, while a union still narrows; in a
/// condition, the right operand of `&&` is tested for truthiness.
#[test]
fn object_guard_and_condition() -> TestResult {
    verify_node_parity("object_guard_and_condition")
}

/// `Object.assign` copies own enumerable symbol-keyed properties too: a
/// symbol flag, and `Symbol.iterator`, which makes the target iterable.
#[test]
fn assign_symbol_keys() -> TestResult {
    verify_node_parity("assign_symbol_keys")
}

/// `new Set(xs)` and `new Map(xs)` with an absent optional iterable build an
/// empty collection at the present type, as JavaScript's constructors do.
#[test]
fn absent_iterable_constructors() -> TestResult {
    verify_node_parity("absent_iterable_constructors")
}

/// `toFixed`, `toPrecision`, `toExponential` and `toString(radix)` round and
/// throw as Node does, typed and through an erased receiver's member read.
#[test]
fn number_format_methods() -> TestResult {
    verify_node_parity("number_format_methods")
}

/// A reducer that can throw folds plain accumulator values, and its throw
/// reaches the enclosing `catch`, with and without an initial value.
#[test]
fn throwing_reduce_callback() -> TestResult {
    verify_node_parity("throwing_reduce_callback")
}

/// An abstract generic parser hierarchy (date-fns' `parse` shape): inherited
/// and overridden members reach the base's `run` through their slots, a
/// mutating setter slot takes its state `&mut`, and a callback the base hands
/// to a generic class at its own `Value` passes without an erased adapter.
#[test]
fn abstract_parser_slots() -> TestResult {
    verify_node_parity("abstract_parser_slots")
}

/// A prototype method read off a number, string or array takes its receiver
/// from the call: detached it throws Node's `TypeError`, while member calls,
/// `.call`, `.apply` and `.bind` supply the receiver; typed and erased
/// receivers read the same unbound function.
#[test]
fn unbound_method_values() -> TestResult {
    verify_node_parity("unbound_method_values")
}

/// A manifest's entries run as a JavaScript program evaluates its module
/// graph: every module body once, a module's imports before it, several
/// entries in the order the manifest lists them. An entry not named
/// `main.ts` (or nested under a source root) is the program too; it once
/// produced an empty `main` that ran nothing.
#[test]
fn entry_modules_run_in_order() -> TestResult {
    let fixtures =
        workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/node-parity/entry_modules");
    let read = |path: &str| fs::read_to_string(fixtures.join(path));
    let (shared, app, second, start, both) = (
        read("shared.ts")?,
        read("app.ts")?,
        read("second.ts")?,
        read("app/start.ts")?,
        read("both.ts")?,
    );
    let files = [
        ("shared.ts", shared.as_str()),
        ("app.ts", app.as_str()),
        ("second.ts", second.as_str()),
        ("app/start.ts", start.as_str()),
        ("both.ts", both.as_str()),
    ];
    // (manifest `[sources]`, what Node prints for the same program)
    let cases = [
        (
            "entries = [\"src/app.ts\"]",
            "app.ts",
            "shared body\napp body 1\n",
        ),
        (
            "roots = [\"src\"]\nentries = [\"src/app/start.ts\"]",
            "app/start.ts",
            "shared body\nstart body 1\n",
        ),
        (
            "entries = [\"src/app.ts\", \"src/second.ts\"]",
            "both.ts",
            "shared body\napp body 1\nsecond body 1\n",
        ),
    ];
    for (sources, node_entry, expected) in cases {
        ensure_node_agrees(&files, node_entry, expected, "entry_modules_run_in_order")?;
        let project = TempProject::new()?;
        let root = project.path();
        fs::create_dir_all(root.join("src/app"))?;
        for (file, text) in &files {
            if *file != "both.ts" {
                fs::write(root.join("src").join(file), text)?;
            }
        }
        fs::write(
            root.join("Smelt.toml"),
            format!(
                "[project]\nname = \"entries\"\nversion = \"0.1.0\"\n\n[sources]\n{sources}\n\n[output]\ntarget = \"./dist\"\ncrate-name = \"entries\"\nbuild = false\n"
            ),
        )?;
        smelt_in(root, &["build"])?;
        let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
        ensure_eq(
            &stdout,
            &expected.to_owned(),
            format!("entries `{sources}`: generated Rust stdout"),
        )?;
    }
    Ok(())
}

/// A module other than the single entry that awaits at top level cannot run
/// from `main`: the build says so instead of emitting a `main` that skips it.
#[test]
fn awaiting_imported_module_is_a_build_error() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/slow.ts"),
        "await Promise.resolve();\nconsole.log(\"slow body\");\nexport const ready = true;\n",
    )?;
    fs::write(
        root.join("src/app.ts"),
        "import { ready } from \"./slow\";\nconsole.log(\"app body\", ready);\n",
    )?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"entries\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/app.ts\"]\n\n[output]\ntarget = \"./dist\"\nbuild = false\n",
    )?;
    let Err(error) = smelt_in(root, &["build"]) else {
        return Err("a build whose imported module awaits at top level succeeded".into());
    };
    ensure(
        error
            .to_string()
            .contains("awaits at top level but is not the program's single entry"),
        format!("the build failed for another reason: {error}"),
    )
}

/// A Python entry runs an imported module's body at its first import, before
/// its own code, as `CPython` does (`helper body` then `app body 2`, checked
/// against `python3 app.py`).
#[test]
fn python_entry_runs_imported_module_body() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root)?;
    fs::write(
        root.join("helper.py"),
        "print(\"helper body\")\nVALUE = 2\n",
    )?;
    fs::write(
        root.join("app.py"),
        "import helper\nprint(\"app body\", helper.VALUE)\n",
    )?;
    fs::write(
        root.join("Smelt.toml"),
        "[project]\nname = \"pymods\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"app.py\"]\n\n[output]\ntarget = \"./dist\"\ncrate-name = \"pymods\"\nbuild = false\n",
    )?;
    smelt_in(root, &["build"])?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"helper body\napp body 2\n".to_owned(),
        "a Python entry's import ran its module body",
    )
}

/// An optional call on an erased receiver short-circuits as `?.` does (a
/// nullish link ends the chain and skips the arguments), and a computed member
/// call reads the member its literal or dynamic key names.
#[test]
fn optional_erased_calls() -> TestResult {
    verify_node_parity("optional_erased_calls")
}
