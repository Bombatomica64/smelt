//! Generated Rust for lists, maps, records and iteration.

use super::*;

/// A concrete numeric list cannot share storage with a list of tagged values.
#[test]
fn concrete_list_erasure_still_converts_elements() {
    let source = source_for(include_str!("../../tests/fixtures/erased_generic_list.ts"));
    let body = emitted_function_body(&source, "fn concrete_array");
    assert!(
        body.contains("SmeltUnknown::Number("),
        "concrete numeric elements still need conversion into tagged values: {body}"
    );
    assert!(
        body.contains("collect::<Vec<_>>()"),
        "different element representations require a new buffer: {body}"
    );
    assert!(
        !body.contains(ERASED_GENERIC_ARRAY_ALIAS_FN),
        "a concrete list is not already erased, so it must not take the alias boundary: {body}"
    );
}

/// A source `unknown[]` erased into `unknown` keeps its direct wrap: only an
/// erased generic list leaving the erased-generic ABI takes the named alias
/// helper the SmeltUnknown report counts as a boundary. A program with no
/// generic list carries no helper.
#[test]
fn source_unknown_list_erasure_keeps_its_direct_wrap() {
    let source = source_for(
        r"
export function eraseUnknown(data: unknown[]): unknown {
  return data
}
",
    );
    let body = emitted_function_body(&source, "fn erase_unknown");
    assert!(
        body.contains("SmeltUnknown::Array(") && body.contains(".into())"),
        "an `unknown[]` already has the erased representation: {body}"
    );
    assert!(
        !body.contains("collect::<Vec<_>>()"),
        "an `unknown[]` must share its storage, not rebuild it: {body}"
    );
    assert!(!source.contains(ERASED_GENERIC_LIST_ALIAS_FN), "{source}");
    assert!(!source.contains(ERASED_GENERIC_ARRAY_ALIAS_FN), "{source}");
}

#[test]
fn injects_url_dependency_for_url_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Url)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("url = \"2\""));
}

#[test]
fn emits_total_unknown_primitive_and_object_extraction() {
    let source = source_for(
        r"
declare const value: unknown;
const text: string = value as string;
const count: number = value as number;
const flag: boolean = value as boolean;
const bag: Record<string, unknown> = value as Record<string, unknown>;
",
    );

    assert!(
        source.contains("SmeltUnknown::Null => String::new()"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false"),
        "{source}"
    );
    // The numeric extraction of an erased string is the shared
    // `StringToNumber` helper rather than a bare `parse::<f64>()`.
    assert!(
        source.contains("SmeltUnknown::String(value) => smelt_js_string_to_number(&value)"),
        "{source}"
    );
    assert!(source.contains("_ => SmeltRecord::new()"), "{source}");
}

#[test]
fn emits_array_iterator_next_as_typed_result() {
    let source = source_for(
        r#"
export function firstEntry(values: string[]): string | undefined {
  const iterator = values.entries();
  const result = iterator.next();
  if ("done" in result && result.done) {
    return undefined;
  }
  return result.value[1];
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn first_entry"]), r#"fn first_entry(values: SmeltList<JsString>) -> Option<JsString> {
let mut _smelt_tmp_5: bool;
let _smelt_tmp_6: bool;
let _smelt_tmp_7: Option<(i64, JsString)>;
let _smelt_tmp_8: (i64, JsString);
let _smelt_tmp_9: JsString;
let mut iterator: SmeltList<(i64, JsString)> = Into::<SmeltList<_>>::into(values.borrow().iter().cloned().enumerate().map(|(smelt_idx, smelt_item)| (smelt_idx as i64, smelt_item)).collect::<Vec<_>>());
let result: SmeltGeneratorResult<(i64, JsString), ()> = if iterator.is_empty() { SmeltGeneratorResult::Complete(()) } else { SmeltGeneratorResult::Yielded(iterator.borrow_mut().remove(0)) };
_smelt_tmp_6 = matches!(result.clone(), SmeltGeneratorResult::Complete(_));
_smelt_tmp_5 = _smelt_tmp_6;
if _smelt_tmp_5 {
return None::<JsString>;
} else {
_smelt_tmp_7 = match result { SmeltGeneratorResult::Yielded(smelt_value) => Some(smelt_value), SmeltGeneratorResult::Complete(smelt_value) => None::<(i64, JsString)> };
_smelt_tmp_8 = _smelt_tmp_7.clone().expect("optional value was absent after narrowing");
_smelt_tmp_9 = _smelt_tmp_8.1.clone();
return Some(_smelt_tmp_9);
}
}"#);
}

#[test]
fn emits_object_literal_as_destination_interface_struct() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

function format(options: Options): string {
  return options.width ?? "full";
}

export function run(): string {
  return format({ width: "short" });
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn run"]), r#"fn run() -> JsString {
let _smelt_tmp_0: Options = Options { width: Some(JsString::from("short")) };
let _smelt_tmp_1: JsString = format(_smelt_tmp_0);
return _smelt_tmp_1;
}"#);
}

#[test]
fn matches_object_literal_keys_to_interface_fields_by_source_spelling() {
    // A JavaScript property key is case-sensitive and never case-folded; an
    // interface FIELD has two spellings, the source name and the Rust-safe
    // rendering the struct field carries. Matching the literal's keys against
    // the RENDERED spelling silently dropped every field whose source name is
    // not already a valid Rust name: the field looked absent, took the optional
    // default, and the program printed `undefined` where Node prints the value.
    let source = source_for(
        r#"
interface Shape {
  plain?: number;
  camelCase?: string;
  snake_case?: string;
}

export function build(): Shape {
  return { plain: 1, camelCase: "a", snake_case: "b" };
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn build"]), r#"fn build() -> Shape {
let _smelt_tmp_0: Shape = Shape { plain: Some(1.0), camel_case: Some(JsString::from("a")), snake_case: Some(JsString::from("b")) };
return _smelt_tmp_0;
}"#);
}

#[test]
fn builds_a_nested_optional_interface_literal_as_a_struct() {
    // The nested literal receives the field's type as its own hint, so both
    // levels are built directly. Without it the inner literal lowered as a
    // `Dict`, which made the OUTER literal unbuildable as a struct too, and
    // both ends went through the erased `SmeltRecord` reconstruction.
    let source = source_for(
        r#"
interface Inner {
  camelCase?: string;
  count?: number;
}

interface Outer {
  innerShape?: Inner;
}

export function build(): Outer {
  return { innerShape: { camelCase: "deep", count: 2 } };
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn build"]), r#"fn build() -> Outer {
let _smelt_tmp_0: Inner = Inner { camel_case: Some(JsString::from("deep")), count: Some(2.0) };
let _smelt_tmp_1: Outer = Outer { inner_shape: Some(_smelt_tmp_0) };
return _smelt_tmp_1;
}"#);
}

#[test]
fn adapts_object_literal_for_interface_callback_field_argument() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

interface FormatLong {
  date: (options: Options) => string;
}

export function run(formatLong: FormatLong): string {
  return formatLong.date({ width: "short" });
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn run"]), r#"fn run(format_long: FormatLong) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: SmeltRecord<JsString, JsString> = SmeltRecord::from([(JsString::from("width"), JsString::from("short"))]);
let _smelt_tmp_2: JsString = (format_long.date.clone())({ let smelt_record_map = _smelt_tmp_1.clone(); Options { width: smelt_record_map.get_text("width").cloned().map(|smelt_value| smelt_value) } });
return Ok(_smelt_tmp_2);
}"#);
}

#[test]
fn emits_empty_object_literal_as_optional_interface_struct_defaults() {
    let source = source_for(
        r"
interface Duration {
  years?: number;
  months?: number;
}

export function make(): Duration {
  return {};
}
",
    );
    assert_eq!(rust_items(&source, &["fn make"]), r"fn make() -> Duration {
let _smelt_tmp_0: Duration = Duration { years: None::<f64>, months: None::<f64> };
return _smelt_tmp_0;
}");
}

#[test]
fn passes_a_mutated_structural_record_as_a_shared_handle() {
    // A record whose field is written after construction is a JavaScript
    // reference value, so it lowers to the reference-record handle newtype (see
    // `classify::reference_classes`) rather than being threaded through Rust's
    // `&mut` ABI. The callee's write goes through the shared cell, so the caller
    // observes it while the value is still passed by value.
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

function setEra(flags: Flags, value: number): void {
  flags.era = value;
}

function readEra(): number {
  const flags: Flags = {};
  setEra(flags, 1);
  return flags.era!;
}
",
    );
    assert_eq!(rust_items(&source, &["struct Flags", "sig set_era"]), r"struct Flags(::std::rc::Rc<::std::cell::RefCell<FlagsInner>>);

fn set_era(mut flags: Flags, value: f64) -> ()");
}

#[test]
fn emits_mutable_function_field_parameters_for_structural_objects() {
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

interface Setter {
  set: (flags: Flags) => number | [number, Flags];
}

function run(setter: Setter): number {
  const flags: Flags = {};
  setter.set(flags);
  return flags.era!;
}
",
    );
    assert!(
        source.contains("set: ::std::rc::Rc<dyn Fn(&mut Flags) -> SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("(setter.set.clone())(&mut flags)"),
        "{source}"
    );
}

#[test]
fn emits_unknown_record_literals_with_tagged_values() {
    let source = source_for(
        r#"
const value: Record<string, unknown> = { done: false, name: "skip" };
"#,
    );
    assert_eq!(rust_items(&source, &["fn main"]), r#"fn main() {
let value: SmeltRecord<JsString, SmeltUnknown> = SmeltRecord::from([(JsString::from("done"), SmeltUnknown::Bool(false)), (JsString::from("name"), SmeltUnknown::String("skip".into()))]);
return;
}"#);
}

#[test]
fn emits_list_operand_item_coercion() {
    let source = source_for(
        r"
function widen(values: string[]): unknown[] {
  return values;
}
",
    );
    assert_eq!(rust_items(&source, &["fn widen"]), r"fn widen(values: SmeltList<JsString>) -> SmeltList<SmeltUnknown> {
return { let smelt_l: SmeltList<_> = values.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>()) };
}");
}

#[test]
fn emits_string_chars_into_unknown_list_destination() {
    let source = source_for(
        r"
function chars(value: string): unknown[] {
  return [...value];
}
",
    );
    assert_eq!(rust_items(&source, &["fn chars"]), r"fn chars(value: JsString) -> SmeltList<SmeltUnknown> {
let _smelt_tmp_1: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(value.clone().code_point_strings().into_iter().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>());
return _smelt_tmp_1;
}");
}

#[test]
fn emits_tuple_into_unknown_runtime_conversion() {
    let source = source_for(
        r"
function pair(value: unknown): unknown {
  return [value, value] as [unknown, unknown];
}
",
    );

    assert!(
        source
            .contains("impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)"),
        "{source}"
    );
}

#[test]
fn ambient_binding_reads_the_global_object_member() {
    // An AMBIENT binding has no initializer: the HOST provides it, as a member
    // of the global object. Its read is that member (a dynamic boundary), not
    // a fabricated default record of the declared type. (A binding WITH an initializer, `let defaultOptions:
    // DefaultOptions = {}`, is a module slot holding its one evaluated value
    // instead — see `module_bound_intersection_alias_reads_its_slot`.)
    let source = source_for(
        r"
type A = { locale?: unknown };
type B = { weekStartsOn?: number };
type DefaultOptions = A & B;

declare const defaultOptions: DefaultOptions;

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
",
    );
    assert_eq!(rust_items(&source, &["fn get_default_options"]), r#"fn get_default_options() -> SmeltUnknown {
let _smelt_tmp_0: SmeltUnknown = smelt_global_object();
let _smelt_tmp_1: SmeltUnknown = smelt_get_unknown_field(&_smelt_tmp_0.clone(), "defaultOptions").clone();
return _smelt_tmp_1;
}"#);
}

#[test]
fn emits_object_symbol_iterator_generator_as_erased_iterable() {
    let source = source_for(
        r"
function count(items: Iterable<unknown>): number {
  const erased = items as unknown;
  return [...(erased as Iterable<unknown>)].length;
}

const result = count({
  *[Symbol.iterator]() {
    yield 0;
    yield 1;
  },
});
",
    );

    assert!(source.contains("\"__smelt_symbol_iterator\""), "{source}");
    assert!(
        source.contains("iterator(vec![]).unwrap_or(SmeltUnknown::Null)"),
        "{source}"
    );
    assert!(
        !source.contains("else { panic!(\"unknown is not array\") })"),
        "{source}"
    );
}

#[test]
fn adapts_rest_callback_without_flattening_list_arguments() {
    let source = source_for(
        r"
function purry<T>(
  callback: (...args: unknown[]) => unknown,
): (data: readonly T[], compare: (left: T, right: T) => number) => unknown {
  const n = 2;
  return (data, compare) => callback(data, compare, n);
}

function wrap<T>(
  func: (data: readonly T[], compare: (left: T, right: T) => number, n: number) => unknown,
): unknown {
  const n = 2;
  return purry((...args) => func(...args, n));
}
",
    );

    // Both fixed parameters are read out of the packed rest vector by index
    // (`__smelt_spread.get({..index..})`). The callable-typed parameter's read
    // is hoisted into a `let smelt_source_value = closure_arg_0.get(..)` binding
    // by the callable-object narrowing, so assert on the shared `.get(` read.
    //
    // `data: readonly T[]` is extracted out of the packed rest vector, and `T`
    // is not a Rust generic in this emission scope, so its elements are already
    // `SmeltUnknown`: the extraction re-wraps the SAME array rather than
    // rebuilding its element vector (see `erased_to_list_expr`). That is what
    // keeps a `purry`-style runtime dispatcher from paying an O(n) copy on every
    // crossing, and it is what makes the array a callback receives BE the array
    // being iterated.
    assert!(
        source.contains("__smelt_spread.borrow().get(")
            && source
                .contains("SmeltUnknown::Array(smelt_values) => SmeltList::with_storage(smelt_values.id,"),
        "fixed callback spread calls should read the first fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.matches("__smelt_spread.borrow().get(").count() >= 2,
        "fixed callback spread calls should read the second fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.contains("}, n)"),
        "fixed callback spread calls should keep trailing scalar arguments after spread expansion: {source}"
    );
}

#[test]
fn emits_tuple_element_push_as_concrete_tuple_value() {
    // Pushing a bare `[value, key]` literal into an `Array<[number, string]>`
    // emits a real `(f64, String)` tuple value rather than widening the pushed
    // item to a `SmeltUnknown` list that could never re-type into the tuple.
    let source = source_for(
        r"
export function collect(value: number, key: string): Array<[number, string]> {
  const result: Array<[number, string]> = [];
  result.push([value, key]);
  return result;
}
",
    );
    assert_eq!(rust_items(&source, &["fn collect"]), r"fn collect(value: f64, key: JsString) -> SmeltList<(f64, JsString)> {
let mut result: SmeltList<(f64, JsString)> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<(f64, JsString)> = vec![]; smelt_list_items }));
let _smelt_tmp_4: (f64, JsString) = (value, key.clone());
let _smelt_tmp_5: f64 = { let smelt_push_item = _smelt_tmp_4; result.borrow_mut().push(smelt_push_item); result.len() as f64 };
return result;
}");
}

/// `String.prototype.match` emits the typed `Option<SmeltMatch>` value and an
/// array method on it reads the match's array view, with no erasure.
#[test]
fn emits_string_match_as_typed_match_with_array_view() {
    let source = source_for(
        r"
function marker(path: string): number {
  const m = path.match(/^(?:a()|b())$/);
  if (!m) return -1;
  return m.indexOf('', 1);
}
",
    );
    assert_eq!(rust_items(&source, &["fn marker"]), r#"fn marker(path: JsString) -> f64 {
let _smelt_tmp_5: f64;
let _smelt_tmp_6: SmeltMatch;
let _smelt_tmp_7: SmeltList<Option<JsString>>;
let _smelt_tmp_8: f64;
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("^(?:a()|b())$"), JsString::new());
let m: Option<SmeltMatch> = _smelt_tmp_2.match_string(&path.clone());
let _smelt_tmp_4: bool = !(m.clone().is_some());
if _smelt_tmp_4 {
_smelt_tmp_5 = -1.0;
return _smelt_tmp_5;
} else {
_smelt_tmp_6 = m.clone().expect("optional value was absent after narrowing");
_smelt_tmp_7 = Into::<SmeltList<_>>::into((_smelt_tmp_6).to_array_view());
_smelt_tmp_8 = { let smelt_needle = Some(JsString::new()); let smelt_predicate = |smelt_item: &Option<JsString>| *smelt_item == smelt_needle; let smelt_list = &_smelt_tmp_7.borrow(); let smelt_len = smelt_list.len() as i64; let smelt_raw = 1.0 as i64; let smelt_start = if smelt_raw < 0 { (smelt_len + smelt_raw).max(0) } else { smelt_raw } as usize; smelt_list.iter().enumerate().skip(smelt_start).find(|(_, smelt_item)| smelt_predicate(*smelt_item)).map_or(-1.0, |(smelt_idx, _)| smelt_idx as f64) };
return _smelt_tmp_8;
}
}"#);
}

/// A match passed where `string[]` is expected is converted through its array
/// view (an unmatched group reads `""` there), never through `SmeltUnknown`.
#[test]
fn coerces_match_to_string_list_through_array_view() {
    let source = source_for(
        r"
function takes(xs: string[]): number { return xs.length; }
function count(text: string): number {
  const m = text.match(/(a)(b)?/);
  return m ? takes(m) : 0;
}
",
    );
    assert_eq!(rust_items(&source, &["fn count"]), r#"fn count(text: JsString) -> f64 {
let mut _smelt_tmp_5: f64;
let _smelt_tmp_6: SmeltMatch;
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("(a)(b)?"), JsString::new());
let m: Option<SmeltMatch> = _smelt_tmp_2.match_string(&text.clone());
let _smelt_tmp_4: bool = !(m.clone().is_none());
if _smelt_tmp_4 {
_smelt_tmp_6 = m.clone().expect("optional value was absent after narrowing");
let _smelt_tmp_7: f64 = takes(SmeltList::new((_smelt_tmp_6).to_array_view().into_iter().map(|smelt_group| smelt_group.unwrap_or_default()).collect::<Vec<_>>()));
_smelt_tmp_5 = _smelt_tmp_7;
} else {
_smelt_tmp_5 = 0.0;
}
return _smelt_tmp_5;
}"#);
}

#[test]
fn coerces_rendered_list_values_to_tuple_destinations() {
    let source = source_for(
        r"
function invoke(
  values: unknown[],
  callback: (pair: [unknown, unknown]) => unknown,
): unknown {
  return callback(values);
}
",
    );
    assert_eq!(rust_items(&source, &["fn invoke"]), r"fn invoke(values: SmeltList<SmeltUnknown>, callback: &dyn Fn((SmeltUnknown, SmeltUnknown)) -> SmeltUnknown) -> SmeltUnknown {
let _smelt_tmp_2: SmeltUnknown = callback({ let smelt_tuple_values = values.clone().to_vec(); (smelt_tuple_values.get(0).cloned().unwrap_or(SmeltUnknown::Null), smelt_tuple_values.get(1).cloned().unwrap_or(SmeltUnknown::Null)) });
return _smelt_tmp_2;
}");
}

#[test]
fn flattens_python_dict_get_optional_value_type() {
    let source = source_for_py(
        r#"
from typing import Optional

def read(values: dict[str, Optional[int]]) -> Optional[int]:
    return values.get("x")
"#,
    );
    assert_eq!(rust_items(&source, &["fn read"]), r#"fn read(values: SmeltRecord<String, Option<i64>>) -> Option<i64> {
let _smelt_tmp_1: Option<i64> = values.get(&"x".to_owned()).flatten();
return _smelt_tmp_1;
}"#);
}

#[test]
fn flattens_optional_list_index_over_optional_items() {
    let source = source_for(
        r"
function read(values: Array<number | undefined>, index: number): number | undefined {
  return values?.[index];
}
",
    );
    assert_eq!(rust_items(&source, &["fn read"]), r"fn read(values: SmeltList<Option<f64>>, index: f64) -> Option<f64> {
let _smelt_tmp_2: Option<f64> = ({ let smelt_len = values.clone().len() as i64; let smelt_index = index as i64; let smelt_normalized = if smelt_index < 0 { smelt_len + smelt_index } else { smelt_index }; usize::try_from(smelt_normalized).ok() }).and_then(|smelt_index| values.clone().borrow().get(smelt_index).cloned().flatten());
return _smelt_tmp_2;
}");
}

/// The private host-marker array builder emits every registry marker plus the
/// runtime-owned abort/namespace markers, so the frontend construction path, the
/// `instanceof` codegen path, and this runtime filter share one source of truth.
#[test]
fn host_marker_registry_array_covers_registry_and_runtime_markers() {
    let text = smelt_runtime::source::item_source("smelt_object_has_host_marker")
        .expect("the host-marker helper is a runtime item");
    let array = text
        .split("has_internal_keys() && [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the helper spells the marker array");
    for marker in smelt_stdlib::host_object_markers() {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing registry marker `{marker}`: {array}"
        );
    }
    for marker in [
        "__smelt_abortcontroller",
        "__smelt_abortsignal",
        "__smelt_builtin_namespace",
        "__smelt_global_object",
    ] {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing runtime marker `{marker}`: {array}"
        );
    }
}

#[test]
fn lowers_imported_function_value_as_map_callback() {
    // The same support extends to value-returning array methods like `map`: a
    // bare imported function passed as the callback lowers to a mapping closure
    // rather than being rejected for not being an inline arrow.
    let source = source_for(
        r#"
import { deburr } from "./deburr";
function run(): unknown {
  const arr = ["a", "b"];
  return arr.map(deburr);
}
"#,
    );

    assert!(
        source.contains("smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)|"),
        "imported map callback should lower to a mapping closure\n{source}"
    );
    assert!(
        source.contains("(smelt_callback)(smelt_item.clone(), smelt_index as i64,"),
        "the closure body should call the resolved callback value\n{source}"
    );
}

#[test]
fn set_interval_typed_extra_args_use_wrapper_closure() {
    // `setInterval` shares the typed-wrapper path with `setTimeout`.
    let source = source_for(
        r#"
function tick(label: string, n: number): void {
  console.log(label);
}
setInterval(tick, 5, "tock", 2);
"#,
    );

    assert!(
        !source.contains("smelt_timer_args"),
        "typed interval extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
        "{source}"
    );
    assert!(source.contains("smelt_set_interval("), "{source}");
}

/// A seeded reduce borrows its receiver for iteration, so the array argument
/// supplied to a four-parameter callback must never be MOVED out of the
/// surrounding function or closure.
///
/// The callback takes that parameter by shared reference
/// (`callback_param_is_shared_reference`), so the fold body binds the same borrow
/// it is already iterating and nothing is copied.
#[test]
fn reduce_borrows_the_array_callback_argument_with_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const mapped = values.map(value => value);
  return mapped.reduce((acc, value, _index, array) => acc + value + array.length, 0);
}
",
    );

    assert!(source.contains("let smelt_array = &mapped;"), "{source}");
    assert!(!source.contains("let smelt_array = mapped;"), "{source}");
    assert!(!source.contains("let smelt_array = mapped.clone();"), "{source}");
}

/// Seedless reduce has the same ownership requirement after extracting its first
/// element: later iterations still borrow the receiver while invoking the callback,
/// so the array argument is that same borrow and is never moved.
#[test]
fn reduce_borrows_the_array_callback_argument_without_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const reversed = values.reverse();
  return reversed.reduce((acc, value, _index, array) => acc + value + array.length);
}
",
    );

    assert!(source.contains("let smelt_array = &reversed;"), "{source}");
    assert!(!source.contains("let smelt_array = reversed;"), "{source}");
    assert!(!source.contains("let smelt_array = reversed.clone();"), "{source}");
}

/// A module const whose initializer is an array spread (es-toolkit's
/// `arrayViews = [...typedArrays, 'DataView']` shape) inlines into a function
/// body as a concrete `SmeltList<String>` concat chain. The homogeneous
/// literal keeps its `String` item type.
#[test]
fn inlined_spread_const_emits_concrete_string_list_concat() {
    let source = source_for(
        r#"
const typedNames = ["Float32Array", "Int8Array", "Uint8Array"];
const viewNames = [...typedNames, "DataView"];
export function describeViews(): string {
  const names = viewNames;
  return names.join(",");
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn describe_views"]), r#"fn describe_views() -> JsString {
let names: SmeltList<JsString> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("Float32Array"), JsString::from("Int8Array"), JsString::from("Uint8Array"), JsString::from("DataView")]; smelt_list_items }));
let _smelt_tmp_2: JsString = JsString::join(names.borrow().iter(), &JsString::from(","));
return _smelt_tmp_2;
}"#);
}

#[test]
fn tuple_length_emits_constant_arity() {
    // A fixed-arity tuple has no Rust `.len()` method (E0599). Its JavaScript
    // `.length` is a compile-time constant, so the length rvalue must emit the
    // arity literal rather than a method call on the tuple.
    let source = source_for(
        r"
export function pairLength(pair: [string, number]): number {
  return pair.length;
}
",
    );
    assert_eq!(rust_items(&source, &["fn pair_length"]), r"fn pair_length(pair: (JsString, f64)) -> f64 {
let _smelt_tmp_1: f64 = 2 as f64;
return _smelt_tmp_1;
}");
}

#[test]
fn tuple_assertion_on_list_value_preserves_list_representation() {
    // A TypeScript tuple assertion applied to a
    // list value (`xs.filter(...) as [T]`) is type-level only. Materializing the
    // tuple would repackage the whole list into a 1-tuple `(SmeltUnknown,)` that
    // does not satisfy a `SmeltList` consumer (E0308). The list value and its
    // type must be preserved.
    let source = source_for(
        r"
export function pickList(xs: unknown[]): unknown[] {
  const ys = xs.filter(x => x != null) as [unknown];
  return ys;
}
",
    );
    assert_eq!(rust_items(&source, &["fn pick_list"]), r"fn pick_list(xs: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown> {
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: bool = matches!(closure_arg_0.clone(), SmeltUnknown::Null);
let _smelt_tmp_2: bool = matches!(closure_arg_0.clone(), SmeltUnknown::Undefined);
let _smelt_tmp_3: bool = _smelt_tmp_1 || _smelt_tmp_2;
let _smelt_tmp_4: bool = !(_smelt_tmp_3);
_smelt_tmp_4
});
let ys: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: bool = matches!(closure_arg_0.clone(), SmeltUnknown::Null);
let _smelt_tmp_2: bool = matches!(closure_arg_0.clone(), SmeltUnknown::Undefined);
let _smelt_tmp_3: bool = _smelt_tmp_1 || _smelt_tmp_2;
let _smelt_tmp_4: bool = !(_smelt_tmp_3);
_smelt_tmp_4
}); let smelt_array = xs.clone(); let smelt_value = smelt_array.borrow().iter().enumerate().filter_map(|(smelt_index, smelt_item)| if (smelt_callback)(smelt_item, smelt_index as i64, &smelt_array) { Some(smelt_item.clone()) } else { None }).collect::<Vec<_>>(); smelt_value });
return ys;
}");
}

/// Slicing a typed list into an erased (`SmeltUnknown`) destination must
/// materialize a real `SmeltList` and erase it, rather than leaking a bare
/// `Vec` into the `SmeltUnknown` slot (E0308). Mirrors es-toolkit's `ary`,
/// which caps a rest-argument list and forwards it through an erased value.
#[test]
fn list_slice_into_unknown_destination_materializes_smelt_list() {
    let source = source_for(
        r"
export function run(args: string[]): unknown {
  const capped: unknown = args.slice(0, 2);
  return capped;
}
",
    );

    // The slice is materialized as an identity-bearing array value and erased,
    // never assigned as a raw `Vec` collected straight into a `SmeltUnknown`.
    assert!(
        source.contains("SmeltArray::with_id") || source.contains("SmeltList::with_id"),
        "a slice into an erased destination must build an identity-bearing list\n{source}"
    );
    assert!(
        !source.contains(": SmeltUnknown = args.clone().iter().skip"),
        "the bare-Vec slice must not be assigned directly to a SmeltUnknown\n{source}"
    );
}

/// A spread call (`fn(...values)`) must route to the variadic overload rather
/// than a fixed-arity one: the spread's runtime length is unknown, so a
/// rest-less overload cannot claim it. Mirrors es-toolkit's
/// `cartesianProduct(...inputs)`, which must return `number[][]` and not a
/// list of 1-tuples-of-lists.
#[test]
fn spread_call_selects_variadic_overload() {
    let source = source_for(
        r"
export function cartesianProduct<T>(arr1: readonly T[]): Array<[T]>;
export function cartesianProduct<T, U>(arr1: readonly T[], arr2: readonly U[]): Array<[T, U]>;
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][];
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][] {
  return arrs as any;
}

export function run(inputs: number[][]): unknown {
  return cartesianProduct(...inputs);
}
",
    );
    assert_eq!(rust_items(&source, &["fn run"]), r#"fn run(inputs: SmeltList<SmeltList<f64>>) -> SmeltUnknown {
let _smelt_tmp_1: SmeltList<SmeltList<SmeltUnknown>> = Into::<SmeltList<_>>::into({ let smelt_l: SmeltList<_> = inputs.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| { let smelt_l: SmeltList<_> = smelt_value.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| SmeltUnknown::Number(smelt_value as f64)).collect::<Vec<_>>()) }).collect::<Vec<_>>()) });
let _smelt_tmp_2: SmeltList<SmeltList<f64>> = { let smelt_l: SmeltList<_> = cartesian_product(_smelt_tmp_1).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| { let smelt_l: SmeltList<_> = smelt_value.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| match smelt_value.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }).collect::<Vec<_>>()) }).collect::<Vec<_>>()) };
return { let smelt_l = _smelt_tmp_2; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| { let smelt_l = smelt_value; let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| SmeltUnknown::Number(smelt_value as f64)).collect::<Vec<_>>())) }).collect::<Vec<_>>())) };
}"#);
}

/// A read-only collection capture in a callback prelude is cloned into a plain,
/// non-`mut` binding: mutability follows `closure_capture_body_writes`.
#[test]
fn read_only_list_capture_emits_non_mut_clone() {
    let source = source_for(
        r"
export function useCapture(values: number[], extra: number[]): number[] {
  return values.map((n: number): number => n + extra.length);
}
",
    );
    assert_eq!(rust_items(&source, &["fn use_capture"]), r"fn use_capture(values: SmeltList<f64>, extra: SmeltList<f64>) -> SmeltList<f64> {
let _smelt_tmp_2 = ::std::rc::Rc::new({
let extra = extra.clone();
move |closure_arg_0: f64, _arg0: i64, _arg1: &SmeltList<f64>| {
let _smelt_tmp_2: f64 = extra.len() as f64;
let _smelt_tmp_3: f64 = closure_arg_0 + _smelt_tmp_2;
_smelt_tmp_3
}
});
let _smelt_tmp_3: SmeltList<f64> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new({
let extra = extra.clone();
move |closure_arg_0: f64, _arg0: i64, _arg1: &SmeltList<f64>| {
let _smelt_tmp_2: f64 = extra.len() as f64;
let _smelt_tmp_3: f64 = closure_arg_0 + _smelt_tmp_2;
_smelt_tmp_3
}
}); let smelt_array = values.clone(); let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
return _smelt_tmp_3;
}");
}

/// A captured collection that the enclosing
/// function still reassigns (so its source local is mutable) keeps a `mut`
/// clone binding. Under-approximating its mutability would not compile.
#[test]
fn mutable_source_list_capture_keeps_mut_clone() {
    let source = source_for(
        r"
export function f(items: number[]): number {
  let acc = items;
  acc = items;
  return [1].map((x: number): number => acc.length + x).length;
}
",
    );
    assert_eq!(rust_items(&source, &["fn f"]), r"fn f(items: SmeltList<f64>) -> f64 {
let mut acc: SmeltList<f64> = Into::<SmeltList<_>>::into(items.clone());
acc = Into::<SmeltList<_>>::into(items.clone());
let _smelt_tmp_2: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0]; smelt_list_items }));
let _smelt_tmp_3 = ::std::rc::Rc::new({
let mut acc = acc.clone();
move |closure_arg_0: f64, _arg0: i64, _arg1: &SmeltList<f64>| {
let _smelt_tmp_2: f64 = acc.len() as f64;
let _smelt_tmp_3: f64 = _smelt_tmp_2 + closure_arg_0;
_smelt_tmp_3
}
});
let _smelt_tmp_4: SmeltList<f64> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new({
let mut acc = acc.clone();
move |closure_arg_0: f64, _arg0: i64, _arg1: &SmeltList<f64>| {
let _smelt_tmp_2: f64 = acc.len() as f64;
let _smelt_tmp_3: f64 = _smelt_tmp_2 + closure_arg_0;
_smelt_tmp_3
}
}); let smelt_array = _smelt_tmp_2; let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
let _smelt_tmp_5: f64 = _smelt_tmp_4.len() as f64;
return _smelt_tmp_5;
}");
}

/// A captured collection that the closure
/// body mutates in place is threaded through shared `RefCell` storage (so the
/// mutation is caller-visible) rather than being silently emitted as a
/// read-only clone. Guards the write-detection path.
#[test]
fn written_list_capture_uses_shared_storage_not_plain_clone() {
    let source = source_for(
        r"
export function g(): number[] {
  const store: number[] = [];
  const add = (x: number): void => { store.push(x); };
  [1, 2, 3].forEach(add);
  return store;
}
",
    );
    assert_eq!(rust_items(&source, &["fn g"]), r"fn g() -> SmeltList<f64> {
let smelt_capture_store: ::std::rc::Rc<::std::cell::RefCell<SmeltList<f64>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![]; smelt_list_items }))));
let _smelt_tmp_3 = ::std::rc::Rc::new({
let smelt_capture_store = smelt_capture_store.clone();
move |closure_arg_0: f64| {
let _smelt_tmp_2: f64 = { let smelt_push_item = closure_arg_0; (*smelt_capture_store.borrow()).borrow_mut().push(smelt_push_item); (*smelt_capture_store.borrow()).len() as f64 };
()
}
});
let add = _smelt_tmp_3.clone();
let _smelt_tmp_4: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
let _smelt_tmp_5: () = { let smelt_callback = ::std::rc::Rc::new({
let smelt_capture_store = smelt_capture_store.clone();
move |closure_arg_0: f64| {
let _smelt_tmp_2: f64 = { let smelt_push_item = closure_arg_0; (*smelt_capture_store.borrow()).borrow_mut().push(smelt_push_item); (*smelt_capture_store.borrow()).len() as f64 };
()
}
}); _smelt_tmp_4.borrow().iter().enumerate().for_each(|(smelt_index, smelt_item)| { let _ = (smelt_callback)(smelt_item.clone()); }); () };
return ((*smelt_capture_store.borrow())).clone();
}");
}

#[test]
fn apply_on_a_callable_object_invokes_its_underlying_callable() {
    // A callable object erases to
    // `SmeltUnknown::Object` carrying a `__smelt_call` slot and has no own
    // `apply` property. `apply`/`call`/`bind`
    // are `Function.prototype` members of the underlying callable and must
    // resolve there when the object itself does not define them.
    let source = source_for(
        r"
export function applyTo(func: unknown, args: unknown[]): unknown {
  return (func as (...values: unknown[]) => unknown).apply(null, args);
}
",
    );
    assert!(
        source.contains("fn smelt_function_method("),
        "expected the Function.prototype method helper: {source}"
    );
    assert!(
        source.contains(r#"match map.get("__smelt_call")"#),
        "`smelt_function_method` must fall back to a callable object's \
         `__smelt_call` slot: {source}"
    );
}

/// An overload set whose signatures all return the same type is stored as ONE
/// concrete closure whose parameters accept every overload's arguments: a
/// position's type is the union of the overloads' types there, and a position
/// some overload omits or marks optional is `Option<_>`. That is the signature
/// a hand-written port stores (the TypeScript implementation signature), so
/// the slot is not erased and a call passes its arguments straight through.
#[test]
fn overload_set_sharing_a_result_is_one_concrete_slot() {
    let source = source_for(
        "interface Setter {
  (key: string, value: number): void;
  (key: string, value: string): void;
}
interface Header {
  (name: string): void;
  (name: string, value: string, append?: boolean): void;
}
class Store {
  seen: string[] = [];
  set: Setter = (key: string, value: number | string) => {
    this.seen.push(key + '=' + String(value));
  };
  header: Header = (name: string, value?: string, append?: boolean) => {
    this.seen.push(name + ':' + (value ?? '') + (append ? '+' : ''));
  };
}
const store = new Store();
store.set('a', 1);
store.set('b', 'two');
store.header('x');
store.header('y', 'z', true);
",
    );

    assert!(
        !source.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set sharing one result must not erase its slot:\n{source}"
    );
    assert!(
        source.contains(
            "__smelt_call: ::std::rc::Rc<dyn Fn(JsString, Option<JsString>, Option<bool>) -> ()>"
        ),
        "positions an overload omits or marks optional merge to Option:\n{source}"
    );
    assert!(
        !source.contains("smelt_callback.call(vec!["),
        "calls must reach the concrete slot without an erased adapter:\n{source}"
    );
}

#[test]
fn erased_object_field_list_alias_push_still_converts_concrete_elements() {
    // The other half of the same general rule: a `string[]` field extracts into
    // `SmeltList<String>`, which shares no representation with `SmeltArray`, so
    // there is nothing to alias and the elements genuinely have to be converted.
    // The aliasing re-wrap is keyed on the element type rendering as
    // `SmeltUnknown`, never on a library or a spelling.
    let source = source_for(
        r"
type Holder = { items: string[] } & { other: unknown };
function record(holder: Holder, value: string): void {
  const items = holder.items;
  items.push(value);
}
",
    );

    assert!(
        source.contains(
            "let smelt_value = SmeltUnknown::Array(items.clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect());"
        ),
        "{source}"
    );
}

/// An array literal whose destination is an OPTIONAL tuple renders a Rust
/// tuple, not a homogeneous `vec![..]`.
///
/// The tuple arm renders a literal into a Rust tuple when the destination is
/// one, but an optional destination has to reach it through the
/// `Type::Optional` recursion, and `Type::Tuple` was missing from that arm's
/// list. The literal then fell through to the `vec![..]` fallback, which infers
/// its element type from the FIRST element and rejects the rest -- hono's
/// `Matcher<T> = [RegExp, HandlerData<T>[][], StaticMap<T>]`, returned as
/// `Matcher<T> | null`, reported it as "expected SmeltRegExp, found
/// SmeltList<SmeltUnknown>" on element 2.
#[test]
fn an_array_literal_at_an_optional_tuple_renders_a_tuple() {
    let source = source_for(
        r"
type Triple = [string, number[], boolean];

export function build(label: string, flag: boolean): Triple | null {
  if (!flag) {
    return null;
  }
  return [label, [1, 2], flag];
}
",
    );
    assert_eq!(rust_items(&source, &["fn build"]), r"fn build(label: JsString, flag: bool) -> Option<(JsString, SmeltList<f64>, bool)> {
let _smelt_tmp_3: SmeltList<f64>;
let _smelt_tmp_4: Option<(JsString, SmeltList<f64>, bool)>;
let _smelt_tmp_2: bool = !(flag);
if _smelt_tmp_2 {
return None::<(JsString, SmeltList<f64>, bool)>;
} else {
_smelt_tmp_3 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
_smelt_tmp_4 = Some((label.clone(), _smelt_tmp_3.clone(), flag));
return _smelt_tmp_4;
}
}");
}

/// The same conversion declines when the source exposes no members at all.
///
/// Without that gate a dictionary source reported "no member" for every target
/// field and an all-optional record was built entirely out of absent fields — a
/// conversion that compiles and silently drops the value.
#[test]
fn a_record_target_is_not_built_from_a_source_with_no_members() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

interface FormatLong {
  date: (options: Options) => string;
}

export function run(formatLong: FormatLong): string {
  return formatLong.date({ width: "short" });
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn run"]), r#"fn run(format_long: FormatLong) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: SmeltRecord<JsString, JsString> = SmeltRecord::from([(JsString::from("width"), JsString::from("short"))]);
let _smelt_tmp_2: JsString = (format_long.date.clone())({ let smelt_record_map = _smelt_tmp_1.clone(); Options { width: smelt_record_map.get_text("width").cloned().map(|smelt_value| smelt_value) } });
return Ok(_smelt_tmp_2);
}"#);
}

/// An indexed read on a typed-array view is already an `f64`, so a `Number(..)`
/// over it must not run the erased numeric-conversion match.
///
/// The read's text and its type are decided by the same rule.
#[test]
fn a_typed_array_element_read_is_a_concrete_number() {
    let source = source_for(
        r#"
export function hex(bytes: Uint8Array): string {
  let out = "";
  for (let index = 0; index < bytes.length; index = index + 1) {
    out = out + Number(bytes[index]).toString(16);
  }
  return out;
}
"#,
    );
    assert_eq!(rust_items(&source, &["fn hex"]), r"fn hex(bytes: SmeltTypedArray) -> JsString {
let mut _smelt_tmp_3: f64;
let mut _smelt_tmp_4: bool;
let mut _smelt_tmp_5: f64;
let mut _smelt_tmp_6: JsString;
let mut _smelt_tmp_7: JsString;
let mut _smelt_tmp_8: f64;
let mut out: JsString = JsString::new();
let mut index: f64 = 0.0;
loop {
_smelt_tmp_3 = bytes.clone().length();
_smelt_tmp_4 = index < _smelt_tmp_3;
if !(_smelt_tmp_4) { break; }
_smelt_tmp_5 = bytes.get(index).unwrap_or(0.0);
_smelt_tmp_6 = JsString::from({ let smelt_value = (_smelt_tmp_5 as f64).trunc() as i128; let smelt_radix = ((16.0 as f64).trunc() as u32).clamp(2, 36); let smelt_negative = smelt_value < 0; let mut smelt_n = smelt_value.unsigned_abs(); let mut smelt_digits = Vec::new(); if smelt_n == 0 { smelt_digits.push('0'); } while smelt_n > 0 { let smelt_digit = (smelt_n % u128::from(smelt_radix)) as u8; smelt_digits.push(if smelt_digit < 10 { (b'0' + smelt_digit) as char } else { (b'a' + smelt_digit - 10) as char }); smelt_n /= u128::from(smelt_radix); } if smelt_negative { smelt_digits.push('-'); } smelt_digits.iter().rev().collect::<String>() });
_smelt_tmp_7 = out + &_smelt_tmp_6;
out = _smelt_tmp_7;
_smelt_tmp_8 = index + 1.0;
index = _smelt_tmp_8;
}
return out;
}");
}

#[test]
/// `Partial<X>` of a plain record is its own struct with every field optional,
/// and an object spread at a record type builds that struct field by field:
/// an optional source field writes only when present.
fn partial_records_and_typed_spreads_build_structs() {
    let source = source_for(
        r"
type Options = { origin: string; maxAge?: number };
export function merge(base: Options, options: Partial<Options>): Options {
  return { ...base, ...options };
}
",
    );
    assert_eq!(rust_items(&source, &["struct PartialOptions", "sig merge"]), r"struct PartialOptions {
origin: Option<JsString>,
max_age: Option<f64>,
}

fn merge(base: Options, options: PartialOptions) -> Options");
}

